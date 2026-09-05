//! Opening tag and attribute parsing: everything from right after a JSX
//! element's name up to `/>`, `>`, or a terminator (grammar §5, §5.1).

use outou_sourcemap::Span;

use crate::ast;
use crate::lexer::disambiguate::NamedTag;
use crate::lexer::scan_jsx_name;

use super::super::diag;
use super::super::region::RegionEnd;
use super::super::Parser;
use super::{is_jsx_name_byte, pos_span, skip_ws, ChildrenOutcome};

/// How a tag finished (grammar §5: `SelfClosing | Open`).
enum TagOutcome {
    SelfClosed,
    Opened { tag_end: usize },
    Terminated,
}

/// A tag that never reached `>` still has a name; it is reported as a
/// [`ast::JsxTag::Named`] whose span is just the name (there is no full
/// open-tag span to report, since `>` was never seen).
fn named_tag(name: ast::Ident) -> ast::JsxTag {
    ast::JsxTag::Named {
        span: name.span,
        name,
    }
}

impl<'s> Parser<'s> {
    pub(super) fn parse_named_element_at_depth(
        &mut self,
        lt_pos: usize,
        tag: NamedTag,
    ) -> (ast::JsxElement, usize) {
        let name_text = self.source[tag.name_start..tag.name_end].to_string();
        let name_ident = ast::Ident {
            span: Span::new(tag.name_start as u32, tag.name_end as u32),
            name: name_text.clone(),
        };

        if let Some(inner_lt) = tag.generic_args_skipped_at {
            self.push_diag(
                Span::new(inner_lt as u32, inner_lt as u32 + 1),
                diag::generic_arguments_not_supported(),
            );
        }
        if name_text == "Self" {
            self.push_diag(name_ident.span, diag::self_not_a_valid_component_name());
        }

        self.open_names.push((
            name_text.clone(),
            Span::new(lt_pos as u32, lt_pos as u32 + 1),
        ));
        let (attributes, errors, tag_outcome, pos) =
            self.parse_attributes(lt_pos, &name_text, tag.resume_at);

        match tag_outcome {
            TagOutcome::SelfClosed | TagOutcome::Terminated => {
                self.open_names.pop();
                let element = ast::JsxElement {
                    span: Span::new(lt_pos as u32, pos as u32),
                    open: named_tag(name_ident),
                    attributes,
                    children: Vec::new(),
                    close: None,
                    errors,
                };
                (element, pos)
            }
            TagOutcome::Opened { tag_end } => {
                let open_span = Span::new(lt_pos as u32, tag_end as u32);
                let (children, close, end_pos, outcome) =
                    self.parse_jsx_children(&name_text, tag_end);
                self.open_names.pop();
                if outcome == ChildrenOutcome::Terminated {
                    self.push_diag(
                        Span::new(lt_pos as u32, lt_pos as u32 + 1),
                        diag::missing_closing_tag(&name_text),
                    );
                }
                let element = ast::JsxElement {
                    span: Span::new(lt_pos as u32, end_pos as u32),
                    open: ast::JsxTag::Named {
                        span: open_span,
                        name: name_ident,
                    },
                    attributes,
                    children,
                    close,
                    errors,
                };
                (element, end_pos)
            }
        }
    }

    /// Parses attributes starting right after the tag name (or after a
    /// skipped generic-argument list), up to `/>`, `>`, or a terminator.
    fn parse_attributes(
        &mut self,
        lt_pos: usize,
        tag_name: &str,
        start: usize,
    ) -> (
        Vec<ast::JsxAttribute>,
        Vec<ast::ErrorNode>,
        TagOutcome,
        usize,
    ) {
        let mut attributes: Vec<ast::JsxAttribute> = Vec::new();
        let mut errors: Vec<ast::ErrorNode> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let mut pos = start;
        loop {
            pos = skip_ws(self.bytes, pos);
            match self.bytes.get(pos).copied() {
                None => {
                    // Missing at EOF: point at the tag's own opening `<`,
                    // per the span convention (grammar §9).
                    self.push_diag_at(pos_span(lt_pos), diag::eof_inside_tag(tag_name));
                    return (attributes, errors, TagOutcome::Terminated, pos);
                }
                Some(b'}') => {
                    self.push_diag_at(pos_span(pos), diag::rbrace_inside_tag(tag_name));
                    self.terminator_diagnosed_at = Some(pos);
                    return (attributes, errors, TagOutcome::Terminated, pos);
                }
                Some(b'/') if self.bytes.get(pos + 1) == Some(&b'>') => {
                    return (attributes, errors, TagOutcome::SelfClosed, pos + 2);
                }
                Some(b'>') => {
                    return (
                        attributes,
                        errors,
                        TagOutcome::Opened { tag_end: pos + 1 },
                        pos + 1,
                    );
                }
                Some(b'{') => {
                    let end = crate::lexer::rust_token::skip_balanced_group(self.bytes, pos);
                    self.push_diag_at(pos_span(pos), diag::spread_attribute_not_supported());
                    pos = end;
                }
                Some(b'.') => {
                    self.push_diag_at(pos_span(pos), diag::dotted_tag_name_not_supported());
                    pos += 1;
                }
                Some(b':') => {
                    self.push_diag_at(pos_span(pos), diag::namespaced_name_not_supported());
                    pos += 1;
                }
                Some(c) if is_jsx_name_byte(c) => {
                    let name_start = pos;
                    let name_end = scan_jsx_name(self.bytes, pos);
                    let attr_name = self.source[name_start..name_end].to_string();
                    if seen.contains(&attr_name) {
                        self.push_diag_at(
                            pos_span(name_start),
                            diag::duplicate_attribute(&attr_name),
                        );
                    } else {
                        seen.push(attr_name.clone());
                    }
                    pos = name_end;
                    let ws_pos = skip_ws(self.bytes, pos);
                    let (value, after, terminated) = if self.bytes.get(ws_pos) == Some(&b'=') {
                        self.parse_attribute_value(&attr_name, ws_pos + 1)
                    } else {
                        (None, pos, false)
                    };
                    attributes.push(ast::JsxAttribute {
                        span: Span::new(name_start as u32, after as u32),
                        name: ast::Ident {
                            span: Span::new(name_start as u32, name_end as u32),
                            name: attr_name,
                        },
                        value,
                    });
                    pos = after;
                    if terminated {
                        return (attributes, errors, TagOutcome::Terminated, pos);
                    }
                }
                Some(other) => {
                    self.push_diag_at(
                        pos_span(pos),
                        diag::unexpected_inside_tag(&(other as char).to_string(), tag_name),
                    );
                    errors.push(ast::ErrorNode {
                        span: pos_span(pos),
                        expected: None,
                    });
                    pos += 1;
                }
            }
        }
    }

