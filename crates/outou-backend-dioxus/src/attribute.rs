//! Lowering a [`JsxAttribute`] to a Dioxus `rsx!` attribute (`name: value`
//! or, for a raw attribute, `"name": value`).
//!
//! The caller ([`crate::element`]) has already dropped attributes whose
//! value is an [`outou_syntax::ast::ErrorNode`] (decision: broken attribute
//! values are omitted, grammar §9), so this module only ever sees a `None`
//! (bare), `Text` or `Expression` value.

use outou_codegen::{Mode, Writer};
use outou_sourcemap::MappingKind;
use outou_syntax::ast::{Island, JsxAttribute, JsxAttributeValue};

use crate::element::lower_island;
use crate::escape::{
    is_plain_ident, is_raw_identifiable_keyword, key_interpolation_literal, rust_string_literal,
};

/// Lowers one attribute: its key, then `: `, then its value.
pub fn lower_attribute(writer: &mut Writer, source: &str, attribute: &JsxAttribute, mode: Mode) {
    lower_key(writer, attribute);
    writer.raw(": ");
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
            if attribute.name.name == "key" {
                lower_key_value(writer, source, island);
            } else {
                lower_island(writer, source, island, mode);
            }
        }
        Some(JsxAttributeValue::Error(_)) => {
            unreachable!("caller (crate::element) filters out broken attribute values")
        }
    }
}

/// Lowers `key={expr}`'s value (HIGH-2, issue #6 fix list item 2):
/// Dioxus's `rsx!` requires `key: "{value}"`, a format-string literal,
/// not the bare island form every other attribute uses. `expr`'s own
/// source text is spliced verbatim inside `{…}` so the expression keeps
/// whatever type it already had — the ledger records that it must be
/// `Display` and must not itself contain `{`/`}` (`docs/backend-leakage.md`).
fn lower_key_value(writer: &mut Writer, source: &str, island: &Island) {
    let expr_source = &source[island.span.start as usize..island.span.end as usize];
    let literal = key_interpolation_literal(expr_source);
    writer.mapped(&literal, &[island.span], MappingKind::Expression, None);
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
