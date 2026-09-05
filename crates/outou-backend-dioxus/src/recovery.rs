//! Recovery-mode placeholders.
//!
//! [`Mode::Recovery`](outou_codegen::Mode) output is for rust-analyzer
//! only: it is never fed to `rustc` by `cargo build` (that is
//! [`Mode::Strict`](outou_codegen::Mode), which never contains a
//! placeholder — [`outou_codegen::reject_syntax_errors_in_strict_mode`]
//! refuses generation before a backend runs). A placeholder therefore only
//! ever has to *type-check* so the rest of the file stays analyzable; it
//! must never be executed. Both hidden functions this module calls
//! (`outou::__private::recovery` and `recovery_element`) are documented as
//! such and `unreachable!()` if somehow called at runtime.

use outou_codegen::Writer;
use outou_sourcemap::{MappingKind, Span};

use crate::PRIVATE_PATH;

/// Emits a call standing in for a JSX element the parser could not
/// recover a name for (grammar §9: an incomplete opening tag). Its result
/// type is `Element`, so it fits anywhere a JSX element could — as a
/// statement, a tail expression, or a child. Mapped back to `broken_span`
/// (the whole broken region) so hover/definition on it still lands
/// somewhere sensible instead of nowhere.
pub fn emit_recovery_element(writer: &mut Writer, broken_span: Span) {
    writer.raw(PRIVATE_PATH);
    writer.mapped(
        "::recovery_element()",
        &[broken_span],
        MappingKind::Other,
        None,
    );
}

/// Emits a call standing in for a broken Rust expression (an
/// [`outou_syntax::ast::ErrorNode`] found where [`outou_syntax::ast::Expr`]
/// was expected — a broken statement, a broken tail, or a broken island).
/// Its return type is generic and inferred from context, so it fits
/// wherever the broken expression stood.
pub fn emit_recovery_expr(writer: &mut Writer, broken_span: Span) {
    writer.raw(PRIVATE_PATH);
    writer.mapped("::recovery()", &[broken_span], MappingKind::Other, None);
}