    /// Parses an attribute value. The returned `bool` is `true` when the
    /// value's own island ran out at end of input: grammar §2.2's "one
    /// diagnostic for the innermost open construct" was already reported
    /// here, so the caller (still scanning this tag's attributes) must
    /// stop immediately without also reporting `eof_inside_tag` for
    /// itself.
    fn parse_attribute_value(
        &mut self,
        attr_name: &str,
        start: usize,
    ) -> (Option<ast::JsxAttributeValue>, usize, bool) {
        let pos = skip_ws(self.bytes, start);
        match self.bytes.get(pos).copied() {
            Some(b'\'') => {
                let tok = crate::lexer::rust_token::next_token(self.bytes, pos);
                self.push_diag_at(pos_span(pos), diag::single_quoted_attribute_value());
                (
                    Some(ast::JsxAttributeValue::Error(ast::ErrorNode {
                        span: Span::new(pos as u32, tok.end as u32),
                        expected: None,
                    })),
                    tok.end,
                    false,
                )
            }
            Some(b'{') => self.parse_attribute_value_island(attr_name, pos),
            Some(_) => self.parse_string_or_invalid_attribute_value(pos),
            None => {
                self.push_diag_at(
                    Span::new(pos as u32, pos as u32),
                    diag::invalid_attribute_value(),
                );
                (
                    Some(ast::JsxAttributeValue::Error(ast::ErrorNode {
                        span: Span::new(pos as u32, pos as u32),
                        expected: None,
                    })),
                    pos,
                    false,
                )
            }
        }
    }

    /// Handles every attribute-value shape [`Parser::parse_attribute_value`]
    /// does not special-case itself (`'`, `{`, and end of input): grammar
    /// §5.1 allows exactly Rust's own plain and raw *string*-literal
    /// grammar (`"…"`, `r"…"`, `r#"…"#`, …) — never a byte-string
    /// (`b"…"`, `br"…"`), a C-string (`c"…"`, `cr"…"`), a char literal, or
    /// a numeric literal (M1, MEDIUM-4), none of which
    /// [`crate::lexer::rust_token::RtKind::Literal`] otherwise
    /// distinguishes from a string. So this tokenizes at `pos` and accepts
    /// the value only if the literal found there is a plain or raw string.
    /// Anything else (a missing value right before `/` or `>`, a bare
    /// identifier, a number, a char/byte-string/C-string literal, …) is
    /// diagnosed. A literal or identifier is consumed whole (MEDIUM-5):
    /// leaving it unconsumed made the tag's own attribute loop re-scan it
    /// one byte at a time, one diagnostic per byte. Punctuation (`/`, `>`,
    /// …) is left unconsumed on purpose — it may be the tag's own
    /// terminator, which the caller must still see fresh.
    fn parse_string_or_invalid_attribute_value(
        &mut self,
        pos: usize,
    ) -> (Option<ast::JsxAttributeValue>, usize, bool) {
        let tok = crate::lexer::rust_token::next_token(self.bytes, pos);
        if tok.kind == crate::lexer::rust_token::RtKind::Literal {
            if let Some((content_start, content_end, closed)) =
                string_literal_inner_range(self.bytes, tok.start, tok.end)
            {
                if !closed {
                    self.push_diag_at(
                        pos_span(pos),
                        diag::unterminated_string_in_attribute_value(),
                    );
                    return (
                        Some(ast::JsxAttributeValue::Error(ast::ErrorNode {
                            span: Span::new(pos as u32, tok.end as u32),
                            expected: None,
                        })),
                        tok.end,
                        false,
                    );
                }
                let inner = &self.source[content_start..content_end];
                // A raw string (`r"…"`, `r#"…"#`) has no escapes at all —
                // its content is used as-is. A plain string uses Rust's
                // own escape grammar (grammar §5.1), so its content must
                // be decoded, not spliced as a raw slice (MEDIUM-8, issue
                // #6 fix list item 9).
                let is_raw = self.bytes.get(tok.start) == Some(&b'r');
                let value = if is_raw {
                    inner.to_string()
                } else {
                    crate::escape::decode_plain_string_escapes(inner)
                };
                return (
                    Some(ast::JsxAttributeValue::Text(ast::JsxText {
                        span: Span::new(pos as u32, tok.end as u32),
                        value,
                    })),
                    tok.end,
                    false,
                );
            }
        }
        self.push_diag_at(pos_span(pos), diag::invalid_attribute_value());
        let end = if matches!(
            tok.kind,
            crate::lexer::rust_token::RtKind::Literal
                | crate::lexer::rust_token::RtKind::Ident
                | crate::lexer::rust_token::RtKind::RawIdent
        ) {
            tok.end
        } else {
            pos
        };
        (
            Some(ast::JsxAttributeValue::Error(ast::ErrorNode {
                span: Span::new(pos as u32, end as u32),
                expected: None,
            })),
            end,
            false,
        )
    }

