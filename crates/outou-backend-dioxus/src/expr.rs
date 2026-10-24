//! Lowering a Rust-or-JSX [`Expr`] and a span-partitioning sequence of them
//! (`RustItem::parts`, `Island::parts` — grammar §9's round-trip contract).

use outou_codegen::Writer;
use outou_sourcemap::{MappingKind, Span};
use outou_syntax::ast::{Expr, JsxTag};
use outou_syntax::lexer::position::{self, ExprPosition};
use outou_syntax::lexer::rust_token;

use crate::element::lower_element_as_rsx;
use crate::recovery::emit_recovery_expr;
use crate::Cx;

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
pub fn lower_expr(writer: &mut Writer, source: &str, expr: &Expr, cx: Cx<'_>) {
    match expr {
        Expr::Rust(rust) => writer.verbatim(source, rust.span, MappingKind::Expression, None),
        Expr::Jsx(element) => lower_element_as_rsx(writer, source, element, cx),
        Expr::Error(error) => emit_recovery_expr(writer, error.span),
    }
}

/// Whether `part` is a closing tag (`</ul>`) that Rust code swallowed right
/// after an operand: an island that lost its `}` runs on into the enclosing
/// element's closing tag. A recovery element there would sit next to the
/// operand (`map(..) recovery_element()`), which is not Rust, so it is left
/// out; the parser has already diagnosed the tag.
fn is_swallowed_closing_tag(source: &str, previous: Option<&Expr>, part: &Expr) -> bool {
    let Expr::Jsx(element) = part else {
        return false;
    };
    if !matches!(element.open, JsxTag::Incomplete(_))
        || !source[element.span.start as usize..].starts_with("</")
    {
        return false;
    }
    match previous {
        Some(Expr::Rust(rust)) => ends_with_operand(&rust.text),
        Some(Expr::Jsx(_)) | Some(Expr::Error(_)) => true,
        None => false,
    }
}

/// Whether the last significant Rust token of `text` completes an operand, so
/// that a following expression cannot start there.
fn ends_with_operand(text: &str) -> bool {
    let bytes = text.as_bytes();
    let (mut pos, mut prev) = (0, None);
    loop {
        let token = rust_token::next_significant(bytes, pos);
        if token.start == token.end {
            break;
        }
        pos = token.end;
        prev = Some(token);
    }
    matches!(
        position::from_prev(prev.map(|t| (t, t.text(text)))),
        ExprPosition::Operand
    )
}

/// Lowers `parts`, a sequence of [`Expr`] that exactly partitions `region`
/// (no gaps, no overlaps — `RustItem::parts`/`Island::parts`'s own
/// invariant), filling any gap between/around them with verbatim Rust.
/// Gaps are expected to be empty given that invariant; filling them
/// anyway costs nothing and tolerates a caller that passes a slightly
/// wider `region` on purpose.
pub fn lower_parts(writer: &mut Writer, source: &str, parts: &[Expr], region: Span, cx: Cx<'_>) {
    let mut cursor = region.start;
    let mut previous: Option<&Expr> = None;
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
        if !is_swallowed_closing_tag(source, previous, part) {
            lower_expr(writer, source, part, cx);
        }
        previous = Some(part);
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
