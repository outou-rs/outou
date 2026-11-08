//! An element's children: text, expression islands, and nested elements,
//! up to its matching closing tag or a terminator (grammar §2.1, §5).

use outou_sourcemap::Span;

use crate::ast;
use crate::lexer::disambiguate::NamedTag;
use crate::lexer::rust_token::next_significant;
use crate::lexer::scan_jsx_name;

use super::super::region::RegionEnd;
use super::super::Parser;
use super::{
    closing_slash_after_lt, incomplete_element, pos_span, skip_trivia, ChildrenOutcome,
    CloseResolution,
};

fn flush_text(children: &mut Vec<ast::JsxChild>, source: &str, start: usize, end: usize) {
    if start >= end {
        return;
    }
    if let Some(value) = crate::whitespace::normalize_jsx_text(&source[start..end]) {
        children.push(ast::JsxChild::Text(ast::JsxText {
            span: Span::new(start as u32, end as u32),
            value,
        }));
    }
}

impl<'s> Parser<'s> {
    /// Parses the children of an already-opened element (`>` was seen).
    pub(super) fn parse_jsx_children(
        &mut self,
        name: &str,
        start: usize,
    ) -> (
        Vec<ast::JsxChild>,
        Option<ast::JsxTag>,
        usize,
        ChildrenOutcome,
    ) {
        let mut children = Vec::new();
        let mut text_start = start;
        let mut pos = start;
        loop {
            match self.bytes.get(pos).copied() {
                None => {
                    flush_text(&mut children, self.source, text_start, pos);
                    return (children, None, pos, ChildrenOutcome::Terminated);
                }
                Some(b'}') => {
                    // §2.2 and §9 are compatible, not in tension: a stray
                    // `}` in `JsxText` (no island open) is both the
                    // terminator that unwinds every open JSX frame (§2.2 —
                    // it almost always closes the enclosing Rust block, so
                    // treating it as literal text would lose that block on
                    // recovery) *and* a byte the user most likely meant as
                    // literal text, which is worth its own diagnosis (§9)
                    // before the unwinding proceeds. Diagnosing here does
                    // not change what gets diagnosed next: the caller still
                    // sees `ChildrenOutcome::Terminated` and adds its own
                    // "missing closing tag" on top, exactly as before.
                    if self.terminator_diagnosed_at != Some(pos) {
                        self.push_diag_at(
                            pos_span(pos),
                            super::super::diag::stray_rbrace_in_text(),
                        );
                        self.terminator_diagnosed_at = Some(pos);
                    }
                    flush_text(&mut children, self.source, text_start, pos);
                    return (children, None, pos, ChildrenOutcome::Terminated);
                }
                Some(b'{') => {
                    flush_text(&mut children, self.source, text_start, pos);
                    let island_start = pos;
                    let scan = self.scan_region(pos + 1);
                    match scan.end {
                        RegionEnd::Eof { pos: eof_pos } => {
                            self.push_diag_at(
                                pos_span(island_start),
                                super::super::diag::eof_in_island(),
                            );
                            return (children, None, eof_pos, ChildrenOutcome::Terminated);
                        }
                        RegionEnd::Closed {
                            close_start,
                            close_end,
                        } => {
                            if !super::super::region::is_trivia_only(
                                self.bytes,
                                island_start + 1,
                                close_start,
                            ) {
                                let island = super::super::region::build_island(
                                    scan,
                                    island_start + 1,
                                    close_start,
                                );
                                children.push(ast::JsxChild::Expression(island));
                            }
                            pos = close_end;
                            text_start = pos;
                        }
                    }
                }
                Some(b'<')
                    if closing_slash_after_lt(self.bytes, pos).is_some_and(|slash| {
                        slash == pos + 1 || self.is_complete_closing(slash)
                    }) =>
                {
                    flush_text(&mut children, self.source, text_start, pos);
                    match self.resolve_closing_tag(pos, name) {
                        CloseResolution::Resolved { close, end } => {
                            return (children, close, end, ChildrenOutcome::Resolved);
                        }
                        CloseResolution::Terminated { end } => {
                            return (children, None, end, ChildrenOutcome::Terminated);
                        }
                    }
                }
                Some(b'<') => {
                    flush_text(&mut children, self.source, text_start, pos);
                    let (element, new_pos) = self.parse_nested_child_element(pos);
                    children.push(ast::JsxChild::Element(element));
                    pos = new_pos;
                    text_start = pos;
                }
                Some(_) => {
                    pos += 1;
                }
            }
        }
    }

