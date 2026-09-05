//! `outou-lsp`: the language server for `.rsx` files.
//!
//! It owns the `outou-rsx` language in the editor and forwards completion,
//! hover, definition and diagnostics to rust-analyzer, translating positions
//! through `outou_sourcemap::Registry` in both directions. It uses the same
//! `outou_codegen` pipeline as `cargo build`, in recovery mode.

use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("outou-lsp {VERSION}");
        return ExitCode::SUCCESS;
    }
    eprintln!("outou-lsp {VERSION}: the language server is not implemented yet (Phase 0, Week 5)");
    ExitCode::FAILURE
}
