//! `outou cargo <build|check|clippy|test> [args]` (issue #25, A5).
//!
//! 1. Generate Rust for the crate (or workspace) exactly like `outou
//!    build`: always Strict, so a syntax error stops here, in Outou's own
//!    words, before Cargo runs. Front-end warnings (as `outou check`
//!    prints them) go to standard error first, whether or not any file
//!    was rewritten.
//! 2. Run `cargo <sub> --message-format=json-diagnostic-rendered-ansi
//!    <args>` and read its standard output line by line.
//! 3. A compiler diagnostic is translated to its `.rsx` position
//!    ([`crate::diagnostics`]) and printed to standard error. Any other
//!    line (a test's own output) is passed through to standard output
//!    unchanged. The exit code is Cargo's.
//!
//! `RUSTC_WORKSPACE_WRAPPER` is not used: clippy ignores it. The JSON
//! stream is the one interface that `build`, `check`, `clippy` and `test`
//! share.
//!
//! # Arguments
//!
//! - `--fix` (before `--`) is rejected: a fix would rewrite
//!   `src/.generated/`, which is overwritten by the next `outou build`.
//! - `--message-format` (before `--`) is rejected: `outou cargo` needs
//!   the JSON form. The command fails with exit code `2` instead of
//!   silently choosing one of the two.
//! - `--manifest-path <file>` is forwarded, and also decides which crate
//!   is generated. Without it, the current directory is.
//! - Everything else, including arguments after `--`, goes to Cargo.

use std::io::{BufRead, BufReader, IsTerminal, Write};
use std::path::PathBuf;
use std::process::{Command, ExitCode, ExitStatus, Stdio};

use crate::build::{self, BuildOptions};
use crate::diagnostics::{
    classify_line, render_translated, translate, CargoLine, DiskResolver, ExplanationNotes,
    TwinFilter,
};

/// The Cargo subcommands `outou cargo` accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum CargoSubcommand {
    /// `cargo build`
    Build,
    /// `cargo check`
    Check,
    /// `cargo clippy`
    Clippy,
    /// `cargo test`
    Test,
}

impl CargoSubcommand {
    fn name(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Check => "check",
            Self::Clippy => "clippy",
            Self::Test => "test",
        }
    }
}

/// The `--message-format` Cargo is asked for.
pub const MESSAGE_FORMAT: &str = "--message-format=json-diagnostic-rendered-ansi";

/// Exit code for a command line `outou cargo` refuses (clap uses `2` too).
const USAGE_ERROR: u8 = 2;

/// Added to a signal number to form a shell-style exit code.
#[cfg(unix)]
const SIGNAL_EXIT_BASE: i32 = 128;

/// Arguments `outou cargo` understood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoArgs {
    /// Directory of the crate to generate.
    pub manifest_dir: PathBuf,
}

/// Why a command line was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ArgError {
    /// `--fix` before `--`.
    #[error("`--fix` is not supported by `outou cargo`: it would rewrite `src/.generated/`, which `outou build` overwrites. Edit the `.rsx` file instead")]
    Fix,
    /// `--message-format` before `--`.
    #[error("`outou cargo` sets `--message-format` itself to translate diagnostics to `.rsx` positions; remove `{0}`, or run `cargo` directly")]
    MessageFormat(String),
}

/// Validates the arguments that will be forwarded to Cargo and finds the
/// crate to generate.
pub fn parse_args(args: &[String]) -> Result<CargoArgs, ArgError> {
    let mut manifest_dir = PathBuf::from(".");
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--" {
            break;
        }
        if arg == "--fix" {
            return Err(ArgError::Fix);
        }
        if arg == "--message-format" || arg.starts_with("--message-format=") {
            return Err(ArgError::MessageFormat(arg.clone()));
        }
        let manifest = if arg == "--manifest-path" {
            iter.next().map(String::as_str)
        } else {
            arg.strip_prefix("--manifest-path=")
        };
        if let Some(manifest) = manifest {
            manifest_dir = match PathBuf::from(manifest).parent() {
                Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
                _ => PathBuf::from("."),
            };
        }
    }
    Ok(CargoArgs { manifest_dir })
}

