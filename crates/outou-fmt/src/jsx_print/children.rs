//! Renders a JSX element's children, either all on one line (when there
//! is at most one child and it fits) or with each non-text child on its
//! own line.
//!
//! **Why a `Text` child is never given extra surrounding whitespace.**
//! [`ast::JsxText::span`] always covers the *entire* maximal run of text
//! between two boundaries (`docs/grammar.md` §8) — never a partial gap
//! with insignificant whitespace left over on either side. So if a
//! `Text` child sits next to a structural sibling, that adjacency in the
//! original source is exactly reproduced by concatenating their rendered
//! forms directly, and inserting a newline there instead would hand the
//! whitespace algorithm a *longer* run than the source had: a single
//! significant space between two elements (`<a/> <b/>`, kept because its
//! one-line run is never trimmed) becomes insignificant the moment a line
//! break follows it (a multi-line run trims every line to nothing). This
//! module never risks that: a newline is added before or after a
//! structural child only when neither of its neighbors in
//! [`ast::JsxElement::children`] is a `Text` node.

use outou_sourcemap::Span;
use outou_syntax::ast;

use super::{format_jsx_element, island};
use crate::error::FormatError;
use crate::width::INDENT_UNIT;
use crate::FormatOptions;

/// If the raw source between `element`'s opening `>` and closing `<`
/// contains anything the child list does not account for (only ever an
/// empty island — whitespace and/or a Rust comment, `docs/grammar.md`
/// §6 — since every other byte range is exactly partitioned by a child;
/// see `docs/grammar.md` §9's round-trip contract), returns that whole
/// region's raw source unchanged. Returns `None` when every gap between
/// children (and before the first / after the last) is pure whitespace,
/// meaning the ordinary child-by-child renderer can safely rebuild it.
pub(super) fn verbatim_if_unmodeled_content(
    element: &ast::JsxElement,
    source: &str,
) -> Option<String> {
    let open_end = open_tag_end(element)?;
    let close_start = close_tag_start(element)?;

    let mut cursor = open_end;
    for child in &element.children {
        let span = child_outer_span(child);
        if !is_whitespace_gap(source, cursor, span.start) {
            return Some(source[open_end as usize..close_start as usize].to_string());
        }
        cursor = span.end;
    }
    if !is_whitespace_gap(source, cursor, close_start) {
        return Some(source[open_end as usize..close_start as usize].to_string());
    }
    None
}

fn is_whitespace_gap(source: &str, start: u32, end: u32) -> bool {
    if start >= end {
        return true;
    }
    source[start as usize..end as usize]
        .chars()
        .all(|c| c.is_whitespace())
}

fn open_tag_end(element: &ast::JsxElement) -> Option<u32> {
    match &element.open {
        ast::JsxTag::Named { span, .. } => Some(span.end),
        ast::JsxTag::Incomplete(_) => None,
    }
}

fn close_tag_start(element: &ast::JsxElement) -> Option<u32> {
    match &element.close {
        Some(ast::JsxTag::Named { span, .. }) => Some(span.start),
        _ => None,
    }
}

/// The byte range a child occupies in the source, *including* the
/// surrounding `{`/`}` for an expression island — [`ast::Island::span`]
/// only covers the content between them, but the braces themselves are
/// still part of the gap accounting here.
fn child_outer_span(child: &ast::JsxChild) -> Span {
    match child {
        ast::JsxChild::Text(text) => text.span,
        ast::JsxChild::Expression(island) => Span::new(island.span.start - 1, island.span.end + 1),
        ast::JsxChild::Element(element) => element.span,
        ast::JsxChild::Error(error) => error.span,
    }
}

/// Attempts to render every one of `element`'s children as a single
/// inline string with no added whitespace between them (safe exactly
/// because [`ast::JsxElement::children`] already omits any gap that
/// normalized away to nothing — see the module doc). `None` means some
/// child could not be rendered on one line, and the caller must fall
/// back to [`format_multiline`].
pub(super) fn try_single_line(
    element: &ast::JsxElement,
    indent: usize,
    source: &str,
    options: &FormatOptions,
) -> Result<Option<String>, FormatError> {
    let mut out = String::new();
    for child in &element.children {
        match render_inline(child, indent, source, options)? {
            Some(rendered) => out.push_str(&rendered),
            None => return Ok(None),
        }
    }
    Ok(Some(out))
}

