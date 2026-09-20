//! Detects Rust/JSX content this crate's line-based reindentation cannot
//! safely touch: a multi-line string or raw-string literal, a multi-line
//! block comment, or multi-line JSX text.
//!
//! [`crate::snippet`]'s wrapper-unwrapping (`dedent_one_level`) and
//! [`crate::jsx_print::island`]'s block-layout indentation
//! (`indent_every_line`) both add or remove a fixed number of spaces from
//! *every physical line* of already-rendered text. That is correct for
//! structural indentation (each line's leading whitespace reflects brace
//! depth), but a token whose own content spans more than one physical
//! line does not follow that rule at all: the continuation line of a
//! multi-line string literal, for example, is part of the string's
//! *value* — its leading whitespace is user data, not structure — and
//! blindly adding or removing spaces there changes what the program
//! means, not just how it looks. The corruption compounds on every
//! subsequent format (a golden reproduction fixed by this module:
//! `tests/golden/multiline-raw-string-in-island/`).
//!
//! [`island_has_unsafe_multiline_content`] is the one check
//! [`crate::jsx_print::island::format_island`] runs before doing anything
//! else: if it is `true`, the island is left exactly as written
//! (`docs/phase0/issues/13-formatter.md`'s "leave verbatim" policy)
//! instead of being formatted at all — safer than trying to reindent
//! around the unsafe lines, and simple enough to keep correct.

use outou_syntax::ast;
use outou_syntax::lexer::rust_token::{next_token, RtKind};

/// Whether formatting `island` risks corrupting a multi-line token or
/// multi-line JSX text anywhere in its subtree (including inside a
/// nested JSX element's attributes, children, or further nested
/// islands, at any depth) — see the module doc for why this must be a
/// whole-subtree check rather than one scoped to the island's own
/// direct Rust content.
pub(crate) fn island_has_unsafe_multiline_content(island: &ast::Island, source: &str) -> bool {
    let text = &source[island.span.start as usize..island.span.end as usize];
    if has_multiline_token(text) {
        return true;
    }
    island
        .parts
        .iter()
        .any(|part| expr_has_multiline_jsx_text(part, source))
}

/// Scans `text` as a stream of Rust tokens (the same coarse tokenizer the
/// parser itself uses, `outou_syntax::lexer::rust_token`) and reports
/// whether any `Literal` (a plain or raw string can contain a literal
/// newline byte between its quotes — `"a\nb"` written with a real line
/// break, not the two-character escape `\n`) or `BlockComment` token
/// spans more than one physical line.
///
/// This is agnostic to whether `text` is "really" Rust or has JSX mixed
/// in: the tokenizer only looks for delimiters (quotes, `/*`) byte by
/// byte, so a multi-line JSX *attribute* string value (`title="aa\n  bb"`)
/// is found the same way a multi-line Rust string literal is, regardless
/// of how deeply nested the tag holding it is.
fn has_multiline_token(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut pos = 0usize;
    loop {
        let token = next_token(bytes, pos);
        if token.start == token.end {
            return false; // End of input.
        }
        if matches!(token.kind, RtKind::Literal | RtKind::BlockComment)
            && text[token.start..token.end].contains('\n')
        {
            return true;
        }
        pos = token.end;
    }
}

fn expr_has_multiline_jsx_text(expr: &ast::Expr, source: &str) -> bool {
    match expr {
        ast::Expr::Jsx(element) => element_has_multiline_jsx_text(element, source),
        ast::Expr::Rust(_) | ast::Expr::Error(_) => false,
    }
}

/// Whether any [`ast::JsxText`] child anywhere under `element` (recursing
/// through nested elements and, since a text-only tokenizer can never see
/// plain JSX text, through nested expression islands too) spans more than
/// one physical line.
fn element_has_multiline_jsx_text(element: &ast::JsxElement, source: &str) -> bool {
    element.children.iter().any(|child| match child {
        ast::JsxChild::Text(text) => {
            source[text.span.start as usize..text.span.end as usize].contains('\n')
        }
        ast::JsxChild::Element(nested) => element_has_multiline_jsx_text(nested, source),
        ast::JsxChild::Expression(island) => island
            .parts
            .iter()
            .any(|part| expr_has_multiline_jsx_text(part, source)),
        ast::JsxChild::Error(_) => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only_child_island(source: &str) -> ast::Island {
        let parsed = outou_syntax::parse(source);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let element = &parsed.file.jsx_elements()[0];
        match &element.children[0] {
            ast::JsxChild::Expression(island) => island.clone(),
            other => panic!("expected an expression child, got {other:?}"),
        }
    }

    #[test]
    fn a_plain_multiline_string_literal_is_unsafe() {
        let source = "fn f() { <div>{\"a\nb\"}</div> }";
        let island = only_child_island(source);
        assert!(island_has_unsafe_multiline_content(&island, source));
    }

    #[test]
    fn a_raw_multiline_string_literal_is_unsafe() {
        let source = "fn f() { <div>{r#\"a\nb\"#}</div> }";
        let island = only_child_island(source);
        assert!(island_has_unsafe_multiline_content(&island, source));
    }

    #[test]
    fn a_multiline_block_comment_is_unsafe() {
        let source = "fn f() { <div>{/* a\nb */ 1}</div> }";
        let island = only_child_island(source);
        assert!(island_has_unsafe_multiline_content(&island, source));
    }

    #[test]
    fn a_multiline_jsx_attribute_string_on_nested_jsx_is_unsafe() {
        let source = "fn f() { <div>{<span title=\"a\nb\" />}</div> }";
        let island = only_child_island(source);
        assert!(island_has_unsafe_multiline_content(&island, source));
    }

    #[test]
    fn multiline_jsx_text_is_unsafe() {
        let source = "fn f() { <div>{<span>a\nb</span>}</div> }";
        let island = only_child_island(source);
        assert!(island_has_unsafe_multiline_content(&island, source));
    }

    #[test]
    fn an_ordinary_single_line_island_is_safe() {
        let source = "fn f() { <div>{1 + 1}</div> }";
        let island = only_child_island(source);
        assert!(!island_has_unsafe_multiline_content(&island, source));
    }

    #[test]
    fn a_single_line_string_literal_is_safe() {
        let source = "fn f() { <div>{\"ab\"}</div> }";
        let island = only_child_island(source);
        assert!(!island_has_unsafe_multiline_content(&island, source));
    }
}
