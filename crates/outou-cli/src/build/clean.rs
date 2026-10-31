//! Removes stale files from `src/.generated/` that this tool manages but
//! did not just (re)write: a module that was renamed or deleted leaves
//! its old generated `.rs` and `.rs.map.json` behind otherwise. Also
//! removes a leftover `.<name>.outou-tmp-<pid>-<n>` file
//! (`atomic_write::write_atomic`'s temporary-file naming) that a crashed run never got to rename into
//! place (N4, issue #8 fix list step 5).
//!
//! Only files this tool manages are ever touched: a file whose name ends
//! in `.rs` or `.rs.map.json` (not any `*.map.json` — narrower than
//! before, MEDIUM-6, so a foreign `notes.map.json` is never mistaken for
//! this tool's own sidecar), plus the temp-file pattern above. Anything
//! else under `src/.generated/` (a stray `notes.txt`, say) is left alone
//! — `outou build` does not own that directory exclusively, only the
//! files it produces.
//!
//! On a case-insensitive filesystem (probed with a `.outou-case-probe.outou-tmp-<pid>-0`
//! file, which follows the temp shape above) names are compared
//! case-folded, and a kept file whose on-disk spelling differs from the
//! produced one is renamed to the produced spelling.

use std::collections::{HashMap, HashSet};
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
    if !generated_dir.exists() {
        return Ok(Vec::new());
    }
    clean_stale_with_case(
        generated_dir,
        produced,
        fs_is_case_insensitive(generated_dir),
    )
}

/// [`clean_stale`] with the filesystem's case folding given rather than
/// probed. When `case_insensitive`, a produced path and an on-disk path
/// that differ only in case are the same file (a unit renamed from
/// `Foo.rsx` to `foo.rsx` writes `foo.rs` over the existing `Foo.rs`), so
/// the on-disk file is kept and respelled to the produced spelling.
///
/// TODO(alpha) (#31): Unicode normalisation is not covered. APFS is also
/// normalisation-insensitive (NFC and NFD `café.rs` are one file), so a
/// non-ASCII module name can still hit the original deletion.
fn clean_stale_with_case(
    generated_dir: &Path,
    produced: &HashSet<PathBuf>,
    case_insensitive: bool,
) -> std::io::Result<Vec<PathBuf>> {
    let kept: HashMap<PathBuf, &PathBuf> = produced
        .iter()
        .map(|path| (comparison_key(path, case_insensitive), path))
        .collect();
    let mut removed = Vec::new();
    for path in walk_managed_files(generated_dir)? {
        if let Some(produced_path) = kept.get(&comparison_key(&path, case_insensitive)) {
            if case_insensitive && path.file_name() != produced_path.file_name() {
                respell(&path, produced_path)?;
            }
            continue;
        }
        match fs::remove_file(&path) {
            Ok(()) => removed.push(path),
            // Already gone: another build's probe or temp file.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }
    Ok(removed)
}

/// Gives the kept file `on_disk` the directory-entry spelling of
/// `produced`. A rename over a case-insensitive filesystem's existing
/// entry keeps the old spelling, so the file hops through a unique
/// temporary name: `on_disk` -> temp -> `produced`. Without this the
/// entry stays `Foo.rs` while the plan says `foo.rs`, and
/// `outou package --check` (an exact comparison) reports it as stale.
fn respell(on_disk: &Path, produced: &Path) -> std::io::Result<()> {
    let name = on_disk
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = on_disk.with_file_name(format!(".{name}.outou-tmp-{}-respell", std::process::id()));
    fs::rename(on_disk, &temp)?;
    fs::rename(&temp, produced)
}

/// The path as compared against the kept set: unchanged on a case-sensitive
/// filesystem, lower-cased on a case-insensitive one.
fn comparison_key(path: &Path, case_insensitive: bool) -> PathBuf {
    if case_insensitive {
        PathBuf::from(path.to_string_lossy().to_lowercase())
    } else {
        path.to_path_buf()
    }
}

/// Whether the filesystem holding `dir` treats names that differ only in
/// case as the same file (macOS and Windows defaults). Probed, not assumed:
/// a temporary file is created with a mixed-case name and looked up under
/// its upper-case spelling. Named like a leftover atomic-write temp file,
/// so a crash between create and remove is cleaned up by the next run. Any
/// I/O failure answers `false`, which keeps the exact (case-sensitive)
/// comparison.
pub(crate) fn fs_is_case_insensitive(dir: &Path) -> bool {
    let probe = dir.join(format!(
        ".outou-case-probe.outou-tmp-{}-0",
        std::process::id()
    ));
    let upper = dir.join(format!(
        ".OUTOU-CASE-PROBE.OUTOU-TMP-{}-0",
        std::process::id()
    ));
    if fs::write(&probe, b"").is_err() {
        return false;
    }
    let folds = upper.exists();
    // Best effort: a leftover is swept as a temp file by the next build.
    fs::remove_file(&probe).ok();
    folds
}

/// Every managed file (`*.rs`, `*.rs.map.json`, or a leftover atomic-write
/// temp file — see [`is_managed_file`]) under `generated_dir`, walked
/// recursively, in an unspecified order. Read-only: shared by
/// [`clean_stale`] (which then removes the ones not in `produced`) and
/// `crate::package`'s in-memory drift check (which only reports stale
/// files, never deletes them), so neither reimplements the walk. A
/// missing `generated_dir` is not an error — a crate with nothing
/// generated yet has nothing to walk.
pub(crate) fn walk_managed_files(generated_dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !generated_dir.exists() {
        return Ok(files);
    }

    let mut stack = vec![generated_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                stack.push(path);
                continue;
            }
            if is_managed_file(&path) {
                files.push(path);
            }
        }
    }
    Ok(files)
}

