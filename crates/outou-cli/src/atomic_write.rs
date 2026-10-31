//! Crash-safe file replacement shared by `outou build` and `outou fmt`.
//!
//! [`write_atomic`] writes the new contents to a temporary file in the same
//! directory as the *resolved* target and renames it over the target, so a
//! reader never observes a partial write and a failure part-way through
//! (a full disk, `ulimit -f`) leaves the previous contents intact.
//!
//! Durability (fsync before the rename) is opt-in via [`Durability`]:
//! `outou fmt` rewrites hand-written sources and syncs; `outou build` emits
//! rebuildable output on every save and does not.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Whether [`write_atomic`] fsyncs the temporary file before renaming it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Durability {
    /// `fsync` the data before the rename, so the new contents survive a
    /// power loss right after the call returns (costs ~5 ms on macOS).
    Sync,
    /// Skip the `fsync`: the rename is still atomic, but after a power
    /// loss the file may be empty or old. For rebuildable output only.
    NoSync,
}

/// Replaces the file at `path` with `contents` atomically.
///
/// A symbolic link at `path` is followed: the temporary file is created next
/// to the link's real target and renamed onto it, so the link itself stays a
/// link. A dangling symbolic link is not followed: it is replaced by a
/// regular file. The permissions of an existing target are kept; on unix the
/// temporary file is created `0o600` and gets those permissions before any
/// contents are written, so a private original is never exposed. A read-only
/// existing target is refused with `PermissionDenied` before anything is
/// created.
///
/// The temporary file (`.<name>.outou-tmp-<pid>-<n>`) is removed when an I/O
/// error occurs, but a killed process (for example SIGXFSZ under
/// `ulimit -f`) may leave it behind; `outou build` cleans those up.
pub(crate) fn write_atomic(
    path: &Path,
    contents: &[u8],
    durability: Durability,
) -> std::io::Result<()> {
    // A missing target is created at `path` itself; an existing one (or a
    // link to one) is resolved so the rename lands on the real file.
    let target = match fs::canonicalize(path) {
        Ok(real) => real,
        Err(err) if err.kind() == ErrorKind::NotFound => path.to_path_buf(),
        Err(err) => return Err(err),
    };
    let original = fs::metadata(&target).ok();
    if let Some(metadata) = &original {
        if metadata.permissions().readonly() {
            return Err(std::io::Error::new(
                ErrorKind::PermissionDenied,
                format!("{} is read-only", target.display()),
            ));
        }
    }
    let tmp_path = temp_path_beside(&target);

    let result =
        write_temp_then_rename(&target, &tmp_path, contents, original.as_ref(), durability);
    if result.is_err() {
        // Best effort: the original error is the one worth reporting.
        let _ = fs::remove_file(&tmp_path);
    }
    result
}

fn temp_path_beside(target: &Path) -> PathBuf {
    let dir = target.parent().unwrap_or_else(|| Path::new("."));
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let unique = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    dir.join(format!(".{name}.outou-tmp-{}-{unique}", std::process::id()))
}

fn write_temp_then_rename(
    target: &Path,
    tmp_path: &Path,
    contents: &[u8],
    original: Option<&fs::Metadata>,
    durability: Durability,
) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    // An existing file's permissions are copied below; until then keep the
    // temp file private. A new file keeps the default (umask) mode.
    #[cfg(unix)]
    if original.is_some() {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(tmp_path)?;
    if let Some(metadata) = original {
        file.set_permissions(metadata.permissions())?;
    }
    file.write_all(contents)?;
    if durability == Durability::Sync {
        file.sync_all()?;
    }
    drop(file);
    fs::rename(tmp_path, target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("outou-cli-atomic-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn entry_count(dir: &Path) -> usize {
        fs::read_dir(dir).unwrap().count()
    }

    #[test]
    fn replaces_contents_and_leaves_no_temp_file() {
        let dir = scratch("plain");
        let target = dir.join("a.rsx");
        fs::write(&target, "old").unwrap();

        write_atomic(&target, b"new", Durability::Sync).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
        assert_eq!(entry_count(&dir), 1);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn creates_a_missing_target() {
        let dir = scratch("missing");
        let target = dir.join("new.rs");

        write_atomic(&target, b"x", Durability::NoSync).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "x");
        assert_eq!(entry_count(&dir), 1);
        fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_stays_a_symlink_and_its_target_is_updated() {
        let dir = scratch("symlink");
        let real_dir = dir.join("real");
        fs::create_dir_all(&real_dir).unwrap();
        let real = real_dir.join("real.rsx");
        fs::write(&real, "old").unwrap();
        let link = dir.join("link.rsx");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        write_atomic(&link, b"new", Durability::Sync).unwrap();

        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_to_string(&real).unwrap(), "new");
        assert_eq!(entry_count(&real_dir), 1, "no temp file next to the target");
        assert_eq!(entry_count(&dir), 2, "no temp file next to the link");
        fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn permissions_of_the_original_are_preserved() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("perms");
        let target = dir.join("a.rsx");
        fs::write(&target, "old").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o754)).unwrap();

        write_atomic(&target, b"new", Durability::Sync).unwrap();

        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o754);
        fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_read_only_target_is_refused_and_left_unchanged() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("readonly-target");
        let target = dir.join("a.rsx");
        fs::write(&target, "old").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o444)).unwrap();
        // Root can write read-only files; the refusal is still expected from
        // us, but skip the assertions to stay robust across environments.
        let is_root = fs::OpenOptions::new().write(true).open(&target).is_ok();

        let result = write_atomic(&target, b"new", Durability::Sync);

        if !is_root {
            let err = result.unwrap_err();
            assert_eq!(err.kind(), ErrorKind::PermissionDenied);
            assert_eq!(fs::read_to_string(&target).unwrap(), "old");
            assert_eq!(entry_count(&dir), 1, "no temp file created");
        }
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
        fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_private_original_stays_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("private");
        let target = dir.join("a.rsx");
        fs::write(&target, "old").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();

        write_atomic(&target, b"new", Durability::NoSync).unwrap();

        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_failed_rename_removes_the_temp_file_and_keeps_the_original() {
        // Renaming a file over a non-empty directory fails on every
        // platform and as any user, after the temporary file was written.
        let dir = scratch("rename-fails");
        let target = dir.join("target");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("keep"), "keep").unwrap();

        let result = write_atomic(&target, b"new", Durability::Sync);

        assert!(result.is_err());
        assert_eq!(fs::read_to_string(target.join("keep")).unwrap(), "keep");
        assert_eq!(entry_count(&dir), 1, "temp file must be removed");
        fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_write_in_a_read_only_directory_leaves_the_original_untouched() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("read-only");
        let target = dir.join("a.rsx");
        fs::write(&target, "old").unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
        // Root ignores directory permissions; the test cannot force a failure.
        let can_still_create = fs::write(dir.join("probe"), "").is_ok();

        let result = write_atomic(&target, b"new", Durability::Sync);

        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        if !can_still_create {
            assert!(result.is_err());
            assert_eq!(fs::read_to_string(&target).unwrap(), "old");
            assert_eq!(entry_count(&dir), 1, "no temp file left behind");
        }
        fs::remove_dir_all(&dir).ok();
    }
}
