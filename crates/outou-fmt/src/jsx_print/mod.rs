//! The JSX-aware pretty printer: everything that decides how one
//! [`ast::JsxElement`] itself is laid out, once the plain Rust around it
//! has already gone through `rustfmt` (`crate::snippet`).
//!
//! Split into [`tag`] (the opening tag and its attributes), [`children`]
//! (deciding single-line vs. one-child-per-line, and rendering each
//! child), and [`island`] (an expression island's own placeholder ->
//! `rustfmt` -> splice pass, reusing [`crate::snippet`]).
//!
//! **Text is never reflowed.** Every [`ast::JsxText`] child is emitted as
//! the exact source bytes of its span, never its normalized `.value` and
//! never with injected surrounding whitespace: `ast::JsxText::span`
//! always covers the *entire* maximal run between two boundaries
//! (`docs/grammar.md` §8), so there is never spare, purely-insignificant
//! whitespace directly adjacent to a text node left over to reformat.
//! [`children::format_multiline`]'s doc comment spells out why adding a
//! newline next to one anyway would be unsafe.

mod children;
mod island;
mod tag;

use outou_syntax::ast;

use crate::error::FormatError;
use crate::width::fits;
use crate::FormatOptions;

/// Formats one JSX element, starting at column `indent`. The returned
/// text's first line has no leading indentation (the caller is always
/// splicing this in right where something else left off — after a
/// placeholder, after a sibling); every other line is indented to at
/// least `indent`.
pub(crate) fn format_jsx_element(
    element: &ast::JsxElement,
    indent: usize,
    source: &str,
    options: &FormatOptions,
) -> Result<String, FormatError> {
    let name = element_name(element)?;
    let open = tag::format_open_tag(element, indent, source, options)?;
    if element.close.is_none() {
        // Self-closing: no children, no closing tag.
        return Ok(open);
    }

    // `ast::JsxElement::children` omits an "empty island" (whitespace
    // and/or a comment only, e.g. `{/* note */}`) entirely — grammar §6
    // says it "produces no child", which is correct for codegen but
    // means this pretty printer cannot safely rebuild the children
    // region purely from the child list: doing so would silently drop
    // the comment. When that happens (or anything else the AST does not
    // model shows up in a gap between children), the whole children
    // region is kept byte-for-byte instead — `docs/phase0/issues/13-formatter.md`:
    // "if a construct cannot be formatted safely, leave that JSX region
    // verbatim".
    if let Some(verbatim) = children::verbatim_if_unmodeled_content(element, source) {
        return Ok(format!("{open}{verbatim}</{name}>"));
    }

    if !open.contains('\n') {
        if let Some(inline) = children::try_single_line(element, indent, source, options)? {
            let candidate = format!("{open}{inline}</{name}>");
            if !candidate.contains('\n') && fits(indent, candidate.chars().count()) {
                return Ok(candidate);
            }
        }
    }

    let body = children::format_multiline(element, indent, source, options)?;
    Ok(format!("{open}{body}</{name}>"))
}

/// The element or component name of `element`'s opening tag.
///
/// Returns [`FormatError::Internal`] for an incomplete tag rather than
/// panicking; this should never actually be reached because
/// [`crate::format_source`] refuses any file containing an
/// [`ast::ErrorNode`] before formatting begins.
pub(crate) fn element_name(element: &ast::JsxElement) -> Result<&str, FormatError> {
    match &element.open {
        ast::JsxTag::Named { name, .. } => Ok(name.name.as_str()),
        ast::JsxTag::Incomplete(_) => Err(FormatError::Internal(
            "cannot format a JSX element with an incomplete opening tag".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only_element(source: &str) -> ast::JsxElement {
        let parsed = outou_syntax::parse(source);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let elements = parsed.file.jsx_elements();
        assert_eq!(elements.len(), 1);
        elements[0].clone()
    }

    #[test]
    fn formats_a_self_closing_element_with_no_attributes() {
        let source = "fn f() { <div/> }";
        let element = only_element(source);
        let out = format_jsx_element(&element, 0, source, &FormatOptions::default()).unwrap();
        assert_eq!(out, "<div />");
    }

    #[test]
    fn formats_an_empty_element_with_a_closing_tag() {
        let source = "fn f() { <div></div> }";
        let element = only_element(source);
        let out = format_jsx_element(&element, 0, source, &FormatOptions::default()).unwrap();
        assert_eq!(out, "<div></div>");
    }
}
