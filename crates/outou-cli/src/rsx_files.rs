//! Finds every `.rsx` file under a crate's `src/` directory, shared by
//! `outou check` and `outou fmt` so the two commands never drift on what
//! counts as a source file (a directory whose name starts with `.` is
//! always skipped, so `.generated/` is never scanned by either).
//!
//! `TODO(phase0)`: a *file* that is itself a symlink (as opposed to the
//! symlinked-directory loop [`collect_rsx_files`] already guards
//! against) is collected like any other `.rsx` file — `outou fmt`, which
//! writes through [`std::fs::write`], therefore writes through the
//! symlink to whatever it points at, which is ordinary Unix `write`
//! semantics but worth calling out since a formatter rewriting a file
//! the user did not expect to be shared (e.g. a symlink into a vendored
//! or generated tree) could surprise them.

use std::path::{Path, PathBuf};

/// Finds every `.rsx` file under `dir`, recursively, in a deterministic
/// (sorted) order. Returns an empty list, not an error, when `dir` itself
/// does not exist — a crate with no `.rsx` sources is not a failure.
///
/// Two hardenings (MEDIUM-11, issue #6 fix list item 11, carried over
/// from `outou check`):
///
/// - `entry.file_type()` is used instead of `Path::is_dir()`, which
///   follows symlinks: a self-referential directory symlink
///   (`src/loop -> src`) would otherwise make this walk
///   `src/loop/loop/loop/…` until the OS path limit.
/// - A directory whose name starts with `.` is skipped, so
///   `.generated/` (never user-authored `.rsx` sources) is never
///   scanned.
pub(crate) fn collect_rsx_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !dir.exists() {
        return Ok(files);
    }

    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let path = entry.path();
            if file_type.is_dir() {
                if is_dot_directory(&path) {
                    continue;
                }
                stack.push(path);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rsx") {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Whether `path`'s file name starts with `.` (`.generated`, `.git`, …).
fn is_dot_directory(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_src_directory_is_not_an_error() {
        let files = collect_rsx_files(Path::new("this/does/not/exist")).expect("no error");
        assert!(files.is_empty());
    }

    #[test]
    fn finds_rsx_files_recursively_in_sorted_order() {
        let dir = std::env::temp_dir().join(format!(
            "outou-cli-rsx-files-test-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        std::fs::write(dir.join("b.rsx"), "").unwrap();
        std::fs::write(dir.join("nested/a.rsx"), "").unwrap();
        std::fs::write(dir.join("ignored.rs"), "").unwrap();

        let files = collect_rsx_files(&dir).expect("directory exists");
        let names: Vec<_> = files
            .iter()
            .map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().into_owned())
            .collect();

        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(names, vec!["b.rsx".to_string(), "nested/a.rsx".to_string()]);
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_a_directory_symlink_loop() {
        // MEDIUM-11, issue #6 fix list item 11: `path.is_dir()` follows
        // symlinks, so a self-referential symlink (`src/loop -> src`)
        // would otherwise walk `src/loop/loop/loop/…` until the OS path
        // limit. `entry.file_type()` does not follow symlinks, so the
        // loop is never even entered.
        let dir = std::env::temp_dir().join(format!(
            "outou-cli-rsx-files-test-symlink-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("keep.rsx"), "").unwrap();
        std::os::unix::fs::symlink(&dir, dir.join("loop")).unwrap();

        let files = collect_rsx_files(&dir).expect("directory exists, no infinite recursion");
        let names: Vec<_> = files
            .iter()
            .map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().into_owned())
            .collect();

        std::fs::remove_file(dir.join("loop")).ok();
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(names, vec!["keep.rsx".to_string()]);
    }

    #[test]
    fn skips_dot_directories_including_generated() {
        let dir = std::env::temp_dir().join(format!(
            "outou-cli-rsx-files-test-dotdir-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(dir.join(".generated")).unwrap();
        std::fs::write(dir.join(".generated/broken.rsx"), "").unwrap();
        std::fs::write(dir.join("keep.rsx"), "").unwrap();

        let files = collect_rsx_files(&dir).expect("directory exists");
        let names: Vec<_> = files
            .iter()
            .map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().into_owned())
            .collect();

        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(names, vec!["keep.rsx".to_string()]);
    }
}
