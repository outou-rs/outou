//! Resolving a `</Name>` closing tag against [`Parser::open_names`], per
//! grammar §2.1's four closing-tag cases (exact match, deeper match, no
//! match, and — implicitly, by the stack simply being non-empty —
//! "something is open").

use outou_sourcemap::Span;

use crate::ast;
use crate::lexer::scan_jsx_name;

use super::super::diag;
use super::super::Parser;
use super::{is_jsx_name_byte, pos_span, skip_ws, CloseResolution};

/// How far a `</...` scan got. Every shape but `Named` is a truncated
/// closing tag; those carry the byte range of the tag name scanned before
/// the interruption (`None` when none was reached, as in `</` + EOF).
pub(super) enum ClosingTagShape {
    Eof {
        name: Option<(usize, usize)>,
    },
    InterruptedByLt {
        lt: usize,
        name: Option<(usize, usize)>,
    },
    InterruptedByRbrace {
        name: Option<(usize, usize)>,
    },
    Named {
        name_start: usize,
        name_end: usize,
    },
}

impl ClosingTagShape {
    /// The tag name text: the full name for `Named`, the partially-scanned
    /// one for a truncated shape, `None` when no name was scanned at all.
    pub(super) fn name_text(&self, source: &str) -> Option<String> {
        let (start, end) = match self {
            ClosingTagShape::Named {
                name_start,
                name_end,
            } => (*name_start, *name_end),
            ClosingTagShape::Eof { name }
            | ClosingTagShape::InterruptedByLt { name, .. }
            | ClosingTagShape::InterruptedByRbrace { name } => (*name)?,
        };
        Some(source[start..end].to_string())
    }
}

fn incomplete_tag(
    open_start: usize,
    at: usize,
    name: Option<ast::Ident>,
    description: &str,
) -> ast::JsxTag {
    ast::JsxTag::Incomplete(ast::IncompleteTag {
        span: Span::new(open_start as u32, at as u32),
        name,
        missing: ast::MissingToken {
            at: at as u32,
            description: description.to_string(),
        },
    })
}

impl<'s> Parser<'s> {
    /// Resolves a `</Name>` seen at `at` against [`Parser::open_names`],
    /// per grammar §2.1's four closing-tag rows.
    pub(super) fn resolve_closing_tag(
        &mut self,
        at: usize,
        _current_name: &str,
    ) -> CloseResolution {
        let (shape, shape_end) = self.scan_closing_tag_shape(at);
        self.diagnose_truncated_closing_tag(at, &shape, shape_end);
        match shape {
            ClosingTagShape::Eof { .. } => CloseResolution::Terminated { end: shape_end },
            ClosingTagShape::InterruptedByLt { lt, .. } => {
                let close = incomplete_tag(at, lt, None, "`>`");
                CloseResolution::Resolved {
                    close: Some(close),
                    end: lt,
                }
            }
            ClosingTagShape::InterruptedByRbrace { .. } => {
                // Mirrors the EOF case: `end` is `shape_end` (the `}`
                // itself), not `at` (the closing tag's own `<`), so this
                // element's span covers the truncated closing tag instead
                // of leaving `</span` dangling as trailing Rust source. The
                // `}` is left unconsumed at `shape_end`, so the enclosing
                // frame's own children loop re-scans it — not "fresh", since
                // `terminator_diagnosed_at` already marks it as diagnosed,
                // but exactly the same position `tag`'s own
                // `TagOutcome::Terminated` leaves a `}` at, which is what
                // lets the enclosing frame add only its own "missing closing
                // tag" diagnostic without re-reporting the `}` itself.
                CloseResolution::Terminated { end: shape_end }
            }
            ClosingTagShape::Named {
                name_start,
                name_end,
            } => {
                let found = self.source[name_start..name_end].to_string();
                let found_ident = ast::Ident {
                    span: Span::new(name_start as u32, name_end as u32),
                    name: found.clone(),
                };
                let top = self
                    .open_names
                    .last()
                    .map(|(n, _)| n.clone())
                    .unwrap_or_default();
                let match_index = self.open_names.iter().rposition(|(n, _)| *n == found);
                match match_index {
                    Some(idx) if idx + 1 == self.open_names.len() => {
                        let close = ast::JsxTag::Named {
                            span: Span::new(at as u32, shape_end as u32),
                            name: found_ident,
                        };
                        CloseResolution::Resolved {
                            close: Some(close),
                            end: shape_end,
                        }
                    }
                    Some(_) => {
                        // Grammar §9's span convention: "missing" is
                        // reported at the opening of the unterminated
                        // construct, not at the closing tag that revealed
                        // it (M4). The saved span from `open_names` is
                        // that opening `<`.
                        let top_span = self
                            .open_names
                            .last()
                            .map(|(_, span)| *span)
                            .unwrap_or_else(|| pos_span(at));
                        self.push_diag_at(top_span, diag::missing_closing_tag(&top));
                        let close =
                            incomplete_tag(at, at, None, &format!("closing tag `</{top}>`"));
                        CloseResolution::Resolved {
                            close: Some(close),
                            end: at,
                        }
                    }
                    None => {
                        self.push_diag_at(pos_span(at), diag::mismatched_closing_tag(&found, &top));
                        let close = incomplete_tag(
                            at,
                            shape_end,
                            Some(found_ident),
                            &format!("closing tag `</{top}>`"),
                        );
                        CloseResolution::Resolved {
                            close: Some(close),
                            end: shape_end,
                        }
                    }
                }
            }
        }
    }