/// Renders one child as a single line with no leading or trailing
/// newline, or returns `None` if it cannot be (its own formatting needed
/// more than one line).
fn render_inline(
    child: &ast::JsxChild,
    indent: usize,
    source: &str,
    options: &FormatOptions,
) -> Result<Option<String>, FormatError> {
    let rendered = match child {
        ast::JsxChild::Text(text) => verbatim_text(text, source),
        ast::JsxChild::Expression(value) => island::format_island(value, indent, source, options)?,
        ast::JsxChild::Element(nested) => format_jsx_element(nested, indent, source, options)?,
        ast::JsxChild::Error(_) => return Ok(None),
    };
    if rendered.contains('\n') {
        Ok(None)
    } else {
        Ok(Some(rendered))
    }
}

/// Renders every child of `element`, each non-text child on its own
/// line at `indent + INDENT_UNIT`, gluing directly around any `Text`
/// child (see the module doc for why). Includes the necessary leading
/// newline before the first non-text child and trailing newline before
/// the closing tag; a caller sandwiches the result directly between the
/// opening and closing tag text with nothing more added.
pub(super) fn format_multiline(
    element: &ast::JsxElement,
    indent: usize,
    source: &str,
    options: &FormatOptions,
) -> Result<String, FormatError> {
    let child_indent = indent + INDENT_UNIT;
    let pad = " ".repeat(child_indent);

    let mut out = String::new();
    let mut previous_was_text = false;
    for child in &element.children {
        match child {
            ast::JsxChild::Text(text) => {
                out.push_str(&verbatim_text(text, source));
                previous_was_text = true;
            }
            other => {
                let rendered = render_owned_line(other, child_indent, source, options)?;
                if !previous_was_text {
                    out.push('\n');
                    out.push_str(&pad);
                }
                out.push_str(&rendered);
                previous_was_text = false;
            }
        }
    }
    if !previous_was_text {
        out.push('\n');
        out.push_str(&" ".repeat(indent));
    }
    Ok(out)
}

fn render_owned_line(
    child: &ast::JsxChild,
    indent: usize,
    source: &str,
    options: &FormatOptions,
) -> Result<String, FormatError> {
    match child {
        ast::JsxChild::Text(_) => unreachable!("text children are handled by the caller"),
        ast::JsxChild::Expression(value) => island::format_island(value, indent, source, options),
        ast::JsxChild::Element(nested) => format_jsx_element(nested, indent, source, options),
        // Never reached: `crate::format_source` refuses files containing
        // an error node. Preserved verbatim rather than panicking if
        // that invariant is ever violated.
        ast::JsxChild::Error(error) => {
            Ok(source[error.span.start as usize..error.span.end as usize].to_string())
        }
    }
}

fn verbatim_text(text: &ast::JsxText, source: &str) -> String {
    source[text.span.start as usize..text.span.end as usize].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only_element(source: &str) -> ast::JsxElement {
        let parsed = outou_syntax::parse(source);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        parsed.file.jsx_elements()[0].clone()
    }

    #[test]
    fn a_significant_single_space_between_elements_is_never_touched() {
        // `<a/> <b/>` — the space is one line, so it is kept per
        // `docs/grammar.md` §8. format_multiline must reproduce it
        // exactly rather than adding a newline that would make the
        // (now two-line) run trim away to nothing on reparse.
        let source = "fn f() { <div><a/> <b/></div> }";
        let element = only_element(source);
        let out = format_multiline(&element, 0, source, &FormatOptions::default()).unwrap();
        assert!(out.contains("<a /> <b />"), "{out:?}");
    }

    #[test]
    fn two_element_children_with_no_text_get_their_own_lines() {
        let source = "fn f() { <div><a/><b/></div> }";
        let element = only_element(source);
        let out = format_multiline(&element, 0, source, &FormatOptions::default()).unwrap();
        assert_eq!(out, "\n    <a />\n    <b />\n");
    }

    /// An empty island (a comment, per grammar §6) produces no
    /// [`ast::JsxChild`] at all, so the ordinary renderer would silently
    /// drop it if it tried to rebuild the children region from the child
    /// list alone. `verbatim_if_unmodeled_content` must catch this.
    #[test]
    fn a_comment_between_children_forces_the_verbatim_fallback() {
        let source = "fn f() { <div>{/* keep me */}<span/></div> }";
        let element = only_element(source);
        let verbatim = verbatim_if_unmodeled_content(&element, source);
        assert_eq!(verbatim.as_deref(), Some("{/* keep me */}<span/>"));
    }

    #[test]
    fn no_verbatim_fallback_when_every_gap_is_plain_whitespace() {
        let source = "fn f() { <div>\n    <a/>\n</div> }";
        let element = only_element(source);
        assert_eq!(verbatim_if_unmodeled_content(&element, source), None);
    }
}
