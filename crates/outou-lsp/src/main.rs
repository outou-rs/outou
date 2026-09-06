//! `outou-lsp`: the language server for `.rsx` files.
//!
//! It owns the `outou-rsx` language in the editor and forwards completion,
//! hover, definition and diagnostics to rust-analyzer, translating positions
//! through `outou_sourcemap::Registry` in both directions. It uses the same
//! `outou_codegen` pipeline as `cargo build` (via `outou_cli::build`), in
//! recovery mode.

mod complete;
mod diagnostics;
mod dispatch;
mod documents;
mod mapping;
mod plan;
mod ra;
mod response;
mod server;
mod translate;
mod uri;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("outou-lsp {VERSION}");
        return ExitCode::SUCCESS;
    }

    server::run(resolve_rust_analyzer)
}

/// Locates the rust-analyzer binary this server will spawn for a planned
/// `.rsx` workspace: `OUTOU_RUST_ANALYZER` if set, otherwise the first
/// `rust-analyzer` (or, on Windows, `rust-analyzer.exe`) found on `PATH`.
///
/// Resolved lazily, only once `crate::server::load_workspace` finds a real
/// `.rsx` crate root that actually needs rust-analyzer (issue #9 Gate 3
/// review, S4): this used to run once, unconditionally, before
/// `initialize` even started, so a machine with no `rust-analyzer` on
/// `PATH` (and no `OUTOU_RUST_ANALYZER`) could never start `outou-lsp` at
/// all — not even syntax-only, for a crate that turns out to have no
/// `.rsx` root, or none open yet.
fn resolve_rust_analyzer() -> Result<String, String> {
    if let Ok(path) = std::env::var("OUTOU_RUST_ANALYZER") {
        return if Path::new(&path).is_file() {
            Ok(path)
        } else {
            Err(format!(
                "OUTOU_RUST_ANALYZER is set to `{path}`, but no file exists there"
            ))
        };
    }

    if let Some(found) = find_on_path("rust-analyzer") {
        return Ok(found);
    }

    Err(
        "rust-analyzer was not found on PATH; install it (`rustup component add rust-analyzer`) \
or set OUTOU_RUST_ANALYZER to its location"
            .to_string(),
    )
}

/// Searches `PATH` for an executable named `name` (or, on Windows,
/// `name.exe`), returning its full path.
fn find_on_path(name: &str) -> Option<String> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate: PathBuf = dir.join(name);
        if is_executable_file(&candidate) {
            return Some(candidate.to_string_lossy().into_owned());
        }
        if cfg!(windows) {
            let candidate = dir.join(format!("{name}.exe"));
            if is_executable_file(&candidate) {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}
