//! The `outou` command line tool.
//!
//! A thin argument-parsing wrapper: the actual `build` pipeline lives in
//! the `outou_cli` library (`src/lib.rs`) so it can also be called
//! directly by `cargo xtask determinism` and by this crate's own tests.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

use outou_cli::build::{self, BuildOptions, Mode as BuildMode};
use outou_cli::check;

/// Rust with JSX.
#[derive(Debug, Parser)]
#[command(name = "outou", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Generate Rust from every `.rsx` file of the current crate.
    Build {
        /// Directory containing the crate's `Cargo.toml`. Defaults to the
        /// current directory.
        #[arg(long)]
        manifest_dir: Option<PathBuf>,
        /// Strict mode fails on any syntax error (the mode `cargo build`
        /// needs); recovery mode never does.
        #[arg(long, value_enum, default_value_t = ModeArg::Strict)]
        mode: ModeArg,
    },
    /// Parse every `.rsx` file and report Outou syntax diagnostics.
    Check,
    /// Generate Rust and prepare the crate for `cargo publish`.
    ///
    /// Published crates ship pre-generated Rust so that consumers need
    /// neither `outou` nor a build script.
    Package,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ModeArg {
    Strict,
    Recovery,
}

impl From<ModeArg> for BuildMode {
    fn from(value: ModeArg) -> Self {
        match value {
            ModeArg::Strict => BuildMode::Strict,
            ModeArg::Recovery => BuildMode::Recovery,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Build { manifest_dir, mode } => run_build(manifest_dir, mode.into()),
        Command::Check => check::run(),
        Command::Package => todo!("outou package: Phase 0, Week 6"),
    }
}

/// Runs `outou build`, printing a short summary on success and Outou's
/// own rendered diagnostics (never backend vocabulary) on failure.
fn run_build(manifest_dir: Option<PathBuf>, mode: BuildMode) -> ExitCode {
    let manifest_dir = manifest_dir.unwrap_or_else(|| PathBuf::from("."));
    let opts = BuildOptions { manifest_dir, mode };

    match build::build(&opts) {
        Ok(report) if !report.built => {
            println!("outou build: no `.rsx` crate root found; nothing to do");
            ExitCode::SUCCESS
        }
        Ok(report) => {
            println!(
                "outou build: generated {} file(s), removed {} stale file(s)",
                report.generated_files.len(),
                report.removed_files.len()
            );
            for file in &report.generated_files {
                println!("  {}", file.display());
            }
            for file in &report.removed_files {
                println!("  removed {}", file.display());
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}
