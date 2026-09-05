//! Removes stale files from `src/.generated/` that this tool manages but
//! did not just (re)write: a module that was renamed or deleted leaves
//! its old generated `.rs` and `.rs.map.json` behind otherwise. Also
//! removes a leftover `.<name>.outou-tmp-<pid>` file (`emit::atomic_write`'s
//! temporary-file naming) that a crashed run never got to rename into
//! place (N4, issue #8 fix list step 5).
//!
//! Only files this tool manages are ever touched: a file whose name ends
//! in `.rs` or `.rs.map.json` (not any `*.map.json` — narrower than
//! before, MEDIUM-6, so a foreign `notes.map.json` is never mistaken for
//! this tool's own sidecar), plus the temp-file pattern above. Anything
//! else under `src/.generated/` (a stray `notes.txt`, say) is left alone
//! — `outou build` does not own that directory exclusively, only the
//! files it produces.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Walks `generated_dir` recursively and removes every managed file
/// (`*.rs`, `*.rs.map.json`) not present in `produced`. Returns the paths
/// removed, in an unspecified order. A missing `generated_dir` is not an
/// error (a crate with nothing generated yet has nothing to clean).
pub fn clean_stale(
    generated_dir: &Path,
    produced: &HashSet<PathBuf>,
) -> std::io::Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    if !generated_dir.exists() {
        return Ok(removed);
    }

    let mut stack = vec![generated_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let path = entry.path();
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if !is_managed_file(&path) {
                continue;
            }
            if produced.contains(&path) {
                continue;
            }
            fs::remove_file(&path)?;
            removed.push(path);
        }
    }
    Ok(removed)
}

/// Whether `path`'s file name is one `outou build` would have produced or
/// left behind: a generated Rust file (`*.rs`), its source-map sidecar
/// (`*.rs.map.json`), or a crashed run's leftover atomic-write temp file
/// (`.<name>.outou-tmp-<pid>`, `emit::atomic_write`'s exact naming).
fn is_managed_file(path: &Path) -> bool {
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(name) => name,
        None => return false,
    };
    name.ends_with(".rs.map.json") || name.ends_with(".rs") || is_leftover_temp_file(name)
}

/// Whether `name` matches `emit::atomic_write`'s temporary-file pattern,
/// `.<original-name>.outou-tmp-<pid>` — a leftover from a run that
/// crashed between writing the temp file and renaming it into place.
fn is_leftover_temp_file(name: &str) -> bool {
    name.starts_with('.') && name.contains(".outou-tmp-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_stale_managed_files_and_keeps_everything_else() {
        let dir = std::env::temp_dir().join(format!(
            "outou-cli-clean-test-{}-{}",
            std::process::id(),
            line!()
        ));
        fs::create_dir_all(&dir).unwrap();
        let kept = dir.join("crate-root.rs");
        let kept_map = dir.join("crate-root.rs.map.json");
        let stale = dir.join("stale.rs");
        let notes = dir.join("notes.txt");
        fs::write(&kept, "").unwrap();
        fs::write(&kept_map, "").unwrap();
        fs::write(&stale, "").unwrap();
        fs::write(&notes, "").unwrap();

        let mut produced = HashSet::new();
        produced.insert(kept.clone());
        produced.insert(kept_map.clone());

        let removed = clean_stale(&dir, &produced).expect("cleaning succeeds");

        assert_eq!(removed, vec![stale.clone()]);
        assert!(kept.exists());
        assert!(kept_map.exists());
        assert!(!stale.exists());
        assert!(notes.exists());

        fs::remove_dir_all(&dir).ok();
    }

    /// MEDIUM-6 / N4 (issue #8 fix list step 5): `is_managed_file` matched
    /// any `*.map.json`, broader than the module's own documented
    /// `*.rs.map.json` — a foreign `notes.map.json` was silently deleted.
    /// A crashed run's leftover `.name.outou-tmp-<pid>` file also matched
    /// neither suffix and was never cleaned up.
    #[test]
    fn narrows_to_rs_map_json_and_also_removes_leftover_temp_files() {
        let dir = std::env::temp_dir().join(format!(
            "outou-cli-clean-test-narrow-{}-{}",
            std::process::id(),
            line!()
        ));
        fs::create_dir_all(&dir).unwrap();
        let foreign_map_json = dir.join("notes.map.json");
        let leftover_temp = dir.join(".crate-root.rs.outou-tmp-999");
        fs::write(&foreign_map_json, "not ours").unwrap();
        fs::write(&leftover_temp, "half-written").unwrap();

        let removed = clean_stale(&dir, &HashSet::new()).expect("cleaning succeeds");

        assert!(
            foreign_map_json.exists(),
            "a `*.map.json` that isn't `*.rs.map.json` must survive"
        );
        assert!(
            !leftover_temp.exists(),
            "a leftover `.name.outou-tmp-<pid>` file must be removed"
        );
        assert!(removed.contains(&leftover_temp));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_generated_dir_is_not_an_error() {
        let removed = clean_stale(Path::new("this/does/not/exist"), &HashSet::new())
            .expect("missing directory is not an error");
        assert!(removed.is_empty());
    }
}
