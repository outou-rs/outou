//! Errors [`crate::format_source`] and [`crate::is_formatted`] can return.
//!
//! Every variant renders a user-friendly, English message with no backend
//! vocabulary (`AGENTS.md`), suitable for printing directly by `outou fmt`
//! or logging by `outou-lsp`.

/// Something that kept a `.rsx` file from being formatted.
#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    /// The file has syntax errors. Formatting a broken file risks losing
    /// or corrupting content, so the pipeline refuses instead.
    #[error("cannot format a file with syntax errors:\n{diagnostics}")]
    SyntaxErrors {
        /// The rendered Outou diagnostics (see [`outou_syntax::render`]).
        diagnostics: String,
    },
    /// `rustfmt` could not be started at all (not on `PATH`, not
    /// executable, …).
    #[error(
        "could not run `rustfmt`: {source}\n\
         outou fmt requires `rustfmt` to be installed and on PATH (it never \
         reimplements Rust formatting itself)"
    )]
    RustfmtUnavailable {
        /// Underlying I/O error from spawning the process.
        #[source]
        source: std::io::Error,
    },
    /// `rustfmt` ran but reported a failure (a non-zero exit status).
    #[error("rustfmt failed:\n{stderr}")]
    RustfmtFailed {
        /// `rustfmt`'s stderr output.
        stderr: String,
    },
    /// `rustfmt` produced output indented with tabs. This crate's own
    /// line-based indentation math (adding or removing a fixed number of
    /// *spaces* per level) is not meaningful against a tab-indented line,
    /// so formatting is refused outright rather than risking corrupting
    /// output — this should not normally happen (`rustfmt_proc` always
    /// passes `--config hard_tabs=false`, which overrides even a
    /// discovered `rustfmt.toml` setting `hard_tabs = true`), and is
    /// reported as a distinct, actionable error if it ever does.
    #[error(
        "rustfmt produced tab-indented output, which outou-fmt cannot lay out correctly; \
         check for `hard_tabs = true` in a discovered rustfmt.toml"
    )]
    TabIndentedOutput,
    /// An internal invariant the formatter relies on did not hold (for
    /// example, a placeholder did not round-trip through `rustfmt`
    /// unchanged). This should never happen; it is reported rather than
    /// panicking or silently producing wrong output (the parser-never-
    /// panics rule in `AGENTS.md` extends to every tool built on top of
    /// it).
    #[error("outou-fmt internal error: {0}")]
    Internal(String),
}
