//! The shared "Rust region" scanner.
//!
//! A function body (`{ ... }`) and a JSX expression island (also `{ ... }`,
//! either an attribute value or a child) are the same kind of thing from
//! the scanner's point of view: a brace-delimited stretch of Rust that may
//! contain JSX elements at any nesting depth (grammar §2.1, §6). Both are
//! scanned by [`Parser::scan_region`]; only what happens when the region
//! runs out at end of input before finding its own matching `}` differs
//! (a block's implicit close is a Rust-level recovery, not an Outou
//! diagnostic; an island's is — see `crate::parser::jsx`), so that
//! decision is left to the caller.

use outou_sourcemap::Span;

use crate::ast;
use crate::lexer::disambiguate;
use crate::lexer::position::{self, ExprPosition};
use crate::lexer::rust_token::{self, RtKind, RtTok};

use super::Parser;

/// How a scanned region ended.
pub enum RegionEnd {
    /// Found this region's own matching `}`. `close_start`/`close_end` are
    /// the byte range of that brace.
    Closed {
        close_start: usize,
        close_end: usize,
    },
    /// Ran out of input before finding a matching `}`.
    Eof { pos: usize },
}

/// The result of scanning one `{ ... }` region.
pub struct RegionScan {
    /// Statements (every node but a final tail expression).
    pub statements: Vec<ast::Expr>,
    /// The region's tail expression, if its last node has no trailing `;`.
    pub tail: Option<Box<ast::Expr>>,
    /// How the region ended.
    pub end: RegionEnd,
}

impl<'s> Parser<'s> {
    /// Parses a `{ ... }` block (a function body). Unlike an island, a
    /// block that runs out at end of input before its own `}` closes
    /// implicitly with no Outou diagnostic — that is a Rust-level
    /// recovery (grammar §2.2's last paragraph), not Outou's to report.
    pub(super) fn parse_block(&mut self, open_brace_pos: usize) -> ast::Block {
        let scan = self.scan_region(open_brace_pos + 1);
        let end = match scan.end {
            RegionEnd::Closed { close_end, .. } => close_end,
            RegionEnd::Eof { pos } => pos,
        };
        ast::Block {
            span: Span::new(open_brace_pos as u32, end as u32),
            statements: scan.statements,
            tail: scan.tail,
        }
    }

    /// Scans a Rust region starting right after its opening `{`
    /// (`content_start`). Never panics; always returns, even for
    /// pathologically broken input.
    pub(super) fn scan_region(&mut self, content_start: usize) -> RegionScan {
        let len = self.bytes.len();
        let mut nodes: Vec<ast::Expr> = Vec::new();
        let mut chunk_start = content_start;
        let mut pos = content_start;
        let mut depth: i32 = 0;
        let mut prev: Option<RtTok> = None;

        loop {
            if pos >= len {
                flush(&mut nodes, self.source, chunk_start, pos);
                return finish(nodes, RegionEnd::Eof { pos });
            }

            if let Some(end) = crate::lexer::opaque::try_skip_opaque_region(
                self.bytes,
                pos,
                is_path_continuation(self.source, prev),
            ) {
                pos = end;
                prev = Some(operand_sentinel(pos));
                continue;
            }

            let tok = rust_token::next_token(self.bytes, pos);
            if tok.start == tok.end {
                flush(&mut nodes, self.source, chunk_start, tok.start);
                return finish(nodes, RegionEnd::Eof { pos: tok.start });
            }

            match tok.kind {
                RtKind::OpenDelim if self.byte_at(tok.start) == b'{' => {
                    depth += 1;
                    prev = Some(tok);
                    pos = tok.end;
                }
                RtKind::CloseDelim if self.byte_at(tok.start) == b'}' => {
                    if depth == 0 {
                        flush(&mut nodes, self.source, chunk_start, tok.start);
                        return finish(
                            nodes,
                            RegionEnd::Closed {
                                close_start: tok.start,
                                close_end: tok.end,
                            },
                        );
                    }
                    depth -= 1;
                    prev = Some(tok);
                    pos = tok.end;
                }
                RtKind::Punct if self.byte_at(tok.start) == b';' && depth == 0 => {
                    flush(&mut nodes, self.source, chunk_start, tok.end);
                    chunk_start = tok.end;
                    prev = Some(tok);
                    pos = tok.end;
                }
                RtKind::Punct if self.byte_at(tok.start) == b'<' => {
                    match self.try_jsx_at(tok, prev) {
                        None => {
                            prev = Some(tok);
                            pos = tok.end;
                        }
                        Some((expr, new_pos)) => {
                            flush(&mut nodes, self.source, chunk_start, tok.start);
                            nodes.push(expr);
                            chunk_start = new_pos;
                            pos = new_pos;
                            prev = Some(operand_sentinel(pos));
                        }
                    }
                }
                _ => {
                    prev = Some(tok);
                    pos = tok.end;
                }
            }
        }
    }

