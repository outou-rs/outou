//! Lowering a `{ … }` [`Block`] (a function body).
//!
//! Unlike `RustItem::parts`/`Island::parts`, `Block::statements` is *not*
//! guaranteed source-ordered (its own doc comment), and the tail
//! expression is tracked separately, so this module sorts the two
//! together before reusing [`crate::expr::lower_parts`]'s gap-filling
//! shape.

use outou_codegen::{Mode, Writer};
use outou_sourcemap::{MappingKind, Span};
use outou_syntax::ast::{Block, Expr};

use crate::expr::{expr_span, lower_expr};

/// Lowers `block`'s content (statements, in source order, then the tail)
/// and, if the block never found its own closing `}` (grammar §2.2's
/// Rust-level recovery — always a [`Mode::Recovery`] situation, since
/// [`outou_codegen::reject_syntax_errors_in_strict_mode`] already refused
/// [`Mode::Strict`] generation for a file with such a diagnostic),
/// synthesizes the missing `}` so the generated Rust stays balanced.
///
/// Callers must not call this for a block that never opened at all
/// (`block.span.start == block.span.end`, no real `{` — see
/// [`outou_syntax::ast::Block::close`]'s doc); there is nothing to lower.
pub fn lower_block(writer: &mut Writer, source: &str, block: &Block, mode: Mode) {
    let mut parts: Vec<&Expr> = block.statements.iter().collect();
    if let Some(tail) = &block.tail {
        parts.push(tail);
    }
    parts.sort_by_key(|expr| expr_span(expr).start);

    let mut cursor = block.span.start;
    for part in parts {
        let span = expr_span(part);
        if cursor < span.start {
            writer.verbatim(
                source,
                Span::new(cursor, span.start),
                MappingKind::Expression,
                None,
            );
        }
        lower_expr(writer, source, part, mode);
        cursor = span.end.max(cursor);
    }
    if cursor < block.span.end {
        writer.verbatim(
            source,
            Span::new(cursor, block.span.end),
            MappingKind::Expression,
            None,
        );
    }
    if block.close.is_none() {
        writer.raw("}");
    }
}