    /// A `<` seen while already inside `JsxText`: always a nested element
    /// (grammar §2.1), so this is a JSX-only classifier with no
    /// Rust-vs-JSX ambiguity to resolve — unlike the top-level entry point
    /// (`lexer::disambiguate::classify`), it only has to recognize the
    /// shapes that are still ambiguous *within* JSX itself
    /// (a fragment, and a tag name followed by explicit generic
    /// arguments, M5) before delegating to the ordinary named-element
    /// parser, which already handles every other reserved shape (dotted
    /// names, namespaced names, spread attributes, `Self`) uniformly for
    /// both nested and top-level tags.
    fn parse_nested_child_element(&mut self, lt_pos: usize) -> (ast::JsxElement, usize) {
        // Trivia between `<` and the name is skipped, as at the top level
        // (grammar §4 rule 2) and as React accepts: `< span/>` is `<span/>`.
        let name_start = skip_trivia(self.bytes, lt_pos + 1);
        if self.bytes.get(name_start) == Some(&b'>') {
            return self.parse_fragment(lt_pos);
        }
        let name_end = scan_jsx_name(self.bytes, name_start);
        let after_name = next_significant(self.bytes, name_end);
        let generic_args_skipped_at =
            if name_end > name_start && matches!(self.bytes.get(after_name.start), Some(b'<')) {
                Some(after_name.start)
            } else {
                None
            };
        let resume_at = match generic_args_skipped_at {
            Some(inner_lt) => {
                crate::lexer::disambiguate::skip_nested_generic_arguments(self.bytes, inner_lt)
            }
            None => name_end,
        };
        if name_end == name_start
            || self.looks_like_text_after_spaced_lt(lt_pos, name_start, resume_at)
        {
            // No valid name at all (`<3>`, `<<A/>`, `< 3`), or a literal `<`
            // in text (`a < b`): diagnose it, then recover by consuming just
            // the `<` as an error and resuming text scanning right after
            // it. Every incomplete tag carries a diagnostic (parser/mod.rs).
            self.push_diag(
                Span::new(lt_pos as u32, lt_pos as u32 + 1),
                super::super::diag::missing_tag_name_after_lt(
                    self.bytes[name_start..].starts_with(b"!--"),
                ),
            );
            return (
                incomplete_element(lt_pos, lt_pos + 1, "a tag name"),
                lt_pos + 1,
            );
        }
        self.parse_named_element(
            lt_pos,
            NamedTag {
                name_start,
                name_end,
                resume_at,
                generic_args_skipped_at,
            },
        )
    }

    /// Whether the `/` at `slash`, which has trivia between it and its `<`,
    /// begins a complete `/name>` (only whitespace around the name). Anything
    /// else (`a < /b</p>`, `a < /b and c > d`) is a literal `<` in text. The
    /// name is required here: the closing tag of a fragment, `</>` or `</ >`,
    /// has its `/` right after the `<` and never reaches this check, so a
    /// spaced `< />` in prose stays text.
    fn is_complete_closing(&self, slash: usize) -> bool {
        let name_start = skip_trivia(self.bytes, slash + 1);
        let name_end = scan_jsx_name(self.bytes, name_start);
        name_end > name_start && self.bytes.get(super::skip_ws(self.bytes, name_end)) == Some(&b'>')
    }

    /// Whether `pos` (right after a tag name) starts an attribute assignment:
    /// a name, `=`, then a `"` string or a `{` island. Only this shape makes
    /// an unfinished head a tag rather than text; prose such as `a < b = c`,
    /// `while n < max { n += 1 }` or `x < y then say "hi"` never matches it.
    fn attribute_assignment_follows(&self, pos: usize) -> bool {
        let attr = skip_trivia(self.bytes, pos);
        let attr_end = scan_jsx_name(self.bytes, attr);
        if attr_end == attr {
            return false;
        }
        let eq = next_significant(self.bytes, attr_end);
        if eq.end != eq.start + 1 || self.bytes.get(eq.start) != Some(&b'=') {
            return false;
        }
        matches!(
            self.bytes.get(skip_trivia(self.bytes, eq.end)),
            Some(b'"' | b'{')
        )
    }

    /// Recovery heuristic for `a < b` text now that `< name` is a tag in
    /// child position: a `<` followed by trivia and a name whose tag head
    /// never reaches `>` (it runs into a `<`, an unbalanced `}` or the end
    /// of the file first) is reported as a literal `<` in text, with the
    /// "write `{"<"}`" advice, instead of as a broken tag.
    ///
    /// A head that starts with an attribute assignment (`name="…"`,
    /// `name={…}`) stays a tag, so `< span class="x"` reports like `<span`.
    ///
    /// This only picks which diagnostic an already-invalid file gets: a
    /// tag head that never reaches `>` is an error whichever way it is
    /// read, so no valid file changes meaning. A `<` with no space
    /// (`a<b`) keeps the ordinary tag diagnostics.
    fn looks_like_text_after_spaced_lt(
        &self,
        lt_pos: usize,
        name_start: usize,
        head_start: usize,
    ) -> bool {
        if name_start == lt_pos + 1 || self.attribute_assignment_follows(head_start) {
            return false;
        }
        let mut pos = head_start;
        loop {
            let tok = next_significant(self.bytes, pos);
            if tok.start == tok.end {
                return true;
            }
            match self.bytes[tok.start] {
                b'>' => return false,
                b'<' | b'}' => return true,
                b'{' => pos = crate::lexer::rust_token::skip_balanced_group(self.bytes, tok.start),
                _ => pos = tok.end,
            }
        }
    }
}
