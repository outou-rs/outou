//! Renders a JSX element's children, either all on one line (when the
//! opening tag is single-line, every child renders on a single line, no
//! blank line has to be kept, and the whole element fits within the line
//! width) or with each non-text child on its own line.
//!
//! **Blank lines follow Prettier.** A blank line between two non-text
//! children (elements and expression islands) is kept, and two or more
//! collapse into one, but only when the element has no text child at all:
//! Prettier drops every such blank line as soon as the element has
//! meaningful text among its children, because there the line breaks are
//! the text's own word separators. A blank line before the first child or
//! after the last one is dropped. Outou's one-line form is never chosen
//! while a blank line is kept. (Prettier has no such form for an element
//! with an element child or two expression children: it breaks those
//! regardless of blank lines.) Whitespace-only gaps that hold a blank line
//! are insignificant JSX whitespace (`docs/grammar.md` §8: a multi-line
//! whitespace-only run normalizes away), so keeping or dropping them never
//! changes the parsed tree or the generated Rust. An explicit `{" "}`
//! island counts as text for this rule, exactly as it does in Prettier.
//! `crates/outou-fmt/tests/golden/README.md` records the observed Prettier
//! behaviour and the goldens that pin it.
//!
//! **Multi-line text is re-indented, not reflowed.** See
//! [`reindent_text`]: only whitespace that §8 discards changes, so the text
//! value is the same.
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
use crate::ctx::Ctx;
use crate::error::FormatError;

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

// The parser guarantees a gap holds only U+0020, tab and line breaks; any
// other whitespace is a `Text` child (`docs/grammar.md` §8).
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
        ast::JsxTag::Named { span, .. } | ast::JsxTag::Fragment { span } => Some(span.end),
        ast::JsxTag::Incomplete(_) => None,
    }
}

fn close_tag_start(element: &ast::JsxElement) -> Option<u32> {
    match &element.close {
        Some(ast::JsxTag::Named { span, .. } | ast::JsxTag::Fragment { span }) => Some(span.start),
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

/// Whether `element` keeps a blank line before each of its children, by
/// child index. Entry `i` is `true` only if the element has no text child
/// (an explicit `{" "}` island counts as one), children `i - 1` and `i` are
/// both non-text, and the whitespace between them spans at least two line
/// breaks (a blank line, possibly with spaces or tabs on it).
fn blank_lines_before_children(element: &ast::JsxElement, source: &str) -> Vec<bool> {
    let mut flags = vec![false; element.children.len()];
    if element
        .children
        .iter()
        .any(|child| is_text_like(child, source))
    {
        return flags;
    }
    for (index, pair) in element.children.windows(2).enumerate() {
        let previous_end = child_outer_span(&pair[0]).end;
        let next_start = child_outer_span(&pair[1]).start;
        if previous_end < next_start
            && line_breaks(&source[previous_end as usize..next_start as usize]) >= 2
        {
            flags[index + 1] = true;
        }
    }
    flags
}

/// A text child, or an island holding only the string literal `" "` (JSX
/// whitespace written explicitly), which Prettier treats as text.
fn is_text_like(child: &ast::JsxChild, source: &str) -> bool {
    match child {
        ast::JsxChild::Text(_) => true,
        ast::JsxChild::Expression(island) => {
            source[island.span.start as usize..island.span.end as usize].trim() == "\" \""
        }
        ast::JsxChild::Element(_) | ast::JsxChild::Error(_) => false,
    }
}

/// Line breaks in `gap`, counting `\n`, `\r\n` and a lone `\r` once each.
fn line_breaks(gap: &str) -> usize {
    let mut count = 0;
    let mut chars = gap.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\n' => count += 1,
            '\r' => {
                count += 1;
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
            }
            _ => {}
        }
    }
    count
}

