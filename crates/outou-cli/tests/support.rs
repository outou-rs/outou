//! Helpers shared by every `outou-cli` integration test binary that
//! copies a fixture into a temp directory (`build_paths.rs` and
//! `build_compile.rs` — split out of one `tests/build.rs`, issue #8 fix
//! list step 10; `build_cli.rs` needs none of these and does not declare
//! `mod support;`). Kept to exactly the functions every such binary uses:
//! an item unused by *any one* of them is flagged `dead_code` in that
//! binary's own compilation, since each `mod support;` inclusion is
//! checked independently — a helper needed by only one file belongs in
//! that file instead, private.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The repository root, computed from this crate's own manifest directory
/// so tests work regardless of the current working directory `cargo test`
/// was invoked from.
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root exists")
}

/// `tests/fixtures/modules`, under the repository root.
pub fn fixtures_dir() -> PathBuf {
    repo_root().join("tests/fixtures/modules")
}

/// Copies `src` into a fresh temp directory and returns it. Every test
/// gets its own directory (a unique suffix from the current time plus the
/// process id) so parallel test threads never collide.
pub fn copy_to_temp(src: &Path, label: &str) -> PathBuf {
    let dest = std::env::temp_dir().join(format!(
        "outou-cli-build-it-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    copy_dir(src, &dest);
    // `build()` canonicalizes its manifest directory internally (its
    // generated paths — and their `file://` URIs — must be absolute); on
    // some platforms `std::env::temp_dir()` itself is a symlink (macOS:
    // `/var` -> `/private/var`), so canonicalize here too, or every path
    // this test compares against `report.generated_files` would silently
    // mismatch on the un-resolved prefix.
    dest.canonicalize()
        .unwrap_or_else(|e| panic!("canonicalizing {}: {e}", dest.display()))
}

/// Recursively copies `src` into `dest`, creating directories as needed.
pub fn copy_dir(src: &Path, dest: &Path) {
    fs::create_dir_all(dest).unwrap_or_else(|e| panic!("creating {}: {e}", dest.display()));
    for entry in fs::read_dir(src).unwrap_or_else(|e| panic!("reading {}: {e}", src.display())) {
        let entry = entry.unwrap();
        let target = dest.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target)
                .unwrap_or_else(|e| panic!("copying {}: {e}", entry.path().display()));
        }
    }
}
