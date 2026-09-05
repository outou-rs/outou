//! `cargo xtask`: repository automation that does not belong in any crate.

mod determinism;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// Repository automation for Outou.
#[derive(Debug, Parser)]
#[command(name = "xtask", about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Manage external syntax corpora listed in `corpus.lock`.
    Corpus {
        #[command(subcommand)]
        command: Corpus,
    },
    /// Generate every fixture through the build path and the LSP path and
    /// verify that the normalized outputs and source maps are identical.
    Determinism,
    /// Build distributable artifacts (CLI, LSP, editor package).
    Dist,
}

#[derive(Debug, Subcommand)]
enum Corpus {
    /// Clone every corpus at its pinned commit into `.corpus/`.
    Fetch,
    /// Run the parser over every fetched corpus and report failures.
    Test,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Corpus {
            command: Corpus::Fetch,
        } => not_implemented("corpus fetch", "Week 6"),
        Command::Corpus {
            command: Corpus::Test,
        } => not_implemented("corpus test", "Week 6"),
        Command::Determinism => determinism::run(),
        Command::Dist => not_implemented("dist", "after Phase 0"),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::FAILURE
        }
    }
}

fn not_implemented(task: &str, when: &str) -> Result<(), String> {
    Err(format!(
        "`{task}` is not implemented yet (planned for Phase 0, {when})"
    ))
}
