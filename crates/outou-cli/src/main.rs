//! The `outou` command line tool.

mod check;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

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
    Build,
    /// Parse every `.rsx` file and report Outou syntax diagnostics.
    Check,
    /// Generate Rust and prepare the crate for `cargo publish`.
    ///
    /// Published crates ship pre-generated Rust so that consumers need
    /// neither `outou` nor a build script.
    Package,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Build => todo!("outou build: Phase 0, Week 4 (Gate 2)"),
        Command::Check => check::run(),
        Command::Package => todo!("outou package: Phase 0, Week 6"),
    }
}
