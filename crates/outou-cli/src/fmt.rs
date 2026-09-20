//! `outou fmt [paths…] [--check]`: formats `.rsx` files in place, or (with
//! `--check`) reports which files are not formatted without writing
//! anything.
//!
//! This calls straight into `outou_fmt::format_source` — the exact same
//! function `textDocument/formatting` in `outou-lsp` calls
//! (`docs/phase0/issues/13-formatter.md`: "one pipeline, not two").
//!
//! `TODO(phase0)`: writing a formatted file (`std::fs::write` in
//! [`format_one_file`]) is not atomic — a write-to-temp-file-then-rename
//! would survive a crash or a full disk mid-write without corrupting the
//! original, at the cost of an extra rename per file and needing to pick
//! a temp path this process is guaranteed write access to next to the
//! original.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use outou_fmt::{FormatError, FormatOptions};

use crate::rsx_files::collect_rsx_files;

/// One `.rsx` file's outcome for one `outou fmt` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileOutcome {
    /// Already formatted; nothing to do.
    AlreadyFormatted,
    /// Formatted (or, in `--check` mode, would be formatted).
    Changed,
    /// Could not be formatted (a syntax error, or `rustfmt` itself
    /// failed). Never a panic and never silently skipped
    /// (`docs/phase0/issues/13-formatter.md`).
    Refused(String),
}

/// Runs `outou fmt` over `paths` (files and/or directories; a directory
/// is scanned recursively for `.rsx` files, same rule `outou check`
/// uses). Empty `paths` defaults to `src` under the current directory.
///
/// `check_only: true` never writes to disk; it only reports what would
/// change. Returns [`ExitCode::FAILURE`] when `check_only` and at least
/// one file is not already formatted, or when any file was refused;
/// [`ExitCode::SUCCESS`] otherwise.
pub fn run(paths: &[PathBuf], check_only: bool) -> ExitCode {
    run_with_options(paths, check_only, &FormatOptions::default())
}

fn run_with_options(paths: &[PathBuf], check_only: bool, options: &FormatOptions) -> ExitCode {
    let files = match resolve_files(paths) {
        Ok(files) => files,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };

    let mut any_unformatted = false;
    let mut any_refused = false;

    for path in &files {
        match format_one_file(path, options, check_only) {
            FileOutcome::AlreadyFormatted => {}
            FileOutcome::Changed => any_unformatted = true,
            FileOutcome::Refused(_) => any_refused = true,
        }
    }

    if any_refused || (check_only && any_unformatted) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Formats one file: reads it, classifies it, and — unless `check_only`
/// — writes the result back if it changed. Calls
/// [`outou_fmt::format_source`] exactly once (via [`classify`]) rather
/// than once to decide whether anything changed and again to get the
/// text to write, and never `.expect()`s that second call succeeded: it
/// already has the formatted text from the first and only call.
fn format_one_file(path: &Path, options: &FormatOptions, check_only: bool) -> FileOutcome {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(err) => {
            eprintln!("error: reading {}: {err}", path.display());
            return FileOutcome::Refused(err.to_string());
        }
    };

    match classify(&source, options) {
        Classified::AlreadyFormatted => FileOutcome::AlreadyFormatted,
        Classified::Changed(formatted) => {
            if check_only {
                println!("{}", path.display());
            } else if let Err(err) = std::fs::write(path, formatted) {
                eprintln!("error: writing {}: {err}", path.display());
                return FileOutcome::Refused(err.to_string());
            } else {
                println!("formatted {}", path.display());
            }
            FileOutcome::Changed
        }
        Classified::Refused(message) => {
            eprintln!("error: {}: {message}", path.display());
            FileOutcome::Refused(message)
        }
    }
}

/// The full result of formatting one file's source text once: whether it
/// is already formatted, and — when it is not — the actual formatted
/// text, so a caller never needs to call [`outou_fmt::format_source`] a
/// second time just to get text it already computed once.
enum Classified {
    AlreadyFormatted,
    Changed(String),
    Refused(String),
}

