//! Lowering a Rust-or-JSX [`Expr`] and a span-partitioning sequence of them
//! (`RustItem::parts`, `Island::parts` — grammar §9's round-trip contract).

use outou_codegen::{Mode, Writer};
use outou_sourcemap::{MappingKind, Span};
use outou_syntax::ast::Expr;

use crate::element::lower_element_as_rsx;
use crate::recovery::emit_recovery_expr;

/// The span an [`Expr`] occupies, for sorting and gap-filling.
pub fn expr_span(expr: &Expr) -> Span {
    match expr {
        Expr::Rust(rust) => rust.span,
        Expr::Jsx(element) => element.span,
        Expr::Error(error) => error.span,
    }
}

/// Lowers one [`Expr`]: verbatim Rust, a JSX element wrapped in
/// `::outou::__private::rsx! { … }`, or a recovery placeholder for an
/// error node (Strict mode never reaches the error case — the caller
/// already refused generation if any error diagnostic exists).
pub fn lower_expr(writer: &mut Writer, source: &str, expr: &Expr, mode: Mode) {
    match expr {
        Expr::Rust(rust) => writer.verbatim(source, rust.span, MappingKind::Expression, None),
        Expr::Jsx(element) => lower_element_as_rsx(writer, source, element, mode),
        Expr::Error(error) => emit_recovery_expr(writer, error.span),
    }
}

/// Lowers `parts`, a sequence of [`Expr`] that exactly partitions `region`
/// (no gaps, no overlaps — `RustItem::parts`/`Island::parts`'s own
/// invariant), filling any gap between/around them with verbatim Rust.
/// Gaps are expected to be empty given that invariant; filling them
/// anyway costs nothing and tolerates a caller that passes a slightly
/// wider `region` on purpose.
pub fn lower_parts(writer: &mut Writer, source: &str, parts: &[Expr], region: Span, mode: Mode) {
    let mut cursor = region.start;
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
    if cursor < region.end {
        writer.verbatim(
            source,
            Span::new(cursor, region.end),
            MappingKind::Expression,
            None,
        );
    }
}
