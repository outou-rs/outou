//! Lowering a [`JsxAttribute`] to a Dioxus `rsx!` attribute (`name: value`
//! or, for a raw attribute, `"name": value`).
//!
//! The caller ([`crate::element`]) has already dropped attributes whose
//! value is an [`outou_syntax::ast::ErrorNode`] (decision: broken attribute
//! values are omitted, grammar §9), so this module only ever sees a `None`
//! (bare), `Text` or `Expression` value.

use outou_codegen::{Mode, Writer};
use outou_sourcemap::MappingKind;
use outou_syntax::ast::{Expr, Island, JsxAttribute, JsxAttributeValue, RustSource};
use outou_syntax::lexer::rust_token::{next_token, RtKind};

use crate::element::lower_island;
use crate::escape::{is_plain_ident, is_raw_identifiable_keyword, rust_string_literal};
use crate::expr_shape::parts_are_one_expression;
use crate::key::KeyBindings;

/// Lowers one attribute: its key, then `: `, then its value.
pub fn lower_attribute(
    writer: &mut Writer,
    source: &str,
    attribute: &JsxAttribute,
    keys: &KeyBindings,
    mode: Mode,
) {
    lower_key(writer, attribute);
    writer.raw(": ");
    // A `key` was evaluated before the `rsx!` call (`crate::key`); the
    // backend only sees the synthesized identifier, as a format string.
    if let Some(binding) = keys.identifier(attribute) {
        writer.raw(&format!("\"{{{binding}}}\""));
        return;
    }
    match &attribute.value {
        // A bare attribute is boolean (grammar §5.1): `<input disabled />`
        // behaves exactly as `disabled={true}` would.
        None => {
            writer.raw("true");
        }
        Some(JsxAttributeValue::Text(text)) => {
            let literal = rust_string_literal(&text.value);
            writer.mapped(&literal, &[text.span], MappingKind::Expression, None);
        }
        Some(JsxAttributeValue::Expression(island)) => {
            if let Some(rust) = bare_rust_expr(island) {
                // Decision 4(b), issue #8 fix list step 7: a single plain
                // Rust expression needs no synthesized braces of its own —
                // dropping them is what removes the file-wide
                // `#![allow(unused_braces)]` from a real-world file that
                // never uses a nested-JSX island prop.
                writer.verbatim(source, rust.span, MappingKind::Expression, None);
            } else {
                // Every other shape (nested JSX, a multi-part island, an
                // error node) keeps its braces — `writer.mark()` is what
                // tells `DioxusBackend::generate` this file needs the
                // allow (docs/backend-leakage.md row 21).
                writer.mark();
                lower_island(writer, source, island, mode);
            }
        }
        Some(JsxAttributeValue::Error(_)) => {
            unreachable!("caller (crate::element) filters out broken attribute values")
        }
    }
}

/// Lowers the attribute's name.
///
/// Grammar §5 allows a JSX attribute name to contain `-` or be any Rust
/// keyword (`<label for="id" />`, `<div data-id="x" />`). Neither is a
/// legal Rust field-key identifier:
///
/// - A keyword name [`is_raw_identifiable_keyword`] accepts (`type`,
///   `for`, …) is written as a raw identifier, `r#name: value` — verified
///   to compile for both elements and components (HIGH-3, issue #6 fix
///   list item 3).
/// - The four keywords Rust itself refuses as a raw identifier (`self`,
///   `Self`, `crate`, `super`) fall back to Dioxus's raw string-key
///   syntax, `"name": value` — which HIGH-3 found only exists for
///   elements, not components (`docs/backend-leakage.md`).
/// - A hyphenated name (`data-id`) has no raw-identifier form at all and
///   uses the same string-key syntax, with the same element-only caveat;
///   on a component it is unrepresentable (`docs/backend-leakage.md`).
fn lower_key(writer: &mut Writer, attribute: &JsxAttribute) {
    let name = &attribute.name.name;
    if is_plain_ident(name) {
        writer.mapped(name, &[attribute.name.span], MappingKind::Attribute, None);
    } else if is_raw_identifiable_keyword(name) {
        let raw = format!("r#{name}");
        writer.mapped(&raw, &[attribute.name.span], MappingKind::Attribute, None);
    } else {
        let literal = rust_string_literal(name);
        writer.mapped(
            &literal,
            &[attribute.name.span],
            MappingKind::Attribute,
            None,
        );
    }
}

