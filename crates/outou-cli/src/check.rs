//! `outou check`: parses every `.rsx` file under the current crate's
//! `src/` directory and prints Outou's own rendered diagnostics for it
//! (never backend vocabulary — `outou-syntax` diagnoses JSX structure
//! itself, in Outou vocabulary, at the `.rsx` position).
//!
//! This uses the same front end (`outou_syntax::parse`) `cargo build`'s
//! generation step and the language server both use — there is one
//! compiler, only the mode differs (`AGENTS.md`).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use outou_syntax::Severity;

/// Runs `outou check`. Exit code `1` if any file has an error diagnostic,
/// `0` otherwise (a file with only warnings still exits `0`).
pub fn run() -> ExitCode {
    let src_dir = Path::new("src");
    let files = match collect_rsx_files(src_dir) {
        Ok(files) => files,
        Err(err) => {
            eprintln!("error: reading {}: {err}", src_dir.display());
            return ExitCode::FAILURE;
        }
    };

    let mut had_errors = false;
    for path in &files {
        let source = match std::fs::read_to_string(path) {
            Ok(source) => source,
            Err(err) => {
                eprintln!("error: reading {}: {err}", path.display());
                had_errors = true;
                continue;
            }
        };

        let parsed = outou_syntax::parse(&source);
        if parsed
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
        {
            had_errors = true;
        }
        if !parsed.diagnostics.is_empty() {
            println!("{}", parsed.render_diagnostics(&path.display().to_string()));
        }
    }

    if had_errors {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Finds every `.rsx` file under `dir`, recursively, in a deterministic
/// (sorted) order. Returns an empty list, not an error, when `dir` itself
/// does not exist — a crate with no `src/.rsx` files is not a failure.
///
/// Two hardenings (MEDIUM-11, issue #6 fix list item 11):
///
/// - `entry.file_type()` is used instead of `Path::is_dir()`, which
///   follows symlinks: a self-referential directory symlink
///   (`src/loop -> src`) previously made this walk `src/loop/loop/loop/…`
///   until the OS path limit, re-reporting the same diagnostics dozens of
///   times. `file_type()` reports a symlink as a symlink, so it is
///   neither recursed into nor mistaken for a `.rsx` file.
/// - A directory whose name starts with `.` is skipped, so `.generated/`
///   (never user-authored `.rsx` sources) is never scanned.
fn collect_rsx_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
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
            "outou-cli-check-test-{}-{}",
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

    #[test]
    fn skips_dot_directories_including_generated() {
        // MEDIUM-11, issue #6 fix list item 11: `outou check` must not
        // descend into `src/.generated/` (or any other dot-directory) —
        // generated files are not user-authored `.rsx` sources.
        let dir = std::env::temp_dir().join(format!(
            "outou-cli-check-test-dotdir-{}-{}",
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

    #[cfg(unix)]
    #[test]
    fn does_not_follow_a_directory_symlink_loop() {
        // MEDIUM-11, issue #6 fix list item 11: `path.is_dir()` follows
        // symlinks, so a self-referential symlink (`src/loop -> src`)
        // made `outou check` walk `src/loop/loop/loop/…` until the OS
        // path limit, re-reporting the same diagnostics dozens of times.
        // `entry.file_type()` does not follow symlinks, so the loop is
        // never even entered.
        let dir = std::env::temp_dir().join(format!(
            "outou-cli-check-test-symlink-{}-{}",
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
}
