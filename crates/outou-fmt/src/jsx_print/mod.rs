//! The JSX-aware pretty printer: everything that decides how one
//! [`ast::JsxElement`] itself is laid out, once the plain Rust around it
//! has already gone through `rustfmt` (`crate::snippet`).
//!
//! Split into [`tag`] (the opening tag and its attributes), [`children`]
//! (deciding single-line vs. one-child-per-line, and rendering each
//! child), and [`island`] (an expression island's own placeholder ->
//! `rustfmt` -> splice pass, reusing [`crate::snippet`]).
//!
//! **Text is never reflowed, only re-indented.** Every [`ast::JsxText`]
//! child is emitted from the source bytes of its span, never its normalized
//! `.value` and never with injected surrounding whitespace:
//! `ast::JsxText::span` always covers the *entire* maximal run between two
//! boundaries (`docs/grammar.md` §8), so there is never spare,
//! purely-insignificant whitespace directly adjacent to a text node.
//! [`children::format_multiline`]'s doc comment spells out why adding a
//! newline next to one would be unsafe. A run on one line is copied as
//! written. A run on several lines has the whitespace that §8 discards
//! (leading on every line but the first, trailing on every line but the
//! last, whole whitespace-only lines; only U+0020 and tab) rewritten so the
//! lines follow the element's indentation, extra blank lines are collapsed,
//! and the opening and closing tags get their own lines when the text has
//! no significant edge whitespace next to them (text glued to a sibling
//! child stays glued); words are never joined, split or moved between lines
//! and its text value is unchanged (`children::reindent_text`).

mod children;
mod island;
mod tag;

use outou_syntax::ast;

use crate::ctx::Ctx;
use crate::error::FormatError;

/// Where an element sits on the line it starts on, for the question "does
/// it fit": how many columns of other text come before it on that line
/// beyond the `indent` it is formatted at (`lead`, for example the width of
/// `let x = `) and how many follow it (`tail`, for example the `;` of
/// `let x = <div />;`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Placement {
    pub(crate) lead: usize,
    pub(crate) tail: usize,
}

impl Placement {
    /// An element that starts its own line at the indent it is formatted
    /// at, with nothing after it (a child element, an element on its own
    /// line).
    pub(crate) const OWN_LINE: Placement = Placement { lead: 0, tail: 0 };
}

/// Formats one JSX element on a line of its own, starting at column
/// `indent`. See [`format_jsx_element_at`].
pub(crate) fn format_jsx_element(
    element: &ast::JsxElement,
    indent: usize,
    source: &str,
    options: &Ctx,
) -> Result<String, FormatError> {
    format_jsx_element_at(element, indent, Placement::OWN_LINE, source, options)
}

/// Formats one JSX element, continuation lines indented from `indent`. The
/// returned text's first line has no leading indentation (the caller is
/// always splicing this in right where something else left off — after a
/// placeholder, after a sibling); every other line is indented to at least
/// `indent`. `placement` says where on its line the first line will start,
/// so that a one-line form is only chosen if the whole line fits.
pub(crate) fn format_jsx_element_at(
    element: &ast::JsxElement,
    indent: usize,
    placement: Placement,
    source: &str,
    options: &Ctx,
) -> Result<String, FormatError> {
    let name = element_name(element)?;
    if tag_trivia_has_comment(element, source) {
        // A comment between `<` and a tag name (`< /* c */ span/>`,
        // `</ /* c */ span>`) is trivia the AST does not model; rebuilding
        // the tags would silently drop it, so the element stays as written.
        return Ok(source[element.span.start as usize..element.span.end as usize].to_string());
    }
    let open = tag::format_open_tag(element, indent, placement, source, options)?;
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
            if !candidate.contains('\n')
                && options.fits(
                    indent + placement.lead,
                    candidate.chars().count() + placement.tail,
                )
            {
                return Ok(candidate);
            }
        }
    }

    let body = children::format_multiline(element, indent, source, options)?;
    Ok(format!("{open}{body}</{name}>"))
}

/// Whether a comment sits between the `<` of a tag and its name, or between
/// its name and the closing `>` of a closing tag (grammar §4 rule 2 skips
/// trivia there). The pretty printer rebuilds those tags from the AST and
/// would lose the comment.
fn tag_trivia_has_comment(element: &ast::JsxElement, source: &str) -> bool {
    let has_comment = |start: u32, end: u32| {
        source
            .get(start as usize..end as usize)
            .is_some_and(|gap| gap.contains("/*") || gap.contains("//"))
    };
    let open_gap = match &element.open {
        ast::JsxTag::Named { name, .. } => has_comment(element.span.start, name.span.start),
        // `< /* c */ >`: the whole tag is trivia around nothing.
        ast::JsxTag::Fragment { span } => has_comment(span.start, span.end),
        ast::JsxTag::Incomplete(_) => false,
    };
    let close_gap = match &element.close {
        Some(ast::JsxTag::Named { span, name }) => {
            has_comment(span.start, name.span.start) || has_comment(name.span.end, span.end)
        }
        Some(ast::JsxTag::Fragment { span }) => has_comment(span.start, span.end),
        _ => false,
    };
    open_gap || close_gap
}

/// The element or component name of `element`'s opening tag; empty for a
/// fragment, so that `<{name}>` and `</{name}>` print `<>` and `</>`.
///
/// Returns [`FormatError::Internal`] for an incomplete tag rather than
/// panicking; this should never actually be reached because
/// [`crate::format_source`] refuses any file containing an
/// [`ast::ErrorNode`] before formatting begins.
pub(crate) fn element_name(element: &ast::JsxElement) -> Result<&str, FormatError> {
    match &element.open {
        ast::JsxTag::Named { name, .. } => Ok(name.name.as_str()),
        ast::JsxTag::Fragment { .. } => Ok(""),
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
        let out = format_jsx_element(&element, 0, source, &Ctx::default()).unwrap();
        assert_eq!(out, "<div />");
    }

    #[test]
    fn formats_an_empty_element_with_a_closing_tag() {
        let source = "fn f() { <div></div> }";
        let element = only_element(source);
        let out = format_jsx_element(&element, 0, source, &Ctx::default()).unwrap();
        assert_eq!(out, "<div></div>");
    }

    #[test]
    fn prints_a_fragment_with_its_own_tags() {
        let source = "fn f() { <  >  <p>x</p></  > }";
        let element = only_element_with_children(source);
        let out = format_jsx_element(&element, 0, source, &Ctx::default()).unwrap();
        assert_eq!(out, "<>  <p>x</p></>");
    }

    #[test]
    fn keeps_an_empty_and_a_single_child_fragment() {
        for (source, expected) in [
            ("fn f() { <></> }", "<></>"),
            ("fn f() { <><p/></> }", "<><p /></>"),
        ] {
            let parsed = outou_syntax::parse(source);
            let element = parsed.file.jsx_elements()[0].clone();
            let out = format_jsx_element(&element, 0, source, &Ctx::default()).unwrap();
            assert_eq!(out, expected);
        }
    }

    #[test]
    fn a_comment_inside_a_fragment_tag_keeps_the_element_as_written() {
        let source = "fn f() { < /* keep */ >x</ /* me */ > }";
        let parsed = outou_syntax::parse(source);
        let element = parsed.file.jsx_elements()[0].clone();
        let out = format_jsx_element(&element, 0, source, &Ctx::default()).unwrap();
        assert_eq!(out, "< /* keep */ >x</ /* me */ >");
    }

    fn only_element_with_children(source: &str) -> ast::JsxElement {
        let parsed = outou_syntax::parse(source);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        parsed.file.jsx_elements()[0].clone()
    }
}
