//! `outou check`: parses every `.rsx` file under the current crate's
//! `src/` directory and prints Outou's own rendered diagnostics for it
//! (never backend vocabulary — `outou-syntax` diagnoses JSX structure
//! itself, in Outou vocabulary, at the `.rsx` position).
//!
//! This uses the same front end (`outou_syntax::parse`) `cargo build`'s
//! generation step and the language server both use — there is one
//! compiler, only the mode differs (`AGENTS.md`).

use std::path::Path;
use std::process::ExitCode;

use outou_syntax::Severity;

use crate::rsx_files::collect_rsx_files;

/// Result of running Outou's own syntax-diagnostic layer over one file's
/// source, in-process: the exact rendered text `outou check` prints for
/// it (empty when the file has no diagnostics at all) and whether any
/// diagnostic in it is error-severity.
///
/// This is the function the UI test harness
/// (`crates/outou-cli/tests/ui.rs`, issue #11) calls directly instead of
/// spawning the `outou` binary — "there is one compiler" (`AGENTS.md`):
/// [`run`] and the harness must go through the exact same code path, only
/// the caller differs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    /// `outou check`'s rendered output for this one file (may be empty).
    pub rendered: String,
    /// Whether any diagnostic in [`CheckReport::rendered`] is
    /// error-severity — the flag `run()` uses to decide its exit code.
    pub had_error: bool,
}

/// Parses `source` and renders Outou's own syntax diagnostics for it,
/// exactly as `outou check` would for a file whose `--> ` line reads
/// `file_name`. Never panics and never fails: a syntax error is a
/// [`CheckReport`] with `had_error: true`, not an `Err`.
pub fn check_source(file_name: &str, source: &str) -> CheckReport {
    let parsed = outou_syntax::parse(source);
    let had_error = parsed
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error);
    let rendered = parsed.render_diagnostics(file_name);
    CheckReport {
        rendered,
        had_error,
    }
}

/// Runs `outou check`. Exit code `1` if any file has an error diagnostic,
/// `0` otherwise (a file with only warnings still exits `0`).
pub fn run() -> ExitCode {
    let src_dir = Path::new("src");
    let files = match collect_rsx_files(src_dir) {
        Ok(files) => files,
        Err(err) => {
            eprintln!("error: reading {}: {err}", src_dir.display());
            return ExitCode::FAILURE;
        }
    };

    let mut had_errors = false;
    for path in &files {
        let source = match std::fs::read_to_string(path) {
            Ok(source) => source,
            Err(err) => {
                eprintln!("error: reading {}: {err}", path.display());
                had_errors = true;
                continue;
            }
        };

        let report = check_source(&path.display().to_string(), &source);
        if report.had_error {
            had_errors = true;
        }
        if !report.rendered.is_empty() {
            println!("{}", report.rendered);
        }
    }

    if had_errors {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_source_reports_no_error_and_empty_output_for_clean_input() {
        let report = check_source("clean.rsx", "fn f() { <div></div>; }");
        assert!(!report.had_error);
        assert_eq!(report.rendered, "");
    }

    #[test]
    fn check_source_renders_a_syntax_error_at_the_given_file_name() {
        let report = check_source("broken.rsx", "fn f() { <div cl");
        assert!(report.had_error);
        assert!(report.rendered.contains("broken.rsx"));
    }
}
