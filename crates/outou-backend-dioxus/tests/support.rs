//! Shared helpers for this crate's integration tests.

use std::path::{Path, PathBuf};

/// The repository root, computed from this crate's own manifest directory
/// so tests work regardless of the current working directory `cargo test`
/// was invoked from.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/outou-backend-dioxus has a parent")
        .parent()
        .expect("crates/ has a parent")
        .to_path_buf()
}
