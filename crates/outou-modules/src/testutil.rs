//! Minimal temp-directory helper for tests that need real files on disk
//! (a permission-denied directory, a deep chain of module files) — kept
//! in-tree instead of adding a `tempfile` dependency for a Phase 0 spike
//! crate. Test-only (`#[cfg(test)]` in `lib.rs`).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A directory under the OS temp dir, removed when dropped.
pub(crate) struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Creates a fresh, empty directory named `outou-modules-{prefix}-…`.
    pub(crate) fn new(prefix: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("outou-modules-{prefix}-{}-{n}", std::process::id()));
        fs::create_dir_all(&path).expect("creating temp dir");
        TempDir { path }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