/// Whether `island`'s value may be lowered as a bare Rust expression, with
/// none of the island's own synthesized `{`/`}` (issue #8 fix list step 7,
/// decision 4(b)): exactly one part, a plain [`Expr::Rust`] (never a
/// nested JSX element — a nested-`rsx!` island prop, `icon={<span/>}`,
/// needs its braces: `error: expected identifier` without them, and
/// `docs/backend-leakage.md` row 21 stays accurate for exactly this
/// shape), whose own source has no top-level `;` — i.e. it really is one
/// Rust *expression*, not a sequence of statements, which is the only
/// shape braces are provably redundant around. Verified brace-less and
/// lint-clean for an identifier, a closure, a struct literal and an
/// `if`-expression (`tabindex={x}`, `onclick={move |_| {…}}`,
/// `s={S { a: 1 }}`, `tabindex={if c { 1i64 } else { 2 }}`).
fn bare_rust_expr(island: &Island) -> Option<&RustSource> {
    match island.parts.as_slice() {
        [Expr::Rust(rust)] if !has_top_level_semicolon(&rust.text) => Some(rust),
        _ => None,
    }
}

/// Whether `text` holds any token other than comments (and whitespace): an
/// island with none, `{}` or `{/* note */}`, has no expression in it.
fn has_code(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut pos = 0usize;
    loop {
        let tok = next_token(bytes, pos);
        if tok.start == tok.end {
            return false;
        }
        if !matches!(tok.kind, RtKind::LineComment | RtKind::BlockComment) {
            return true;
        }
        pos = tok.end;
    }
}

/// Whether `text` (a Rust expression's own source) contains a `;` not
/// nested inside `(`, `[` or `{` — the shape of a Rust *statement*
/// sequence, which cannot be spliced into an expression position without
/// braces. Tokenizes with [`outou_syntax::lexer::rust_token`] (rather than
/// a naive byte scan) so a `;` inside a string, char or comment is never
/// mistaken for a statement separator.
fn has_top_level_semicolon(text: &str) -> bool {
    scan_semicolon(text, &mut 0)
}

/// Whether `island` is one Rust *expression* (never a statement sequence,
/// never empty or comment-only, never containing a broken part), however
/// many nested JSX elements it holds: the shape that can be passed as a
/// single call argument with no braces of its own (issue #27). Statements
/// that need no `;` (`if c {} "x"`, `fn f() {} f()`) are not an expression
/// either; see [`crate::expr_shape`].
pub(crate) fn is_single_expression(island: &Island) -> bool {
    let has_code = island.parts.iter().any(|part| match part {
        Expr::Rust(rust) => has_code(&rust.text),
        Expr::Jsx(_) => true,
        Expr::Error(_) => false,
    });
    let has_error = island
        .parts
        .iter()
        .any(|part| matches!(part, Expr::Error(_)));
    has_code && !has_error && parts_are_one_expression(&island.parts)
}

fn scan_semicolon(text: &str, depth: &mut i32) -> bool {
    let bytes = text.as_bytes();
    let mut pos = 0usize;
    loop {
        let tok = next_token(bytes, pos);
        if tok.start == tok.end {
            break;
        }
        match tok.kind {
            RtKind::OpenDelim => *depth += 1,
            RtKind::CloseDelim => *depth -= 1,
            RtKind::Punct if *depth == 0 && tok.text(text) == ";" => return true,
            _ => {}
        }
        pos = tok.end;
    }
    false
}
