//! Recursive `*.rs` file discovery under a corpus checkout.

use std::fs;
use std::path::{Path, PathBuf};

/// Every `*.rs` file under `dir`, recursively, in a deterministic
/// (sorted) order. An absent or unreadable directory yields an empty
/// list rather than an error: `corpus test` reports that separately, with
/// a message that names the missing directory.
pub fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn finds_rs_files_recursively_and_sorted() {
        let dir = std::env::temp_dir().join(format!(
            "outou-xtask-walk-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("b.rs"), "").unwrap();
        fs::write(dir.join("sub/a.rs"), "").unwrap();
        fs::write(dir.join("ignore.txt"), "").unwrap();

        let mut found = collect_rs_files(&dir);
        found.sort();
        assert_eq!(found, vec![dir.join("b.rs"), dir.join("sub/a.rs")]);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_directory_yields_empty_list() {
        let missing = std::env::temp_dir().join("outou-xtask-walk-test-missing-dir-xyz");
        assert!(collect_rs_files(&missing).is_empty());
    }
}