    fn byte_at(&self, pos: usize) -> u8 {
        self.bytes.get(pos).copied().unwrap_or(0)
    }

    /// Classifies a `<` token already known to have the raw byte `<`
    /// (grammar §4) and, if it commits to JSX, parses the element.
    ///
    /// Shared by [`scan_region`](Self::scan_region) and
    /// [`super::item::parse_items`] (decision D1): both need to decide, at
    /// every `<` they see, whether it is the less-than operator, a
    /// qualified-path type, or the start of a JSX expression — the same
    /// three-step check (expression position, then rules 2/3), just
    /// applied at different brace depths.
    ///
    /// Returns `None` when rule 1/2/3 decided Rust: the caller treats
    /// `tok` as an ordinary token and keeps scanning. Returns
    /// `Some((node, new_pos))` when it committed to JSX; the returned node
    /// is already past grammar §7.1's postfix check.
    pub(super) fn try_jsx_at(
        &mut self,
        tok: RtTok,
        prev: Option<RtTok>,
    ) -> Option<(ast::Expr, usize)> {
        let expr_pos = position::from_prev(prev.map(|t| (t, t.text(self.source))));
        if !matches!(expr_pos, ExprPosition::Expr) {
            return None;
        }
        match disambiguate::classify(self.source, tok.start) {
            disambiguate::Classification::Rust => None,
            classification => {
                let (element, new_pos) = self.parse_jsx_element(tok.start, classification);
                self.check_postfix_on_jsx(new_pos);
                Some((ast::Expr::Jsx(element), new_pos))
            }
        }
    }

    /// Grammar §7.1: a JSX expression must not be followed directly by
    /// `.`, `?`, `(` or `[`, even across trivia (whitespace or comments,
    /// M7): `<A/> .into()` is rejected exactly like `<A/>.into()`, so this
    /// looks at the next *significant* token rather than the very next
    /// byte.
    fn check_postfix_on_jsx(&mut self, after: usize) {
        let tok = rust_token::next_significant(self.bytes, after);
        if tok.start == tok.end {
            return;
        }
        if matches!(self.bytes[tok.start], b'.' | b'?' | b'(' | b'[') {
            self.push_diag(
                Span::new(tok.start as u32, tok.start as u32 + 1),
                super::diag::postfix_on_jsx_not_supported(),
            );
        }
    }
}

/// A synthetic "previous token" used after consuming something atomically
/// (an opaque macro/attribute region, a completed JSX element) that isn't
/// itself a single [`RtTok`] but behaves like a completed operand for
/// expression-position purposes ([`position::from_prev`]'s `CloseDelim`
/// arm never inspects the token's text). `pub(super)` so
/// [`super::item::parse_items`] can use the same sentinel (decision D1).
pub(super) fn operand_sentinel(pos: usize) -> RtTok {
    RtTok {
        kind: RtKind::CloseDelim,
        start: pos,
        end: pos,
    }
}

fn flush(nodes: &mut Vec<ast::Expr>, source: &str, start: usize, end: usize) {
    if start >= end {
        return;
    }
    let text = &source[start..end];
    if is_trivia_only(source.as_bytes(), start, end) {
        // Absorb trailing trivia into an immediately preceding *Rust* node
        // only (M9, decision D3 point 1: a `JsxElement.span` — and, for
        // the same reason, an `ErrorNode.span` — is never widened over
        // adjacent trivia). When the preceding node is `Jsx` or `Error`,
        // the trivia becomes its own small `Expr::Rust` node instead, so
        // every span still exactly partitions the region with nothing
        // ever widened past what it actually is.
        if let Some(ast::Expr::Rust(rs)) = nodes.last_mut() {
            rs.span = Span::new(rs.span.start, end as u32);
            rs.text = source[rs.span.start as usize..end].to_string();
            return;
        }
    }
    nodes.push(ast::Expr::Rust(ast::RustSource {
        span: Span::new(start as u32, end as u32),
        text: text.to_string(),
    }));
}

