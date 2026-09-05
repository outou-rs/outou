//! Front end for Outou sources: a mode-aware lexer, a recovering parser and
//! the AST shared by every backend.
//!
//! The grammar is Rust plus JSX expressions. See `docs/grammar.md` in the
//! repository for the normative description. The parser always recovers:
//! an incomplete tag such as `<div cl` still yields a [`ast::File`] with an
//! [`ast::ErrorNode`] where the broken syntax was, so IDE features keep
//! working while the user types.

pub mod ast;
pub mod lexer;

pub use outou_sourcemap::Span;

/// Severity of a [`Diagnostic`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The file cannot be compiled in strict mode.
    Error,
    /// The file compiles, but something is suspicious.
    Warning,
}

/// A syntax diagnostic produced by Outou itself. These are always phrased in
/// Outou vocabulary (`closing tag </span> does not match <div>`), never in
/// terms of the backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Where the problem is, in the `.rsx` file.
    pub span: Span,
    /// Human-readable message.
    pub message: String,
    /// Error or warning.
    pub severity: Severity,
}

/// Result of parsing one file: an AST (possibly containing error nodes) and
/// the diagnostics found on the way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// The parsed file. Never absent, even for badly broken input.
    pub file: ast::File,
    /// Syntax diagnostics, in source order.
    pub diagnostics: Vec<Diagnostic>,
}

/// Parses one `.rsx` source text.
///
/// Parsing never fails: recoverable errors become [`ast::ErrorNode`]s and
/// [`Diagnostic`]s rather than an `Err`.
pub fn parse(source: &str) -> Parsed {
    let _ = source;
    todo!("outou-syntax: parser is implemented in Phase 0, Week 3 (Gate 1)")
}