fn classify(source: &str, options: &FormatOptions) -> Classified {
    match outou_fmt::format_source(source, options) {
        Ok(formatted) if formatted == source => Classified::AlreadyFormatted,
        Ok(formatted) => Classified::Changed(formatted),
        Err(FormatError::SyntaxErrors { diagnostics }) => Classified::Refused(diagnostics),
        Err(err) => Classified::Refused(err.to_string()),
    }
}

/// The [`FileOutcome`] for one file's source text, without touching
/// disk. Exposed separately from [`run`] so this crate's own tests can
/// ask the same question without spawning a process.
pub fn outcome_for(source: &str, options: &FormatOptions) -> FileOutcome {
    match classify(source, options) {
        Classified::AlreadyFormatted => FileOutcome::AlreadyFormatted,
        Classified::Changed(_) => FileOutcome::Changed,
        Classified::Refused(message) => FileOutcome::Refused(message),
    }
}

/// Expands `paths` into a sorted, deduplicated list of `.rsx` files: a
/// path to a file is used as-is (regardless of extension, matching
/// `rustfmt`'s own behavior of formatting whatever file it is pointed
/// at); a directory is scanned recursively for `.rsx` files. Defaults to
/// `src` in the current directory when `paths` is empty, matching `outou
/// check`.
fn resolve_files(paths: &[PathBuf]) -> std::io::Result<Vec<PathBuf>> {
    if paths.is_empty() {
        return collect_rsx_files(Path::new("src"));
    }
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            files.extend(collect_rsx_files(path)?);
        } else {
            files.push(path.clone());
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn already_formatted_source_reports_no_change() {
        let source = "fn f() {\n    <div />\n}\n";
        let outcome = outcome_for(source, &FormatOptions::default());
        assert_eq!(outcome, FileOutcome::AlreadyFormatted);
    }

    #[test]
    fn unformatted_source_reports_a_change() {
        let source = "fn f() { <div  /> }";
        let outcome = outcome_for(source, &FormatOptions::default());
        assert_eq!(outcome, FileOutcome::Changed);
    }

    #[test]
    fn a_syntax_error_is_refused_not_panicked() {
        let source = "fn f() { <div cl";
        let outcome = outcome_for(source, &FormatOptions::default());
        assert!(matches!(outcome, FileOutcome::Refused(_)));
    }

    // `resolve_files`'s empty-`paths` default (`collect_rsx_files(Path::new("src"))`
    // relative to the current directory) is not tested by mutating the
    // process-wide current directory here: `cargo test` runs a crate's
    // tests concurrently on multiple threads, and `std::env::set_current_dir`
    // is process, not thread, state, so doing that in one test could make
    // an unrelated concurrently running test resolve relative paths
    // against the wrong directory. `collect_rsx_files` itself (this
    // function's only real logic) is covered directly in
    // `crate::rsx_files::tests`.

    #[test]
    fn run_calls_format_source_exactly_once_per_changed_file() {
        let dir = std::env::temp_dir().join(format!(
            "outou-cli-fmt-test-count-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.rsx");
        std::fs::write(&file, "fn f() { <div  /> }").unwrap();
        let counter = dir.join("count.txt");
        let script = dir.join("counting-rustfmt.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\necho x >> \"{}\"\nexec rustfmt \"$@\"\n",
                counter.display()
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).unwrap();
        }

        let options = FormatOptions {
            rustfmt_program: script.to_string_lossy().to_string(),
            ..FormatOptions::default()
        };
        run_with_options(std::slice::from_ref(&file), false, &options);

        let count = std::fs::read_to_string(&counter)
            .map(|s| s.lines().count())
            .unwrap_or(0);
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(
            count, 1,
            "format_source (and so rustfmt) must be invoked exactly once per changed file, not twice"
        );
    }

    #[test]
    fn resolve_files_expands_a_directory_argument() {
        let dir = std::env::temp_dir().join(format!(
            "outou-cli-fmt-test-dir-arg-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.rsx"), "").unwrap();
        std::fs::write(dir.join("b.rsx"), "").unwrap();

        let files = resolve_files(std::slice::from_ref(&dir)).unwrap();

        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(files, vec![dir.join("a.rsx"), dir.join("b.rsx")]);
    }
}
