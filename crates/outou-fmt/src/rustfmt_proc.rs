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

    // `stdin` is written on its own thread while this thread drains
    // `rustfmt`'s stdout and stderr in `wait_with_output`. Writing it
    // inline would deadlock as soon as `rustfmt` filled its output pipe
    // while this process was still blocked writing input: neither side
    // could make progress. Dropping the handle at the end of the thread
    // closes the pipe so `rustfmt` sees end of input.
    let mut stdin = child.stdin.take().expect("stdin was piped");
    let bytes = input.as_bytes().to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&bytes));

    let output = child
        .wait_with_output()
        .map_err(|source| FormatError::RustfmtUnavailable { source })?;
    let write_result = match writer.join() {
        Ok(result) => result,
        Err(_) => {
            return Err(FormatError::Internal(
                "the thread writing to rustfmt's stdin panicked".to_string(),
            ))
        }
    };

    if !output.status.success() {
        // `rustfmt` can exit (a syntax error) before reading all of its
        // input, so a broken pipe is expected here and the exit status is
        // the real answer.
        if let Err(source) = write_result {
            if source.kind() != std::io::ErrorKind::BrokenPipe {
                return Err(FormatError::RustfmtUnavailable { source });
            }
        }
        return Err(FormatError::RustfmtFailed {
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    // A successful exit that did not read all of its input produced output
    // for a truncated file; accepting it would silently cut the user's code.
    if let Err(source) = write_result {
        return Err(if source.kind() == std::io::ErrorKind::BrokenPipe {
            FormatError::Internal(
                "rustfmt exited successfully without reading all of its input".to_string(),
            )
        } else {
            FormatError::RustfmtUnavailable { source }
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

    /// A program that echoes its input while still reading it fills its
    /// stdout pipe long before the input is fully written. Writing stdin
    /// inline deadlocked here; a watchdog turns a regression into a failure
    /// instead of a hang.
    #[cfg(unix)]
    #[test]
    fn a_large_input_does_not_deadlock_against_a_program_that_echoes_it() {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::mpsc;
        use std::time::Duration;

        const ETXTBSY: i32 = 26;
        let dir = std::env::temp_dir().join(format!(
            "outou-fmt-echo-test-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("echo-rustfmt.sh");
        std::fs::write(&script, "#!/bin/sh\nexec cat\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let options = FormatOptions {
            rustfmt_program: script.to_string_lossy().into_owned(),
            ..FormatOptions::default()
        };
        let input = "fn f() {}\n".repeat(400_000);

        let (sender, receiver) = mpsc::channel();
        let expected = input.clone();
        std::thread::spawn(move || {
            // Another thread forking while the script was open for writing
            // can make the first spawn fail with ETXTBSY; retry.
            let mut result = run_rustfmt(&input, &options);
            for _ in 0..20 {
                match &result {
                    Err(FormatError::RustfmtUnavailable { source })
                        if source.raw_os_error() == Some(ETXTBSY) =>
                    {
                        std::thread::sleep(Duration::from_millis(50));
                        result = run_rustfmt(&input, &options);
                    }
                    _ => break,
                }
            }
            sender.send(result).ok();
        });
        let result = receiver
            .recv_timeout(Duration::from_secs(60))
            .expect("run_rustfmt deadlocked on a large input");
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(result.expect("the echo program succeeds"), expected);
    }

    /// A program that closes stdin early and still exits 0 produced output
    /// for a truncated file; that must be an error, not a success.
    #[cfg(unix)]
    #[test]
    fn a_successful_exit_that_did_not_read_all_input_is_an_error() {
        use std::os::unix::fs::PermissionsExt;
        use std::time::Duration;

        const ETXTBSY: i32 = 26;
        let dir = std::env::temp_dir().join(format!(
            "outou-fmt-truncating-test-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("truncating-rustfmt.sh");
        std::fs::write(&script, "#!/bin/sh\nhead -c 1\nexit 0\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let options = FormatOptions {
            rustfmt_program: script.to_string_lossy().into_owned(),
            ..FormatOptions::default()
        };
        let input = "fn f() {}\n".repeat(400_000);
        let mut result = run_rustfmt(&input, &options);
        for _ in 0..20 {
            match &result {
                Err(FormatError::RustfmtUnavailable { source })
                    if source.raw_os_error() == Some(ETXTBSY) =>
                {
                    std::thread::sleep(Duration::from_millis(50));
                    result = run_rustfmt(&input, &options);
                }
                _ => break,
            }
        }
        std::fs::remove_dir_all(&dir).ok();
        assert!(
            matches!(result, Err(FormatError::Internal(_))),
            "a truncated output was accepted: {:?}",
            result.map(|text| text.len())
        );
    }

    #[test]
    fn has_tab_indentation_detects_a_leading_tab() {
        assert!(has_tab_indentation("fn f() {\n\tlet x = 1;\n}\n"));
        assert!(!has_tab_indentation("fn f() {\n    let x = 1;\n}\n"));
    }
}