/// Runs `outou cargo`.
pub fn run(subcommand: CargoSubcommand, args: &[String]) -> ExitCode {
    let parsed = match parse_args(args) {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(USAGE_ERROR);
        }
    };

    match build::build(&BuildOptions::new(&parsed.manifest_dir)) {
        Ok(report) => report.print_warnings(),
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    }

    let mut child = match Command::new(cargo_binary())
        .arg(subcommand.name())
        .arg(MESSAGE_FORMAT)
        .args(args)
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            eprintln!("error: running `cargo {}`: {err}", subcommand.name());
            return ExitCode::FAILURE;
        }
    };

    if let Some(stdout) = child.stdout.take() {
        forward(BufReader::new(stdout));
    }

    match child.wait() {
        Ok(status) => exit_code_of(status),
        Err(err) => {
            eprintln!("error: waiting for `cargo`: {err}");
            ExitCode::FAILURE
        }
    }
}

/// The exit code to report for Cargo's `status`: its own code, or on Unix
/// `128 + N` when a signal `N` ended it (the shell's convention, so `^C`
/// and `kill` are not reported as a plain failure).
fn exit_code_of(status: ExitStatus) -> ExitCode {
    if let Some(code) = status.code() {
        return u8::try_from(code).map_or(ExitCode::FAILURE, ExitCode::from);
    }
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
        return u8::try_from(SIGNAL_EXIT_BASE + signal).map_or(ExitCode::FAILURE, ExitCode::from);
    }
    ExitCode::FAILURE
}

/// The `cargo` binary: `$CARGO` when set (the same one `outou package`
/// uses), else `cargo` on `PATH`.
fn cargo_binary() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

/// Reads Cargo's standard output, translating diagnostics and passing
/// everything else through. Each diagnostic is printed as soon as it
/// arrives, so it stays in order with Cargo's own messages on the
/// terminal.
fn forward(mut reader: impl BufRead) {
    let color = std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let mut resolver = DiskResolver::new();
    let mut twins = TwinFilter::default();
    let mut explanations = ExplanationNotes::default();
    let mut buf = Vec::new();
    let mut build_finished = false;
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        // After `build-finished` every line is the program's (`cargo
        // test`), even one that looks like a cargo record.
        let record = if build_finished {
            None
        } else {
            std::str::from_utf8(&buf)
                .ok()
                .and_then(|line| classify_line(line.trim_end_matches(['\r', '\n'])))
        };
        match record {
            Some(CargoLine::Message(message)) => {
                let Some(translated) = translate(&message, &mut resolver) else {
                    continue;
                };
                let shown = if translated.level == "failure-note" {
                    explanations.rewrite(translated)
                } else {
                    twins.admit(&translated).then(|| {
                        explanations.record(&translated);
                        translated
                    })
                };
                if let Some(shown) = shown {
                    eprintln!("{}", render_translated(&shown, color));
                }
            }
            Some(CargoLine::Other) => {}
            Some(CargoLine::BuildFinished) => build_finished = true,
            None => {
                let mut stdout = std::io::stdout().lock();
                stdout.write_all(&buf).ok();
                stdout.flush().ok();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn fix_before_the_double_dash_is_refused() {
        assert_eq!(parse_args(&args(&["--fix"])), Err(ArgError::Fix));
        assert!(parse_args(&args(&["--", "--fix"])).is_ok());
    }

    #[test]
    fn message_format_is_refused_in_both_spellings() {
        for spelling in [
            &["--message-format=json"][..],
            &["--message-format", "json"][..],
        ] {
            assert!(matches!(
                parse_args(&args(spelling)),
                Err(ArgError::MessageFormat(_))
            ));
        }
    }

    #[test]
    fn the_manifest_path_decides_the_crate_to_generate() {
        let parsed = parse_args(&args(&["--manifest-path", "app/Cargo.toml"])).unwrap();
        assert_eq!(parsed.manifest_dir, PathBuf::from("app"));
        let parsed = parse_args(&args(&["--manifest-path=Cargo.toml"])).unwrap();
        assert_eq!(parsed.manifest_dir, PathBuf::from("."));
        assert_eq!(parse_args(&[]).unwrap().manifest_dir, PathBuf::from("."));
    }
}