/// Attempts to render every one of `element`'s children as a single
/// inline string with no added whitespace between them (safe exactly
/// because [`ast::JsxElement::children`] already omits any gap that
/// normalized away to nothing — see the module doc). `None` means some
/// child could not be rendered on one line, and the caller must fall
/// back to [`format_multiline`]. A blank line that has to be kept (see the
/// module doc) also means `None`: it cannot be shown on one line.
pub(super) fn try_single_line(
    element: &ast::JsxElement,
    indent: usize,
    source: &str,
    options: &Ctx,
) -> Result<Option<String>, FormatError> {
    if blank_lines_before_children(element, source).contains(&true) {
        return Ok(None);
    }
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
    options: &Ctx,
) -> Result<Option<String>, FormatError> {
    let rendered = match child {
        ast::JsxChild::Text(text) => reindent_text(source_of(text, source), "", "", true, true),
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
/// line at `indent + options.indent_unit`, gluing directly around any `Text`
/// child (see the module doc for why). Includes the necessary leading
/// newline before the first non-text child and trailing newline before
/// the closing tag; a caller sandwiches the result directly between the
/// opening and closing tag text with nothing more added.
pub(super) fn format_multiline(
    element: &ast::JsxElement,
    indent: usize,
    source: &str,
    options: &Ctx,
) -> Result<String, FormatError> {
    let child_indent = indent + options.indent_unit;
    let pad = " ".repeat(child_indent);

    let blank_before = blank_lines_before_children(element, source);
    let mut out = String::new();
    let mut previous_was_text = false;
    for (index, child) in element.children.iter().enumerate() {
        match child {
            ast::JsxChild::Text(text) => {
                // The text's last line is either the closing tag's line (when
                // the text ends the element) or the line the next child starts on.
                let tail_pad = if index + 1 == element.children.len() {
                    " ".repeat(indent)
                } else {
                    pad.clone()
                };
                out.push_str(&reindent_text(
                    source_of(text, source),
                    &pad,
                    &tail_pad,
                    index == 0,
                    index + 1 == element.children.len(),
                ));
                previous_was_text = true;
            }
            other => {
                let rendered = render_owned_line(other, child_indent, source, options)?;
                if !previous_was_text {
                    // The flags are all false whenever a text child exists, so
                    // nesting this under `!previous_was_text` is only for clarity.
                    if blank_before[index] {
                        out.push('\n');
                    }
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
    options: &Ctx,
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

fn source_of<'a>(text: &ast::JsxText, source: &'a str) -> &'a str {
    &source[text.span.start as usize..text.span.end as usize]
}

/// The only characters `docs/grammar.md` §8 trims from the edges of a line:
/// U+0020 and tab (a tab becomes U+0020 before the trim). No-break space,
/// U+000B, U+000C and every other Unicode space are text and are never
/// touched.
const TRIMMED_BY_JSX: [char; 2] = [' ', '\t'];

/// Re-indents a text run that spans several lines, without reflowing it.
///
/// Only whitespace that `docs/grammar.md` §8 discards is changed, so the
/// run's JSX text value stays the same. Only U+0020 and tab are ever removed.
///
/// - **First line.** If it starts with U+0020 or tab, that whitespace is
///   significant (the line continues the one holding the preceding tag or
///   island): it is kept and the line stays where it is. If it starts with
///   anything else it moves onto a line of its own at `line_pad`, but only
///   when the run is the element's first child (`is_first`), so that it
///   follows the opening tag; the old first line is then whitespace-only
///   and §8 drops it. After a sibling child the line stays glued to it, as
///   Prettier keeps `<b>x</b>after` and `All ({x})` together. Trailing
///   whitespace is removed either way.
/// - **Middle lines.** Leading and trailing whitespace is removed and the
///   line starts at `line_pad`. An empty line is a blank line; two or more
///   in a row become one, and blank lines next to the tag or child before
///   the text, or the child or closing tag after it, are dropped (as
///   between element children, `docs/grammar.md` §8 step 4 discards them).
/// - **Last line.** Leading whitespace is removed. If it ends with U+0020 or
///   tab, that is significant: the line is kept at `line_pad` and what
///   follows is glued to it. When a sibling child follows (`!is_last`) the
///   child is glued to the text in the same way, whatever the line ends
///   with. Only when the run is the element's last child and its last line
///   has no trailing whitespace does the closing tag go onto its own line,
///   at `tail_pad`. An empty last line is a line start for what follows,
///   at `tail_pad`.
///
/// Words are never joined, split or moved between lines. A run on a single
/// line is returned as written.
fn reindent_text(
    raw: &str,
    line_pad: &str,
    tail_pad: &str,
    is_first: bool,
    is_last: bool,
) -> String {
    let lines = split_lines(raw);
    let [first, middle @ .., last] = lines.as_slice() else {
        return raw.to_string();
    };

    let first = first.trim_end_matches(TRIMMED_BY_JSX);
    let first_is_blank = first.is_empty();
    let first_moves = is_first && !first_is_blank && !first.starts_with(TRIMMED_BY_JSX);
    let last = last.trim_start_matches(TRIMMED_BY_JSX);
    let last_is_glued = !last.is_empty() && (!is_last || last.ends_with(TRIMMED_BY_JSX));

    // The lines between the first line's start and the tail, "" being a
    // blank line.
    let mut body: Vec<&str> = Vec::new();
    if first_moves {
        body.push(first);
    }
    body.extend(middle.iter().map(|line| line.trim_matches(TRIMMED_BY_JSX)));
    if !last_is_glued && !last.is_empty() {
        body.push(last);
    }
    if first_is_blank {
        while body.first() == Some(&"") {
            body.remove(0);
        }
    }
    if !last_is_glued {
        while body.last() == Some(&"") {
            body.pop();
        }
    }
    body.dedup_by(|line, previous| line.is_empty() && previous.is_empty());

    let mut out = if first_moves { "" } else { first }.to_string();
    for line in body {
        out.push('\n');
        if !line.is_empty() {
            out.push_str(line_pad);
            out.push_str(line);
        }
    }
    out.push('\n');
    if last_is_glued {
        out.push_str(line_pad);
        out.push_str(last);
    } else {
        out.push_str(tail_pad);
    }
    out
}

/// Splits on `\n`, `\r\n` and a lone `\r`, keeping empty lines. `\r\n` has
/// already been normalised to `\n` upstream (`crate::newline::normalize`), so
/// that arm only mirrors `docs/grammar.md` §8; a lone `\r` is what really
/// reaches here.
fn split_lines(raw: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = raw.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        let width = match c {
            '\n' => 1,
            '\r' if chars.peek().is_some_and(|&(_, next)| next == '\n') => {
                chars.next();
                2
            }
            '\r' => 1,
            _ => continue,
        };
        lines.push(&raw[start..at]);
        start = at + width;
    }
    lines.push(&raw[start..]);
    lines
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
        let out = format_multiline(&element, 0, source, &Ctx::default()).unwrap();
        assert!(out.contains("<a /> <b />"), "{out:?}");
    }

    #[test]
    fn two_element_children_with_no_text_get_their_own_lines() {
        let source = "fn f() { <div><a/><b/></div> }";
        let element = only_element(source);
        let out = format_multiline(&element, 0, source, &Ctx::default()).unwrap();
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

    #[test]
    fn a_blank_line_between_element_children_is_kept_once() {
        let source = "fn f() { <div><a/>\n\n\n\n<b/></div> }";
        let element = only_element(source);
        let out = format_multiline(&element, 0, source, &Ctx::default()).unwrap();
        assert_eq!(out, "\n    <a />\n\n    <b />\n");
    }

    #[test]
    fn leading_and_trailing_blank_lines_are_dropped() {
        let source = "fn f() { <div>\n\n<a/>\n\n</div> }";
        let element = only_element(source);
        let out = format_multiline(&element, 0, source, &Ctx::default()).unwrap();
        assert_eq!(out, "\n    <a />\n");
    }

    #[test]
    fn any_text_child_drops_every_blank_line() {
        let source = "fn f() { <div><a/>\n\n<b/> <c/></div> }";
        let element = only_element(source);
        let out = format_multiline(&element, 0, source, &Ctx::default()).unwrap();
        assert!(!out.contains("\n\n"), "{out:?}");
    }

    #[test]
    fn an_explicit_space_island_counts_as_text() {
        let source = "fn f() { <div><a/>\n\n{\" \"}<b/></div> }";
        let element = only_element(source);
        let out = format_multiline(&element, 0, source, &Ctx::default()).unwrap();
        assert!(!out.contains("\n\n"), "{out:?}");
    }

    #[test]
    fn a_kept_blank_line_rules_out_the_single_line_form() {
        let source = "fn f() { <div><a/>\n\n<b/></div> }";
        let element = only_element(source);
        assert_eq!(
            try_single_line(&element, 0, source, &Ctx::default()).unwrap(),
            None
        );
    }

    /// A run that is both the first and the last child of its element.
    fn alone(raw: &str, line_pad: &str, tail_pad: &str) -> String {
        reindent_text(raw, line_pad, tail_pad, true, true)
    }

    #[test]
    fn text_after_a_sibling_child_keeps_its_first_line_glued() {
        let out = reindent_text("after\n  more ", "    ", "  ", false, true);
        assert_eq!(out, "after\n    more ");
    }

    #[test]
    fn text_before_a_sibling_child_keeps_the_child_glued_to_its_last_line() {
        let out = reindent_text(" a\n  b", "    ", "  ", true, false);
        assert_eq!(out, " a\n    b");
        let out = reindent_text("a\n  b", "    ", "  ", true, false);
        assert_eq!(out, "\n    a\n    b");
        let out = reindent_text(" a\n  ", "    ", "  ", true, false);
        assert_eq!(out, " a\n  ");
    }

    #[test]
    fn text_between_two_siblings_only_gets_the_middle_re_indented() {
        let out = reindent_text("a\n\n\n   b\n   c", "    ", "  ", false, false);
        assert_eq!(out, "a\n\n    b\n    c");
    }

    #[test]
    fn reindent_text_leaves_a_single_line_run_alone() {
        assert_eq!(alone("  a  b  ", "    ", "  "), "  a  b  ");
    }

    #[test]
    fn reindent_text_only_trims_ascii_space_and_tab() {
        let out = alone(" a \t\n \t\u{a0}b\u{a0} \n\u{c} \n\t c\t", "    ", "  ");
        assert_eq!(out, " a\n    \u{a0}b\u{a0}\n    \u{c}\n    c\t");
    }

    #[test]
    fn reindent_text_treats_cr_lf_and_lone_cr_as_line_breaks() {
        assert_eq!(alone("a\r\n  b\r  c ", "  ", ""), "\n  a\n  b\n  c ");
    }

    #[test]
    fn an_empty_last_line_gets_the_tail_pad_and_a_glued_one_keeps_the_line_pad() {
        assert_eq!(alone(" a\n      ", "    ", "  "), " a\n  ");
        assert_eq!(alone(" a\n      b ", "    ", "  "), " a\n    b ");
    }

    #[test]
    fn a_last_line_without_trailing_whitespace_puts_what_follows_on_its_own_line() {
        assert_eq!(alone(" a\n  b", "    ", "  "), " a\n    b\n  ");
    }

    #[test]
    fn a_first_line_without_leading_whitespace_moves_to_its_own_line() {
        assert_eq!(alone("a\n  b ", "    ", "  "), "\n    a\n    b ");
        assert_eq!(alone(" a\n  b ", "    ", "  "), " a\n    b ");
    }

    #[test]
    fn blank_lines_inside_text_collapse_and_edge_ones_go() {
        assert_eq!(alone("\n\n a\n\n\n b\n\n", "  ", ""), "\n  a\n\n  b\n");
        // A blank line next to a first line that moved is an inner one.
        assert_eq!(alone("a\n\n b ", "  ", ""), "\n  a\n\n  b ");
    }

    #[test]
    fn a_lone_cr_inside_text_rules_out_the_single_line_form() {
        let source = "fn f() { <p>a\r b</p> }";
        let element = only_element(source);
        assert_eq!(
            try_single_line(&element, 0, source, &Ctx::default()).unwrap(),
            None
        );
    }

    #[test]
    fn line_breaks_counts_each_break_once() {
        assert_eq!(line_breaks("\n  \n"), 2);
        assert_eq!(line_breaks("\r\n\r\n"), 2);
        assert_eq!(line_breaks("\r\r"), 2);
        assert_eq!(line_breaks("   "), 0);
    }
}