    /// Reports the diagnostic for a truncated `</...` (end of file, a `<`
    /// or a `}` before the closing `>`); a no-op for `Named`. Shared by
    /// [`Self::resolve_closing_tag`] and the top-level recoveries
    /// (`recover_stray_close`, `recover_fragment`'s trailing close), so
    /// every place a closing tag can be cut short words it identically.
    pub(super) fn diagnose_truncated_closing_tag(
        &mut self,
        at: usize,
        shape: &ClosingTagShape,
        shape_end: usize,
    ) {
        match shape {
            ClosingTagShape::Eof { .. } => {
                self.push_diag_at(pos_span(at), diag::eof_inside_closing_tag());
            }
            ClosingTagShape::InterruptedByLt { lt, .. } => {
                self.push_diag_at(pos_span(*lt), diag::lt_inside_closing_tag());
            }
            ClosingTagShape::InterruptedByRbrace { .. } => {
                self.push_diag_at(pos_span(shape_end), diag::rbrace_inside_closing_tag());
                self.terminator_diagnosed_at = Some(shape_end);
            }
            ClosingTagShape::Named { .. } => {}
        }
    }

    /// Scans the shape of a `</...` construct starting at `at` (the `<`),
    /// without consulting the open-name stack. Shared by real closing
    /// tags and by the top-level "stray close" recovery.
    pub(super) fn scan_closing_tag_shape(&self, at: usize) -> (ClosingTagShape, usize) {
        let mut pos = at + 2; // past `</`
        pos = skip_ws(self.bytes, pos);
        let mut name: Option<(usize, usize)> = None;
        loop {
            match self.bytes.get(pos).copied() {
                None => return (ClosingTagShape::Eof { name }, pos),
                Some(b'<') => return (ClosingTagShape::InterruptedByLt { lt: pos, name }, pos),
                Some(b'}') => return (ClosingTagShape::InterruptedByRbrace { name }, pos),
                Some(b'>') => {
                    // No name was ever found; treat as an empty name.
                    return (
                        ClosingTagShape::Named {
                            name_start: pos,
                            name_end: pos,
                        },
                        pos + 1,
                    );
                }
                Some(c) if is_jsx_name_byte(c) => {
                    let name_start = pos;
                    let name_end = scan_jsx_name(self.bytes, pos);
                    // Keep the first name: `</div class` reports `div`.
                    name.get_or_insert((name_start, name_end));
                    pos = skip_ws(self.bytes, name_end);
                    match self.bytes.get(pos).copied() {
                        Some(b'>') => {
                            return (
                                ClosingTagShape::Named {
                                    name_start,
                                    name_end,
                                },
                                pos + 1,
                            )
                        }
                        Some(b'<') => {
                            return (ClosingTagShape::InterruptedByLt { lt: pos, name }, pos)
                        }
                        Some(b'}') => return (ClosingTagShape::InterruptedByRbrace { name }, pos),
                        None => return (ClosingTagShape::Eof { name }, pos),
                        Some(_) => {
                            pos += 1; // best-effort: skip stray bytes and keep looking for `>`
                        }
                    }
                }
                Some(_) => pos += 1,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// A truncated `</Name` at true end of input (grammar §9's "unexpected
    /// end of file inside closing tag" row) must still resolve to an
    /// element span that reaches true EOF: `CloseResolution::Terminated`'s
    /// `end` must carry the scan's own end-of-input position, not the
    /// closing tag's `<` (mirrors `tag::TagOutcome::Terminated`). Otherwise
    /// the truncated `</div` is left as trailing Rust source and
    /// Recovery-mode codegen splices it after the generated call,
    /// producing invalid Rust (see `docs/phase0-results.md` §2).
    #[test]
    fn element_span_reaches_eof_for_truncated_closing_tag() {
        let source = "fn f() {\n    <div>\n</div";
        let parsed = crate::parser::parse(source);

        let crate::ast::Item::Function(function) = &parsed.file.items[0] else {
            panic!("expected a function item, got {:?}", parsed.file.items[0]);
        };
        let crate::ast::Expr::Jsx(element) = function
            .body
            .tail
            .as_deref()
            .expect("body has a tail expression")
        else {
            panic!("expected the tail expression to be a JSX element");
        };
        assert_eq!(
            element.span.end as usize,
            source.len(),
            "element span does not reach true EOF for a truncated closing tag"
        );
    }

    /// A closing tag interrupted by a `}` that also happens to be the
    /// enclosing function's own closing brace (the shape
    /// `tests/fixtures/incomplete/closing-tag-interrupted-by-rbrace.rsx`
    /// exercises) must report the "unexpected `}` inside closing tag"
    /// diagnostic exactly once — not once per enclosing element — and every
    /// element's span must stop at the `}`, not leak `</span}` past it.
    /// Regression test for the bug where `CloseResolution::Terminated`
    /// resumed from the closing tag's own `<` instead of the `}`, which
    /// left the `}` to be "discovered" again by every enclosing frame.
    #[test]
    fn rbrace_inside_closing_tag_diagnosed_once_and_spans_end_at_rbrace() {
        let source = "fn f() -> Element {\n    <div><span></span}";
        let rbrace_pos = source.rfind('}').expect("source contains a `}`");
        let parsed = crate::parser::parse(source);

        let rbrace_diagnostics: Vec<_> = parsed
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("unexpected `}` inside closing tag"))
            .collect();
        assert_eq!(
            rbrace_diagnostics.len(),
            1,
            "expected exactly one `}}`-inside-closing-tag diagnostic, got {:?}",
            parsed.diagnostics
        );
        assert_eq!(rbrace_diagnostics[0].span.start as usize, rbrace_pos);

        let crate::ast::Item::Function(function) = &parsed.file.items[0] else {
            panic!("expected a function item, got {:?}", parsed.file.items[0]);
        };
        let crate::ast::Expr::Jsx(div) = function
            .body
            .tail
            .as_deref()
            .expect("body has a tail expression")
        else {
            panic!("expected the tail expression to be a JSX element");
        };
        assert_eq!(
            div.span.end as usize, rbrace_pos,
            "div's span must stop at the `}}`, not leak past it"
        );
        let crate::ast::JsxChild::Element(span) = &div.children[0] else {
            panic!("expected div's first child to be the nested <span> element");
        };
        assert_eq!(
            span.span.end as usize, rbrace_pos,
            "span's span must stop at the `}}`, not leak `</span}}` past it"
        );
    }
}