/// Whether `path`'s file name is one `outou build` would have produced or
/// left behind: a generated Rust file (`*.rs`), its source-map sidecar
/// (`*.rs.map.json`), or a crashed run's leftover atomic-write temp file
/// (`.<name>.outou-tmp-<pid>-<n>`, `atomic_write::write_atomic`'s exact
/// naming).
pub(crate) fn is_managed_file(path: &Path) -> bool {
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(name) => name,
        None => return false,
    };
    name.ends_with(".rs.map.json") || name.ends_with(".rs") || is_leftover_temp_file(name)
}

/// Whether `name` matches `atomic_write::write_atomic`'s temporary-file
/// pattern, `.<original-name>.outou-tmp-<pid>-<n>` — a leftover from a run that
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
    /// A crashed run's leftover `.name.outou-tmp-<pid>-<n>` file also matched
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
            "a leftover `.name.outou-tmp-<pid>-<n>` file must be removed"
        );
        assert!(removed.contains(&leftover_temp));

        fs::remove_dir_all(&dir).ok();
    }

    /// Issue #20 A1 (f): on a case-insensitive filesystem a produced file
    /// that differs from the on-disk spelling only in case is the same
    /// file and must survive the clean.
    #[test]
    fn keeps_a_produced_file_whose_name_differs_only_in_case() {
        let dir = std::env::temp_dir().join(format!(
            "outou-cli-clean-test-case-{}-{}",
            std::process::id(),
            line!()
        ));
        fs::create_dir_all(&dir).unwrap();
        if !fs_is_case_insensitive(&dir) {
            eprintln!("skipped: the temp directory's filesystem is case-sensitive");
            fs::remove_dir_all(&dir).ok();
            return;
        }
        // A previous build left `Foo.rs`; the rename to `foo.rsx` writes
        // `foo.rs`, which on this filesystem is the same file.
        fs::write(dir.join("Foo.rs"), "old").unwrap();
        fs::write(dir.join("foo.rs"), "new").unwrap();
        let produced = HashSet::from([dir.join("foo.rs")]);

        let removed = clean_stale(&dir, &produced).expect("cleaning succeeds");

        assert!(removed.is_empty(), "removed {removed:?}");
        assert_eq!(fs::read_to_string(dir.join("foo.rs")).unwrap(), "new");

        fs::remove_dir_all(&dir).ok();
    }

    /// Platform-independent: the case-folding flag is injected, and the
    /// produced path need not exist on disk.
    #[test]
    fn case_folded_comparison_applies_only_when_the_filesystem_folds_case() {
        let dir = std::env::temp_dir().join(format!(
            "outou-cli-clean-test-fold-{}-{}",
            std::process::id(),
            line!()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Foo.rs"), "").unwrap();
        let produced = HashSet::from([dir.join("foo.rs")]);
        let removed = clean_stale_with_case(&dir, &produced, true).unwrap();
        assert!(removed.is_empty(), "removed {removed:?}");
        assert_eq!(entry_names(&dir), vec!["foo.rs"]);

        let other = dir.join("Bar.rs");
        fs::write(&other, "").unwrap();
        let produced = HashSet::from([dir.join("foo.rs"), dir.join("bar.rs")]);
        let removed = clean_stale_with_case(&dir, &produced, false).unwrap();
        assert_eq!(removed, vec![other.clone()]);
        assert!(!other.exists());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn probe_reports_the_filesystems_case_sensitivity_consistently() {
        let dir = std::env::temp_dir().join(format!(
            "outou-cli-clean-test-probe-{}-{}",
            std::process::id(),
            line!()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Probe.rs"), "").unwrap();
        let folds = dir.join("pROBE.rs").exists();
        assert_eq!(fs_is_case_insensitive(&dir), folds);
        assert!(
            fs::read_dir(&dir).unwrap().count() == 1,
            "the probe must leave no file behind"
        );
        fs::remove_dir_all(&dir).ok();
    }

    fn entry_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// Platform-independent: a kept file whose on-disk spelling differs in
    /// case is respelled to the produced spelling (and so is its map).
    #[test]
    fn respells_a_kept_file_to_the_produced_spelling() {
        let dir = std::env::temp_dir().join(format!(
            "outou-cli-clean-test-respell-{}-{}",
            std::process::id(),
            line!()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Foo.rs"), "new").unwrap();
        fs::write(dir.join("Foo.rs.map.json"), "map").unwrap();
        let produced = HashSet::from([dir.join("foo.rs"), dir.join("foo.rs.map.json")]);

        let removed = clean_stale_with_case(&dir, &produced, true).unwrap();

        assert!(removed.is_empty(), "removed {removed:?}");
        assert_eq!(entry_names(&dir), vec!["foo.rs", "foo.rs.map.json"]);
        assert_eq!(fs::read_to_string(dir.join("foo.rs")).unwrap(), "new");

        fs::remove_dir_all(&dir).ok();
    }

    /// Real filesystem: after the clean, the directory entry name (not just
    /// a case-folded lookup) is the produced spelling, so
    /// `outou package --check`'s exact comparison sees no stale file.
    #[test]
    fn real_filesystem_entry_name_matches_the_produced_spelling() {
        let dir = std::env::temp_dir().join(format!(
            "outou-cli-clean-test-entry-{}-{}",
            std::process::id(),
            line!()
        ));
        fs::create_dir_all(&dir).unwrap();
        if !fs_is_case_insensitive(&dir) {
            eprintln!("skipped: the temp directory's filesystem is case-sensitive");
            fs::remove_dir_all(&dir).ok();
            return;
        }
        fs::write(dir.join("Foo.rs"), "old").unwrap();
        fs::write(dir.join("Foo.rs.map.json"), "old").unwrap();
        // The build's atomic write leaves the old spelling with new content.
        fs::write(dir.join("foo.rs"), "new").unwrap();
        fs::write(dir.join("foo.rs.map.json"), "new").unwrap();
        let produced = HashSet::from([dir.join("foo.rs"), dir.join("foo.rs.map.json")]);

        let removed = clean_stale(&dir, &produced).expect("cleaning succeeds");

        assert!(removed.is_empty(), "removed {removed:?}");
        assert_eq!(entry_names(&dir), vec!["foo.rs", "foo.rs.map.json"]);
        assert_eq!(fs::read_to_string(dir.join("foo.rs")).unwrap(), "new");
        let stale: Vec<PathBuf> = walk_managed_files(&dir)
            .unwrap()
            .into_iter()
            .filter(|path| !produced.contains(path))
            .collect();
        assert!(stale.is_empty(), "stale {stale:?}");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn probe_file_follows_the_documented_temp_shape() {
        let probe = format!(".outou-case-probe.outou-tmp-{}-0", std::process::id());
        assert!(is_leftover_temp_file(&probe));
    }

    #[test]
    fn missing_generated_dir_is_not_an_error() {
        let removed = clean_stale(Path::new("this/does/not/exist"), &HashSet::new())
            .expect("missing directory is not an error");
        assert!(removed.is_empty());
    }
}
