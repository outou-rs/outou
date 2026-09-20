//! Formats `.rsx` source text.
//!
//! The pipeline (`docs/adr/0011-formatter-placeholder-rustfmt-splice.md`,
//! `docs/phase0/issues/13-formatter.md`):
//!
//! 1. Parse the file with [`outou_syntax::parse`]. A file with syntax
//!    errors is never formatted (see [`FormatError::SyntaxErrors`]).
//! 2. Replace every top-level JSX region with a placeholder identifier
//!    that cannot collide with anything in the source
//!    (`placeholder`), producing a text that is now plain, complete
//!    Rust (`docs/grammar.md` §1 guarantees this).
//! 3. Run `rustfmt` on that text as an external process
//!    (`rustfmt_proc`) — never a custom Rust formatter.
//! 4. Recursively format each placeholder's JSX element
//!    (`jsx_print`), including any further Rust content nested inside
//!    it (an expression island), which goes through the same
//!    placeholder -> `rustfmt` -> splice pipeline one level down
//!    (`snippet`) before its own nested JSX is formatted.
//! 5. Splice each formatted JSX region back in at the placeholder's
//!    position, indented to match the line `rustfmt` put it on.
//!
//! This is one pipeline used by both `outou fmt` (`outou-cli`) and
//! `textDocument/formatting` (`outou-lsp`) — never two.
//!
//! **Idempotence and determinism.** Formatting the same source always
//! produces the same output, and formatting already-formatted output is
//! a no-op: every decision in [`jsx_print`] depends only on the AST and
//! on `rustfmt`'s own (deterministic) output, never on incidental
//! whitespace outside a preserved [`outou_syntax::ast::JsxText`] span or
//! a whole expression island left verbatim by `multiline_guard` (below).
//!
//! **What is left verbatim.** A JSX attribute's string value and every
//! [`outou_syntax::ast::JsxText`] child are always copied byte-for-byte
//! from the source (see `jsx_print`'s module doc for why) — comments and
//! text are never at risk of being lost or reflowed. A file containing
//! any [`outou_syntax::ast::ErrorNode`] is refused outright rather than
//! partially formatted. An expression island containing a multi-line
//! string/raw-string literal, a multi-line block comment, or multi-line
//! JSX text anywhere in its subtree is left byte-for-byte as written, in
//! its entirety, instead of being formatted at all: this crate's
//! line-based reindentation cannot safely touch a token whose own
//! content spans more than one physical line (`multiline_guard`'s module
//! doc has the full reasoning and a reproduction).

mod collect;
mod error;
mod jsx_print;
mod multiline_guard;
mod placeholder;
mod rustfmt_proc;
mod snippet;
mod width;

pub use error::FormatError;

/// Options for [`format_source`].
#[derive(Debug, Clone)]
pub struct FormatOptions {
    /// The Rust edition to pass to `rustfmt` (`--edition`). Should match
    /// the edition of the crate the `.rsx` file belongs to.
    pub edition: String,
    /// The `rustfmt` binary to run: a bare name resolved against `PATH`
    /// (the default, `"rustfmt"`) or an explicit path. Overridable so a
    /// test can point at a program that does not exist and observe
    /// [`FormatError::RustfmtUnavailable`] without needing to actually
    /// uninstall `rustfmt` — see `rustfmt_proc`'s tests and
    /// `crates/outou-cli`'s own CLI test for the same message.
    pub rustfmt_program: String,
}

impl Default for FormatOptions {
    /// Matches this workspace's own edition
    /// (`Cargo.toml`'s `[workspace.package] edition`) and resolves
    /// `rustfmt` from `PATH`.
    fn default() -> Self {
        Self {
            edition: "2021".to_string(),
            rustfmt_program: "rustfmt".to_string(),
        }
    }
}

/// Formats one `.rsx` file's source text.
///
/// Returns [`FormatError::SyntaxErrors`] rather than formatting anything
/// when `source` has a parse error — a broken file is never partially
/// rewritten (`docs/phase0/issues/13-formatter.md`).
pub fn format_source(source: &str, options: &FormatOptions) -> Result<String, FormatError> {
    let parsed = outou_syntax::parse(source);
    if !parsed.diagnostics.is_empty() {
        return Err(FormatError::SyntaxErrors {
            diagnostics: parsed.render_diagnostics("<input>"),
        });
    }
    if collect::has_error_nodes(&parsed.file) {
        return Err(FormatError::SyntaxErrors {
            diagnostics: "the file contains a region the parser could not fully recover"
                .to_string(),
        });
    }

    let top_level = collect::file_top_level_jsx(&parsed.file);
    snippet::format_rust_snippet(source, 0..source.len(), &top_level, false, options)
}

/// Whether `source` is already exactly what [`format_source`] would
/// produce for it.
pub fn is_formatted(source: &str, options: &FormatOptions) -> Result<bool, FormatError> {
    Ok(format_source(source, options)? == source)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> FormatOptions {
        FormatOptions::default()
    }

    #[test]
    fn formats_plain_rust_with_no_jsx_at_all() {
        let source = "fn add(a:i32,b:i32)->i32{a+b}\n";
        let out = format_source(source, &opts()).unwrap();
        assert_eq!(out, "fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n");
    }

    #[test]
    fn refuses_a_file_with_syntax_errors() {
        let source = "fn f() { <div cl";
        let err = format_source(source, &opts()).unwrap_err();
        assert!(matches!(err, FormatError::SyntaxErrors { .. }));
    }

    #[test]
    fn formats_a_single_self_closing_element() {
        let source = "fn f() {\n<div/>\n}\n";
        let out = format_source(source, &opts()).unwrap();
        assert_eq!(out, "fn f() {\n    <div />\n}\n");
    }

    #[test]
    fn is_idempotent_on_a_simple_component() {
        let source = "fn f() { <div class=\"a\"><span>{1}</span></div> }";
        let once = format_source(source, &opts()).unwrap();
        let twice = format_source(&once, &opts()).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn already_formatted_input_is_a_no_op() {
        let source = "fn f() -> Element {\n    <div><span>{1}</span></div>\n}\n";
        assert!(is_formatted(source, &opts()).unwrap());
    }

    #[test]
    fn a_single_child_that_does_not_fit_on_one_line_gets_its_own_line() {
        let source = "fn f() { <div><span_with_a_really_quite_long_name_indeed_to_force_wrapping>{value}</span_with_a_really_quite_long_name_indeed_to_force_wrapping></div> }";
        let out = format_source(source, &opts()).unwrap();
        assert!(out.contains("\n        <span"), "{out}");
    }
}
