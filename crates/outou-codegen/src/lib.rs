//! Code generation from the Outou AST to Rust.
//!
//! There is exactly one compiler. `cargo build` and the language server both
//! call [`generate`] with the same AST and the same [`Backend`]; only the
//! [`Mode`] differs. Generated output is deterministic: the same source,
//! compiler version, configuration and backend always yield byte-identical
//! Rust and an identical source map.
//!
//! This crate carries no backend-specific knowledge. It defines the
//! [`Backend`] trait every backend implements (see `outou-backend-dioxus`),
//! the shared [`Writer`] infrastructure a backend uses to emit text and
//! record source-map mappings as it goes, and the [`Mode`]-driven contract
//! ([`reject_syntax_errors_in_strict_mode`]) every backend follows before
//! it starts lowering.

mod writer;

pub use writer::Writer;

use std::collections::BTreeMap;

use outou_sourcemap::{SourceMap, Uri};
use outou_syntax::{Diagnostic, Parsed, Severity};

/// How to treat broken input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// For `cargo build`. Any syntax error is a compile error.
    Strict,
    /// For the IDE. Error nodes are replaced with placeholders so that
    /// rust-analyzer can still analyze the rest of the file.
    Recovery,
}

/// Options steering one [`Backend::generate`] call.
///
/// A backend lowers exactly one `.rsx` source into exactly one generated
/// Rust file per call; multi-file crates call `generate` once per module
/// (see `outou-modules`' `ModuleGraph::generated_units`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerateOptions {
    /// URI of the file codegen is producing.
    pub generated_uri: Uri,
    /// URI of the `.rsx` source being lowered.
    pub source_uri: Uri,
    /// Module name → path string to emit in `#[path = "…"]` for that
    /// module's declaration (`mod name;`). A module absent from this map
    /// has its declaration emitted verbatim, unchanged — the common case
    /// for an inline module, or when the caller has not resolved the
    /// module graph yet.
    pub module_paths: BTreeMap<String, String>,
}

impl GenerateOptions {
    /// Creates options with an empty `module_paths` map.
    pub fn new(generated_uri: Uri, source_uri: Uri) -> Self {
        Self {
            generated_uri,
            source_uri,
            module_paths: BTreeMap::new(),
        }
    }

    /// Returns options with one more `module_paths` entry.
    pub fn with_module_path(
        mut self,
        module_name: impl Into<String>,
        path: impl Into<String>,
    ) -> Self {
        self.module_paths.insert(module_name.into(), path.into());
        self
    }
}

/// Output of one generation run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generated {
    /// Generated Rust source.
    pub rust: String,
    /// Source map from the generated file back to its `.rsx` input.
    pub source_map: SourceMap,
}

/// Errors from [`generate`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The file contains syntax errors and the mode is [`Mode::Strict`].
    #[error("source contains syntax errors; strict mode refuses to generate")]
    SyntaxErrors {
        /// The error-severity diagnostics that caused the refusal, in
        /// source order.
        diagnostics: Vec<Diagnostic>,
    },
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
    ///
    /// `source` is the exact text `parsed` was parsed from — the backend
    /// needs it to splice Rust verbatim into the generated file.
    ///
    /// Implementations call [`reject_syntax_errors_in_strict_mode`] first,
    /// so [`Mode::Strict`] never has to handle an [`outou_syntax::ast::ErrorNode`].
    fn generate(
        &self,
        parsed: &Parsed,
        source: &str,
        mode: Mode,
        opts: &GenerateOptions,
    ) -> Result<Generated, Error>;
}

/// Generates Rust for `parsed` using `backend`.
pub fn generate(
    backend: &dyn Backend,
    parsed: &Parsed,
    source: &str,
    mode: Mode,
    opts: &GenerateOptions,
) -> Result<Generated, Error> {
    backend.generate(parsed, source, mode, opts)
}

/// The [`Mode::Strict`] gate every backend runs first: if `parsed` carries
/// any error-severity [`Diagnostic`] and `mode` is [`Mode::Strict`], this
/// returns [`Error::SyntaxErrors`] instead of letting the backend see an
/// [`outou_syntax::ast::ErrorNode`]. In [`Mode::Recovery`], or when there are no errors,
/// this is a no-op.
pub fn reject_syntax_errors_in_strict_mode(parsed: &Parsed, mode: Mode) -> Result<(), Error> {
    if mode != Mode::Strict {
        return Ok(());
    }
    let diagnostics: Vec<Diagnostic> = parsed
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .cloned()
        .collect();
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(Error::SyntaxErrors { diagnostics })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use outou_sourcemap::Span;

    #[test]
    fn strict_mode_rejects_error_diagnostics() {
        let parsed = outou_syntax::parse("fn f() { <div cl");
        let err = reject_syntax_errors_in_strict_mode(&parsed, Mode::Strict).unwrap_err();
        match err {
            Error::SyntaxErrors { diagnostics } => assert!(!diagnostics.is_empty()),
            other => panic!("expected SyntaxErrors, got {other:?}"),
        }
    }

    #[test]
    fn recovery_mode_never_rejects() {
        let parsed = outou_syntax::parse("fn f() { <div cl");
        assert!(reject_syntax_errors_in_strict_mode(&parsed, Mode::Recovery).is_ok());
    }

    #[test]
    fn strict_mode_accepts_clean_input() {
        let parsed = outou_syntax::parse("fn f() { <div /> }");
        assert!(reject_syntax_errors_in_strict_mode(&parsed, Mode::Strict).is_ok());
    }

    #[test]
    fn generate_options_builder_accumulates_module_paths() {
        let opts = GenerateOptions::new(Uri::new("file:///g.rs"), Uri::new("file:///a.rsx"))
            .with_module_path("components", "components.rs");
        assert_eq!(
            opts.module_paths.get("components").map(String::as_str),
            Some("components.rs")
        );
    }

    // A minimal backend so `generate`/`Backend` are exercised end to end
    // without depending on `outou-backend-dioxus` (would be a cycle).
    struct Echo;
    impl Backend for Echo {
        fn name(&self) -> &'static str {
            "echo"
        }
        fn generate(
            &self,
            parsed: &Parsed,
            source: &str,
            mode: Mode,
            opts: &GenerateOptions,
        ) -> Result<Generated, Error> {
            reject_syntax_errors_in_strict_mode(parsed, mode)?;
            let mut writer = Writer::new(opts.generated_uri.clone(), opts.source_uri.clone());
            writer.verbatim(
                source,
                Span::new(0, source.len() as u32),
                outou_sourcemap::MappingKind::Other,
                None,
            );
            let (rust, source_map) = writer.finish();
            Ok(Generated { rust, source_map })
        }
    }

    #[test]
    fn generate_dispatches_to_the_backend() {
        let parsed = outou_syntax::parse("fn f() {}");
        let opts = GenerateOptions::new(Uri::new("file:///g.rs"), Uri::new("file:///a.rsx"));
        let generated = generate(&Echo, &parsed, "fn f() {}", Mode::Strict, &opts).unwrap();
        assert_eq!(generated.rust, "fn f() {}");
    }

    #[test]
    fn ast_module_stays_reachable_from_this_crate_for_backends() {
        // Compile-time smoke check that `outou_syntax::ast` is visible
        // through this crate's dependency the way backends need it.
        fn _accepts(_: &outou_syntax::ast::File) {}
    }
}