/// Whether `[start, end)` contains nothing but whitespace and/or Rust
/// comments (grammar §6: an island of only these is empty and produces no
/// child and no diagnostic). Unlike a plain `text.trim().is_empty()`
/// check, this correctly recognizes a comment-only stretch
/// (`/* a note */`) as trivial too — a comment is not whitespace, but it
/// carries no expression either (M3).
///
/// `end` bounds the scan: [`rust_token::next_token`] does not know about
/// it, so a token found at or past `end` means only whitespace remained
/// inside the range, which is also trivial.
pub(super) fn is_trivia_only(bytes: &[u8], start: usize, end: usize) -> bool {
    let mut pos = start;
    loop {
        let tok = rust_token::next_token(bytes, pos);
        if tok.start >= end || tok.start == tok.end {
            return true;
        }
        if !matches!(tok.kind, RtKind::LineComment | RtKind::BlockComment) {
            return false;
        }
        pos = tok.end;
    }
}

/// Builds an [`ast::Island`] from a scanned region's nodes (grammar §6,
/// §9; decision D3). Every node the scan found — every statement and the
/// tail, in order — becomes one of `parts`; nothing is collapsed into a
/// single opaque slice, so a JSX element anywhere inside the island (not
/// just as its sole content) survives as its own [`ast::Expr::Jsx`] (H6).
pub(super) fn build_island(
    scan: RegionScan,
    content_start: usize,
    content_end: usize,
) -> ast::Island {
    let mut parts = scan.statements;
    if let Some(tail) = scan.tail {
        parts.push(*tail);
    }
    // `RegionScan::tail` is not necessarily last in *byte order* (see
    // `finish`'s doc comment: trailing trivia after the tail, before the
    // closing `}`, is its own trailing statement). `parts` must partition
    // `span` in source order (grammar §9's round-trip contract, D3), so
    // sort by span start rather than assuming push order is byte order.
    parts.sort_by_key(expr_span_start);
    ast::Island {
        span: Span::new(content_start as u32, content_end as u32),
        parts,
    }
}

/// Whether `prev` is an identifier, a raw identifier, or `::` — i.e.
/// whether the position right after it is in the *middle* of an
/// already-established Rust path rather than at a fresh position where a
/// macro invocation could start (M12, decision for issue #4 fix list item
/// 17). `pub(super)` so [`super::item::parse_items`] can use the same
/// check.
pub(super) fn is_path_continuation(source: &str, prev: Option<RtTok>) -> bool {
    match prev {
        Some(tok) => {
            matches!(tok.kind, RtKind::Ident | RtKind::RawIdent)
                || (tok.kind == RtKind::Punct && tok.text(source) == "::")
        }
        None => false,
    }
}

fn expr_span_start(expr: &ast::Expr) -> u32 {
    match expr {
        ast::Expr::Rust(rs) => rs.span.start,
        ast::Expr::Jsx(el) => el.span.start,
        ast::Expr::Error(e) => e.span.start,
    }
}

/// Whether `node` is a `Rust` node consisting entirely of whitespace —
/// exactly what [`flush`] pushes for trailing trivia it could not absorb
/// into a preceding node (M9, decision D3: never into a `Jsx` or `Error`
/// node's span). Such a node can never be a meaningful Rust tail
/// expression.
fn is_whitespace_only_rust_node(node: &ast::Expr) -> bool {
    matches!(node, ast::Expr::Rust(rs) if rs.text.trim().is_empty())
}

/// Determines the region's tail expression and the remaining statements.
///
/// The tail candidate is the *last node that is not purely trailing
/// trivia* — not simply `nodes.last()` — because trailing whitespace
/// between the real tail (often a `Jsx` element) and the region's closing
/// `}` is its own node (M9: it is never merged into the element's span).
/// That trivia node, if present, is left in place in `statements` even
/// though it comes after the tail in byte order; callers that reconstruct
/// source order (e.g. the splicing round-trip, `tests/fixtures.rs`) sort
/// by span rather than assuming `statements` then `tail` is byte order.
fn finish(nodes: Vec<ast::Expr>, end: RegionEnd) -> RegionScan {
    let mut nodes = nodes;
    let tail_index = nodes.iter().rposition(|n| !is_whitespace_only_rust_node(n));
    let tail = match tail_index.map(|idx| (idx, &nodes[idx])) {
        Some((idx, ast::Expr::Jsx(_))) => Some(Box::new(nodes.remove(idx))),
        Some((idx, ast::Expr::Rust(rs))) if !rs.text.trim_end().ends_with(';') => {
            Some(Box::new(nodes.remove(idx)))
        }
        _ => None,
    };
    RegionScan {
        statements: nodes,
        tail,
        end,
    }
}
