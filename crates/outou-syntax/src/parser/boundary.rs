//! Where an item ends when the input is cut short: the column-0 item
//! head that bounds a broken function's signature or body (issue #18,
//! grammar §9, "Cutting an unclosed function body").

use outou_sourcemap::Span;

use crate::ast;
use crate::lexer::rust_token::{self, RtKind, RtTok};

use super::diag;
use super::Parser;

/// The kind of code an item head is looked for in. A bare `const` or
/// `impl` at column 0 is a valid part of a signature (`fn f<\nconst N:
/// usize>()`, an `-> impl Trait` return type on its own line) but cannot
/// occur in a body's statements at column 0 of well-formed code, so only
/// a body treats them as item heads.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum HeadContext {
    Signature,
    Body,
}

impl<'s> Parser<'s> {
    /// [`Self::parse_function`], but a function whose body runs to the end
    /// of input without a closing `}` is re-parsed as if the input ended
    /// at the next item boundary ([`Self::next_item_boundary`]), so the
    /// items after it survive as their own nodes instead of being
    /// swallowed by the broken function's island or element (the swallowed
    /// tail, issue #18; grammar §9, "Cutting an unclosed function body").
    ///
    /// The cut is taken only when the next column-0 item head lies after
    /// the earliest diagnostic the first parse reported for this function
    /// (so item-looking text in a string, comment or JSX text *before* the
    /// real error is never a boundary), and at most
    /// [`diag::MAX_RECOVERY_CUTS`] times per file (every cut costs one
    /// extra parse of the swallowed range). A cut is always diagnosed, at
    /// the body's `{`, whether or not the first parse reported anything:
    /// the re-parse discards the first parse's diagnostics, so the cut
    /// diagnostic is what keeps a function with no other error from being
    /// silently split.
    pub(super) fn parse_function_bounded(
        &mut self,
        attrs: Vec<ast::RustSource>,
        item_start: usize,
        head: rust_token::RtTok,
    ) -> (ast::Function, usize) {
        let len = self.bytes.len();
        let snapshot = self.recovery_snapshot();
        let diagnostics_len = snapshot.diagnostics_len;
        let (function, end) = self.parse_function(attrs, item_start, head);
        let is_unclosed = end == len
            && function.body.close.is_none()
            && (function.body.span.start as usize) < end;
        if !is_unclosed || self.recovery_cuts >= diag::MAX_RECOVERY_CUTS {
            return (function, end);
        }
        let brace = function.body.span.start as usize;
        let earliest_diagnostic = self.diagnostics[diagnostics_len..]
            .iter()
            .map(|d| d.span.start as usize)
            .min();
        let Some(cut) = self.next_item_boundary(brace, earliest_diagnostic.unwrap_or(brace)) else {
            return (function, end);
        };

        self.recovery_cuts += 1;
        self.restore_recovery_snapshot(snapshot);
        let (full_source, full_bytes) = (self.source, self.bytes);
        self.source = &full_source[..cut];
        self.bytes = &full_bytes[..cut];
        let reparsed = self.parse_function(function.attributes, item_start, head);
        self.source = full_source;
        self.bytes = full_bytes;
        // The re-parse saw the cut as end of input; its "end of file"
        // wording is wrong where the file goes on.
        for diagnostic in &mut self.diagnostics[diagnostics_len..] {
            diagnostic.message = diag::reword_end_of_file_as_next_item(&diagnostic.message);
        }
        self.push_diag(
            Span::new(brace as u32, brace as u32 + 1),
            diag::unclosed_function_body(&reparsed.0.name.name),
        );
        reparsed
    }

    /// The start of the first token after `after` that begins, in column
    /// 0, an item head ([`Self::item_head_at`], in a body).
    ///
    /// Walks token by token from the body's `{` at `from`, so a string, raw
    /// string, char literal or comment is one atomic token and an
    /// item-looking line inside one is never seen, and a failed head check
    /// resumes after the attribute run it scanned (linear overall).
    /// Column 0 keeps nested or indented code out of it.
    pub(super) fn next_item_boundary(&self, from: usize, after: usize) -> Option<usize> {
        let mut pos = from;
        loop {
            let tok = rust_token::next_token(self.bytes, pos);
            if tok.start == tok.end {
                return None;
            }
            if tok.start > after && self.starts_line(tok.start) {
                let (is_head, resume) = self.item_head_at(tok, HeadContext::Body);
                if is_head {
                    return Some(tok.start);
                }
                pos = resume.max(tok.end);
            } else {
                pos = tok.end;
            }
        }
    }

    /// Whether `pos` is the first byte of its line.
    pub(super) fn starts_line(&self, pos: usize) -> bool {
        pos == 0 || self.bytes[pos - 1] == b'\n'
    }

    /// Whether `tok` begins an item head: leading attributes and doc
    /// comments, a visibility/qualifier prefix, then `fn <name>`, `struct`,
    /// `enum`, `trait`, `mod`, `use`, `static` or `type` (and, in a
    /// [`HeadContext::Body`], `impl` and `const NAME:`). Also returns where
    /// the attribute run it scanned ended, so a caller that moves on after
    /// a failed check does not rescan it.
    pub(super) fn item_head_at(&self, tok: RtTok, context: HeadContext) -> (bool, usize) {
        let is_candidate = match tok.kind {
            RtKind::Ident => true,
            RtKind::Punct => self.bytes[tok.start] == b'#',
            RtKind::LineComment => {
                let text = tok.text(self.source);
                text.starts_with("///") || text.starts_with("//!")
            }
            _ => false,
        };
        if !is_candidate {
            return (false, tok.end);
        }
        let (_, after_attrs) = self.scan_leading_attributes(tok.start);
        let qualified_at = self.skip_item_qualifiers(after_attrs);
        let head = rust_token::next_significant(self.bytes, qualified_at);
        if head.kind != RtKind::Ident {
            return (false, after_attrs);
        }
        let is_head = match head.text(self.source) {
            "fn" => matches!(
                rust_token::next_significant(self.bytes, head.end).kind,
                RtKind::Ident | RtKind::RawIdent
            ),
            // `use<'a>` is a precise-capturing bound in a return type
            // (`-> impl Sized +\nuse<'a> {`), not an import.
            "use" => {
                let next = rust_token::next_significant(self.bytes, head.end);
                !(next.kind == RtKind::Punct && self.bytes[next.start] == b'<')
            }
            "struct" | "enum" | "trait" | "mod" | "static" | "type" => true,
            "impl" => context == HeadContext::Body,
            _ => context == HeadContext::Body && self.is_const_item(after_attrs),
        };
        (is_head, after_attrs)
    }

    /// Whether the item starting at `pos` (past its attributes) is
    /// `[pub] const NAME:` or `const _:`. `skip_item_qualifiers` swallows a
    /// `const` as a qualifier, so this is checked separately.
    pub(super) fn is_const_item(&self, pos: usize) -> bool {
        let keyword = rust_token::next_significant(self.bytes, self.skip_visibility(pos));
        if keyword.kind != RtKind::Ident || keyword.text(self.source) != "const" {
            return false;
        }
        let name = rust_token::next_significant(self.bytes, keyword.end);
        let colon = rust_token::next_significant(self.bytes, name.end);
        colon.kind == RtKind::Punct && self.bytes[colon.start] == b':'
    }
}
