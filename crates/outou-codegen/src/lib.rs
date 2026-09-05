//! Code generation from the Outou AST to Rust.
//!
//! There is exactly one compiler. `cargo build` and the language server both
//! call [`generate`] with the same AST and the same [`Backend`]; only the
//! [`Mode`] differs. Generated output is deterministic: the same source,
//! compiler version, configuration and backend always yield byte-identical
//! Rust and an identical source map.

use outou_sourcemap::SourceMap;
use outou_syntax::ast;

/// How to treat broken input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// For `cargo build`. Any syntax error is a compile error.
    Strict,
    /// For the IDE. Error nodes are replaced with placeholders so that
    /// rust-analyzer can still analyze the rest of the file.
    Recovery,
}

/// Output of one generation run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generated {
    /// Generated Rust source.
    pub rust: String,
    /// Source map from the generated file back to its `.rsx` inputs.
    pub source_map: SourceMap,
}

/// Errors from [`generate`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The file contains syntax errors and the mode is [`Mode::Strict`].
    #[error("source contains syntax errors; strict mode refuses to generate")]
    SyntaxErrors,
    /// The backend does not support a construct.
    #[error("{backend}: {message}")]
    Unsupported {
        /// Backend name.
        backend: &'static str,
        /// Explanation in Outou vocabulary.
        message: String,
    },
}

/// A lowering from the Outou AST to Rust source for a specific runtime.
///
/// Backends receive the Outou AST, never a backend-specific AST. Generated
/// code must reference the runtime only through `::outou::__private::*`, so
/// that user crates never depend on the runtime directly.
pub trait Backend {
    /// Short name used in diagnostics and determinism reports.
    fn name(&self) -> &'static str;

    /// Lowers a parsed file to Rust plus a source map.
    fn generate(&self, file: &ast::File, mode: Mode) -> Result<Generated, Error>;
}

/// Generates Rust for `file` using `backend`.
pub fn generate(backend: &dyn Backend, file: &ast::File, mode: Mode) -> Result<Generated, Error> {
    backend.generate(file, mode)
}
