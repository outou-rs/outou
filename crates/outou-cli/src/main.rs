//! The `outou` command line tool.

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

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Build => todo!("outou build: Phase 0, Week 4 (Gate 2)"),
        Command::Check => todo!("outou check: Phase 0, Week 3 (Gate 1)"),
        Command::Package => todo!("outou package: Phase 0, Week 6"),
    }
}