    /// Parses an attribute-value island, `{ … }`: [`Parser::parse_attribute_value`]
    /// delegates here once it has seen the leading `{`.
    fn parse_attribute_value_island(
        &mut self,
        attr_name: &str,
        pos: usize,
    ) -> (Option<ast::JsxAttributeValue>, usize, bool) {
        let scan = self.scan_region(pos + 1);
        match scan.end {
            RegionEnd::Eof { pos: eof_pos } => {
                self.push_diag_at(
                    pos_span(pos),
                    diag::eof_in_attribute_value_island(attr_name),
                );
                (
                    Some(ast::JsxAttributeValue::Error(ast::ErrorNode {
                        span: Span::new(pos as u32, eof_pos as u32),
                        expected: None,
                    })),
                    eof_pos,
                    true,
                )
            }
            RegionEnd::Closed {
                close_start,
                close_end,
            } => {
                if super::super::region::is_trivia_only(self.bytes, pos + 1, close_start) {
                    self.push_diag_at(pos_span(pos), diag::empty_attribute_value_island(attr_name));
                    (
                        Some(ast::JsxAttributeValue::Error(ast::ErrorNode {
                            span: Span::new(pos as u32, close_end as u32),
                            expected: None,
                        })),
                        close_end,
                        false,
                    )
                } else {
                    let island = super::super::region::build_island(scan, pos + 1, close_start);
                    (
                        Some(ast::JsxAttributeValue::Expression(island)),
                        close_end,
                        false,
                    )
                }
            }
        }
    }
}

/// If the literal spanning `[start, end)` is one of Rust's plain or raw
/// *string* literal forms — `"…"`, `r"…"`, `r#"…"#`, … — returns the byte
/// range of its content, with the quotes and any raw-string hashes
/// stripped. Returns `None` for anything else
/// [`crate::lexer::rust_token::RtKind::Literal`] can also produce: a char
/// literal (`'x'`), a byte or C string (`b"…"`, `br"…"`, `c"…"`,
/// `cr"…"`), a byte char (`b'x'`), or a numeric literal. Grammar §5.1
/// allows exactly Rust's plain and raw *string* grammar as an attribute
/// value — a byte-string or C-string is rejected even though both look
/// like an ordinary string apart from their prefix (M1, MEDIUM-4).
fn string_literal_inner_range(
    bytes: &[u8],
    start: usize,
    end: usize,
) -> Option<(usize, usize, bool)> {
    let mut pos = start;
    if bytes.get(pos) == Some(&b'r') && matches!(bytes.get(pos + 1), Some(b'"') | Some(b'#')) {
        pos += 1;
        let hashes_start = pos;
        while bytes.get(pos) == Some(&b'#') {
            pos += 1;
        }
        let hash_count = pos - hashes_start;
        if bytes.get(pos) != Some(&b'"') {
            return None;
        }
        let content_start = pos + 1;
        // Closed iff the literal's final `1 + hash_count` bytes are a `"`
        // followed by exactly `hash_count` `#`s in that position — the
        // same closer `scan_raw_string_body` requires.
        let closed = end.checked_sub(1 + hash_count).is_some_and(|closer_start| {
            closer_start >= content_start
                && bytes.get(closer_start) == Some(&b'"')
                && bytes[closer_start + 1..end].iter().all(|&b| b == b'#')
        });
        let content_end = if closed { end - 1 - hash_count } else { end };
        return Some((content_start, content_end, closed));
    }
    if bytes.get(pos) == Some(&b'"') {
        let content_start = pos + 1;
        let closed = end > content_start && bytes.get(end - 1) == Some(&b'"');
        let content_end = if closed { end - 1 } else { end };
        return Some((content_start, content_end, closed));
    }
    None
}
