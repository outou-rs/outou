//! Runs `rustfmt` as an external process over stdin/stdout. Never a
//! custom Rust formatter (`docs/phase0/issues/13-formatter.md`).

use std::io::Write;
use std::process::{Command, Stdio};

use crate::error::FormatError;
use crate::FormatOptions;

/// Formats `input` (a complete, syntactically valid Rust source text) by
/// piping it through `<options.rustfmt_program> --edition <options.edition>`
/// and reading its stdout back.
///
/// Failure to start the program at all (not installed, not on `PATH`)
/// and a non-zero exit status are both reported as explicit
/// [`FormatError`]s — never silently swallowed, and never a fallback to
/// leaving the text unformatted (`docs/phase0/issues/13-formatter.md`).
pub(crate) fn run_rustfmt(input: &str, options: &FormatOptions) -> Result<String, FormatError> {
    let mut child = build_command(options)
        .spawn()
        .map_err(|source| FormatError::RustfmtUnavailable { source })?;

    // This blocks until all of `input` is written, then only afterward
    // reads stdout/stderr via `wait_with_output`. The realistic deadlock
    // is not primarily "the input itself is huge" (a `.rsx` file's Rust
    // content comfortably fits an OS pipe buffer): it is `rustfmt`
    // writing enough to *its* stderr (or stdout) pipe to fill that
    // buffer — plausible for a file with many warnings or an unusual
    // error — while this process is still blocked writing stdin and so
    // never reaches the point where it would drain rustfmt's stderr.
    // Neither side can then make progress. `TODO(phase0)`: write stdin on
    // a separate thread (or interleave writing/reading) if a real file
    // ever deadlocks here.
    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(input.as_bytes())
        .map_err(|source| FormatError::RustfmtUnavailable { source })?;

    let output = child
        .wait_with_output()
        .map_err(|source| FormatError::RustfmtUnavailable { source })?;

    if !output.status.success() {
        return Err(FormatError::RustfmtFailed {
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    let formatted = String::from_utf8(output.stdout).map_err(|err| {
        FormatError::Internal(format!("rustfmt produced non-UTF-8 output: {err}"))
    })?;

    if has_tab_indentation(&formatted) {
        return Err(FormatError::TabIndentedOutput);
    }
    Ok(formatted)
}

/// Builds the `rustfmt` invocation for `options`, without spawning it —
/// kept separate so a test can inspect the exact arguments (in
/// particular, that `hard_tabs=false` is always present) without
/// actually running a process.
fn build_command(options: &FormatOptions) -> Command {
    let mut command = Command::new(&options.rustfmt_program);
    command
        .arg("--edition")
        .arg(&options.edition)
        // Overrides even a discovered `rustfmt.toml` setting
        // `hard_tabs = true` (issue #13 review): this crate's own
        // indentation math adds or removes a fixed number of *spaces*
        // per level, which is meaningless against a tab-indented line.
        .arg("--config")
        .arg("hard_tabs=false")
        .arg("--emit")
        .arg("stdout")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

/// Whether any line of `text` is indented with a tab character. A
/// defensive backstop behind the `hard_tabs=false` override above:
/// checked, not just assumed, so a `rustfmt` that ignores or rejects
/// that override on some future version is caught here rather than
/// quietly corrupting output (`FormatError::TabIndentedOutput`).
fn has_tab_indentation(text: &str) -> bool {
    text.lines().any(|line| {
        let leading_whitespace_len = line.len() - line.trim_start().len();
        line[..leading_whitespace_len].contains('\t')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_a_trivial_rust_file() {
        let formatted = run_rustfmt("fn main( ) {\nlet x=1;\n}\n", &FormatOptions::default())
            .expect("rustfmt runs");
        assert_eq!(formatted, "fn main() {\n    let x = 1;\n}\n");
    }

    #[test]
    fn a_syntax_error_is_reported_as_a_failure_not_a_panic() {
        let result = run_rustfmt("fn main( {\n", &FormatOptions::default());
        assert!(matches!(result, Err(FormatError::RustfmtFailed { .. })));
    }

    /// [`FormatOptions::rustfmt_program`] is what makes an
    /// unavailable-`rustfmt` error reproducible in a test without
    /// actually uninstalling it — used again by `outou-cli`'s own CLI
    /// test of the missing-`rustfmt` message.
    #[test]
    fn an_unresolvable_program_name_is_reported_as_unavailable_not_swallowed() {
        let options = FormatOptions {
            rustfmt_program: "outou-fmt-test-nonexistent-program-xyz".to_string(),
            ..FormatOptions::default()
        };
        let result = run_rustfmt("fn main() {}\n", &options);
        assert!(matches!(
            result,
            Err(FormatError::RustfmtUnavailable { .. })
        ));
    }

    /// The command always carries `--config hard_tabs=false`: a
    /// discovered `rustfmt.toml` setting `hard_tabs = true` would
    /// otherwise make this crate's space-based indentation math
    /// (`crate::snippet`, `crate::jsx_print::island`) meaningless.
    #[test]
    fn the_command_always_overrides_hard_tabs_to_false() {
        let command = build_command(&FormatOptions::default());
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let config_flag_index = args
            .iter()
            .position(|arg| arg == "--config")
            .expect("--config is always passed");
        assert_eq!(
            args.get(config_flag_index + 1).map(String::as_str),
            Some("hard_tabs=false")
        );
    }

    /// End-to-end: even when a `rustfmt.toml` on the filesystem sets
    /// `hard_tabs = true`, `--config hard_tabs=false` on the command
    /// line wins (verified directly against the real `rustfmt` binary,
    /// not just that the argument is present, in case a future rustfmt
    /// version changes CLI/file precedence).
    #[test]
    fn a_discovered_hard_tabs_config_is_overridden_end_to_end() {
        let dir = std::env::temp_dir().join(format!(
            "outou-fmt-hard-tabs-test-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("rustfmt.toml");
        std::fs::write(&config_path, "hard_tabs = true\n").unwrap();

        let mut command = build_command(&FormatOptions::default());
        command
            .arg("--config-path")
            .arg(&config_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("rustfmt runs");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"fn main() {\nlet x=1;\n}\n")
            .unwrap();
        let output = child.wait_with_output().unwrap();

        std::fs::remove_dir_all(&dir).ok();

        assert!(output.status.success());
        let formatted = String::from_utf8(output.stdout).unwrap();
        assert!(!has_tab_indentation(&formatted), "{formatted:?}");
        assert_eq!(formatted, "fn main() {\n    let x = 1;\n}\n");
    }

    #[test]
    fn has_tab_indentation_detects_a_leading_tab() {
        assert!(has_tab_indentation("fn f() {\n\tlet x = 1;\n}\n"));
        assert!(!has_tab_indentation("fn f() {\n    let x = 1;\n}\n"));
    }
}
