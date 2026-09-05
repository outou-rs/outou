//! `cargo xtask corpus`: fetches and tests the external syntax corpora
//! listed in `corpus.lock` (issue #12, feeds Gate 4's parser-and-corpus
//! report). Nothing under `.corpus/` is vendored into the repository; see
//! `corpus.lock`'s own header comment and `.gitignore`.

mod fetch;
mod lock;
mod report;
mod splice;
mod test;
mod walk;

use std::path::{Path, PathBuf};

pub use lock::CorpusEntry;

/// `cargo xtask corpus fetch`.
pub fn fetch() -> Result<(), String> {
    let root = repo_root();
    let entries = read_lock(&root)?;
    fetch::run(&root, &entries)
}

/// `cargo xtask corpus test`.
pub fn test(strict: bool) -> Result<(), String> {
    let root = repo_root();
    let entries = read_lock(&root)?;
    test::run(&root, &entries, strict)
}

fn read_lock(root: &Path) -> Result<Vec<CorpusEntry>, String> {
    let lock_path = root.join("corpus.lock");
    let text = std::fs::read_to_string(&lock_path)
        .map_err(|e| format!("reading {}: {e}", lock_path.display()))?;
    lock::parse(&text)
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask/ has a parent")
        .to_path_buf()
}
