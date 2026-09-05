//! An element's children: text, expression islands, and nested elements,
//! up to its matching closing tag or a terminator (grammar §2.1, §5).

use outou_sourcemap::Span;

use crate::ast;
use crate::lexer::disambiguate::NamedTag;
use crate::lexer::rust_token::next_significant;
use crate::lexer::scan_jsx_name;

use super::super::region::RegionEnd;
use super::super::Parser;
use super::{incomplete_element, pos_span, ChildrenOutcome, CloseResolution};

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
                Some(b'<') if self.bytes.get(pos + 1) == Some(&b'/') => {
                    flush_text(&mut children, self.source, text_start, pos);
                    match self.resolve_closing_tag(pos, name) {
                        CloseResolution::Resolved { close, end } => {
                            return (children, close, end, ChildrenOutcome::Resolved);
                        }
                        CloseResolution::Terminated => {
                            return (children, None, pos, ChildrenOutcome::Terminated);
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
    /// reserved shapes that are still ambiguous *within* JSX itself
    /// (a fragment, and a tag name followed by explicit generic
    /// arguments, M5) before delegating to the ordinary named-element
    /// parser, which already handles every other reserved shape (dotted
    /// names, namespaced names, spread attributes, `Self`) uniformly for
    /// both nested and top-level tags.
    fn parse_nested_child_element(&mut self, lt_pos: usize) -> (ast::JsxElement, usize) {
        if self.bytes.get(lt_pos + 1) == Some(&b'>') {
            return self.recover_fragment(lt_pos);
        }
        let name_start = lt_pos + 1;
        let name_end = scan_jsx_name(self.bytes, name_start);
        if name_end == name_start {
            // No valid name at all (`<3>`, `<<A/>`, …): recover by
            // consuming just the `<` as an error and resuming text
            // scanning right after it. Not one of the required fixtures;
            // TODO(phase0): a byte-precise message for this shape is not
            // specified by grammar §9.
            return (
                incomplete_element(lt_pos, lt_pos + 1, "a tag name"),
                lt_pos + 1,
            );
        }
        let after_name = next_significant(self.bytes, name_end);
        let generic_args_skipped_at = if matches!(self.bytes.get(after_name.start), Some(b'<')) {
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
}
