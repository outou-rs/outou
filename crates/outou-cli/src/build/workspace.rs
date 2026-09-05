//! Minimal `[workspace] members` reading, so `outou build --manifest-dir
//! <workspace root>` builds every member with an `.rsx` crate root in one
//! pass (issue #10 deliverable 3), instead of requiring one invocation
//! per crate.
//!
//! **`TODO(phase0)`:** globbing is limited to a single trailing `/*`
//! segment on one path component (`"crates/*"`), the only shape this
//! repository's own fixtures need. A full glob (`"crates/**"`, bracket
//! patterns, …) is out of scope for Phase 0; `cargo_metadata` was
//! deliberately not pulled in for this (`crates/outou-cli/README.md`),
//! so an unsupported pattern is silently treated as a literal path
//! rather than expanded.

use std::fs;
use std::path::{Path, PathBuf};

/// Errors from [`workspace_members`].
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    /// `Cargo.toml` exists but is not valid TOML.
    #[error("parsing `{}`: {source}", path.display())]
    Parse {
        /// The manifest that failed to parse.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: toml::de::Error,
    },
    /// A `/*` member pattern's own directory could not be listed.
    #[error("listing workspace members under `{}`: {source}", path.display())]
    ListMembers {
        /// The directory that could not be read.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
}

/// Reads `manifest_dir/Cargo.toml` and returns every `[workspace]
/// members` entry, resolved to an absolute directory, in declaration
/// order (a `/*` pattern's own matches are sorted for determinism).
///
/// Returns `Ok(None)` when `manifest_dir/Cargo.toml` does not exist, is
/// unreadable, has a `[package]` table, or has no `[workspace]` table at
/// all — an ordinary crate manifest, which [`super::build`] already knows
/// how to build directly.
///
/// A manifest may carry both `[package]` and an empty `[workspace]`
/// (`examples/phase0-app`'s own Cargo.toml does exactly this, to opt out
/// of the repository's own workspace): `[package]`'s presence always
/// wins, so that shape still builds as the single crate it is, rather
/// than as a workspace with zero members.
pub fn workspace_members(manifest_dir: &Path) -> Result<Option<Vec<PathBuf>>, WorkspaceError> {
    let manifest_path = manifest_dir.join("Cargo.toml");
    let Ok(text) = fs::read_to_string(&manifest_path) else {
        return Ok(None);
    };
    let value: toml::Value = toml::from_str(&text).map_err(|source| WorkspaceError::Parse {
        path: manifest_path.clone(),
        source,
    })?;
    if value.get("package").is_some() {
        return Ok(None);
    }
    let Some(workspace) = value.get("workspace") else {
        return Ok(None);
    };

    let patterns = workspace
        .get("members")
        .and_then(|members| members.as_array())
        .map(|array| {
            array
                .iter()
                .filter_map(|value| value.as_str())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut members = Vec::new();
    for pattern in patterns {
        match pattern.strip_suffix("/*") {
            Some(prefix) => members.extend(glob_star(manifest_dir, prefix)?),
            None => members.push(manifest_dir.join(pattern)),
        }
    }
    Ok(Some(members))
}

/// Every immediate subdirectory of `manifest_dir/prefix` that itself
/// contains a `Cargo.toml`, sorted by name for determinism.
fn glob_star(manifest_dir: &Path, prefix: &str) -> Result<Vec<PathBuf>, WorkspaceError> {
    let base = manifest_dir.join(prefix);
    let mut found: Vec<PathBuf> = fs::read_dir(&base)
        .map_err(|source| WorkspaceError::ListMembers {
            path: base.clone(),
            source,
        })?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.join("Cargo.toml").is_file())
        .collect();
    found.sort();
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, relative: &str, contents: &str) {
        let path = dir.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "outou-cli-workspace-test-{label}-{}-{}",
            std::process::id(),
            line!()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_plain_package_manifest_has_no_members() {
        let dir = temp_dir("plain");
        write(&dir, "Cargo.toml", "[package]\nname = \"x\"\n");

        let members = workspace_members(&dir).expect("reading succeeds");

        fs::remove_dir_all(&dir).ok();
        assert_eq!(members, None);
    }

    /// `examples/phase0-app`'s own Cargo.toml shape: `[package]` plus an
    /// *empty* `[workspace]` table, only to opt the crate out of the
    /// repository's own workspace. `[package]`'s presence must still win,
    /// or `build_workspace` would treat this as a workspace with zero
    /// members and silently build nothing (the exact regression this
    /// guards: `outou build --manifest-dir examples/phase0-app` printing
    /// "nothing to do" instead of generating `crate-root.rs`).
    #[test]
    fn a_package_with_its_own_empty_workspace_table_has_no_members() {
        let dir = temp_dir("package-plus-workspace");
        write(
            &dir,
            "Cargo.toml",
            "[package]\nname = \"x\"\n\n[workspace]\n",
        );

        let members = workspace_members(&dir).expect("reading succeeds");

        fs::remove_dir_all(&dir).ok();
        assert_eq!(members, None);
    }

    #[test]
    fn a_missing_manifest_has_no_members() {
        let dir = temp_dir("missing");

        let members = workspace_members(&dir).expect("reading succeeds");

        fs::remove_dir_all(&dir).ok();
        assert_eq!(members, None);
    }

    #[test]
    fn explicit_and_globbed_members_are_both_resolved() {
        let dir = temp_dir("members");
        write(
            &dir,
            "Cargo.toml",
            "[workspace]\nmembers = [\"app\", \"crates/*\"]\n",
        );
        write(&dir, "app/Cargo.toml", "[package]\nname = \"app\"\n");
        write(&dir, "crates/a/Cargo.toml", "[package]\nname = \"a\"\n");
        write(&dir, "crates/b/Cargo.toml", "[package]\nname = \"b\"\n");
        // A directory with no `Cargo.toml` is not a member.
        fs::create_dir_all(dir.join("crates/not-a-crate")).unwrap();

        let members = workspace_members(&dir)
            .expect("reading succeeds")
            .expect("this manifest has a [workspace] table");

        fs::remove_dir_all(&dir).ok();
        assert_eq!(
            members,
            vec![dir.join("app"), dir.join("crates/a"), dir.join("crates/b"),]
        );
    }
}
