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
    /// A `[workspace]` table resolved to zero members after `exclude` and
    /// deduplication (issue #12 corpus review, F10/MEDIUM). `outou build`
    /// used to build nothing at all for this shape and print the
    /// misleading "no `.rsx` crate root found; nothing to do" — the same
    /// message a plain, non-workspace crate with no `.rsx` root gets —
    /// rather than naming the actual problem.
    #[error(
        "workspace `{}` has no members (after `exclude`, if any); nothing to build",
        path.display()
    )]
    NoMembers {
        /// The workspace root manifest directory.
        path: PathBuf,
    },
}

/// Reads `manifest_dir/Cargo.toml` and returns every `[workspace]
/// members` entry (minus `exclude`, deduplicated), resolved to an
/// absolute directory, in declaration order (a `/*` pattern's own matches
/// are sorted for determinism).
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
///
/// `[workspace] default-members` (issue #12 corpus review, F10/MEDIUM) is
/// deliberately **not** read: Cargo only uses `default-members` to narrow
/// what a bare `cargo build`/`cargo test` at the workspace root builds
/// when *no* `-p`/`--workspace` flag is given, but a plain member list is
/// still what every other workspace-aware command (`cargo build
/// --workspace`, `cargo metadata`, and this function's own callers,
/// `cargo_matrix`'s full-workspace checks) needs. `outou build` has no
/// per-package flag of its own in Phase 0, so it always builds every
/// member — the `--workspace` behavior, not the bare-invocation one —
/// and reading `default-members` here would only be able to narrow that,
/// never widen it back when a caller actually wants everything. Recorded
/// as a `TODO(phase0)` rather than silently decided (`AGENTS.md`): a
/// `--package`/`--workspace`-aware `outou build` that needs
/// `default-members` for its own bare-invocation default is future work.
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

    let member_patterns = string_array(workspace, "members");
    let mut members = Vec::new();
    for pattern in member_patterns {
        match pattern.strip_suffix("/*") {
            Some(prefix) => members.extend(glob_star(manifest_dir, prefix)?),
            None => members.push(manifest_dir.join(pattern)),
        }
    }

    // `exclude` (F10): each entry is a literal path (Cargo does not glob
    // `exclude` either, beyond the same `members` glob support), resolved
    // the same way as a non-globbed `members` entry and removed from the
    // resolved set. Applied *after* `members` globbing, matching Cargo's
    // own semantics ("exclude" removes paths that would otherwise be
    // included, including ones a `/*` glob just expanded to).
    let exclude_patterns = string_array(workspace, "exclude");
    let excluded: Vec<PathBuf> = exclude_patterns
        .into_iter()
        .map(|pattern| manifest_dir.join(pattern))
        .collect();
    members.retain(|member| !excluded.contains(member));

    // Deduplicate while preserving first-declared order (F10): an
    // overlapping `members` list — an explicit path that a `/*` glob also
    // matches, or the same pattern listed twice — must build that member
    // once, not once per listing.
    let mut seen = std::collections::HashSet::new();
    members.retain(|member| seen.insert(member.clone()));

    if members.is_empty() {
        return Err(WorkspaceError::NoMembers {
            path: manifest_dir.to_path_buf(),
        });
    }
    Ok(Some(members))
}

/// Reads a `[workspace]` table key as a plain array of strings, or an
/// empty `Vec` if the key is absent or not an array of strings — the same
/// tolerant shape `members` was already read with.
fn string_array(workspace: &toml::Value, key: &str) -> Vec<String> {
    workspace
        .get(key)
        .and_then(|value| value.as_array())
        .map(|array| {
            array
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
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

    /// F10 (issue #12 corpus review, MEDIUM): `exclude` removes a path
    /// `members` would otherwise include, even one a `/*` glob just
    /// expanded to.
    #[test]
    fn excluded_members_are_removed() {
        let dir = temp_dir("exclude");
        write(
            &dir,
            "Cargo.toml",
            "[workspace]\nmembers = [\"app\", \"crates/*\"]\nexclude = [\"crates/legacy\"]\n",
        );
        write(&dir, "app/Cargo.toml", "[package]\nname = \"app\"\n");
        write(&dir, "crates/a/Cargo.toml", "[package]\nname = \"a\"\n");
        write(
            &dir,
            "crates/legacy/Cargo.toml",
            "[package]\nname = \"legacy\"\n",
        );

        let members = workspace_members(&dir)
            .expect("reading succeeds")
            .expect("this manifest has a [workspace] table");

        fs::remove_dir_all(&dir).ok();
        assert_eq!(members, vec![dir.join("app"), dir.join("crates/a")]);
    }

    /// F10: a member listed twice (once explicitly, once via a `/*` glob
    /// that also matches it) is only built once.
    #[test]
    fn duplicate_members_are_deduplicated() {
        let dir = temp_dir("dedupe");
        write(
            &dir,
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/a\", \"crates/*\"]\n",
        );
        write(&dir, "crates/a/Cargo.toml", "[package]\nname = \"a\"\n");

        let members = workspace_members(&dir)
            .expect("reading succeeds")
            .expect("this manifest has a [workspace] table");

        fs::remove_dir_all(&dir).ok();
        assert_eq!(members, vec![dir.join("crates/a")]);
    }

    /// F10: a `[workspace]` table (with no `[package]`) that resolves to
    /// zero members is a clear error, not a silent "nothing to do" —
    /// `outou build`'s previous behavior for this shape (`main.rs` prints
    /// the same message a plain, non-`.rsx` crate gets, which is
    /// misleading for an actual workspace with a real configuration
    /// mistake).
    #[test]
    fn a_workspace_with_zero_members_is_an_error() {
        let dir = temp_dir("zero-members");
        write(&dir, "Cargo.toml", "[workspace]\nmembers = []\n");

        let error = workspace_members(&dir).expect_err("zero members must error");

        fs::remove_dir_all(&dir).ok();
        assert!(matches!(error, WorkspaceError::NoMembers { .. }));
        assert!(error.to_string().contains("no members"), "{error}");
    }

    /// The same zero-members error fires when `exclude` removes every
    /// resolved member, not only when `members` itself is empty.
    #[test]
    fn excluding_every_member_is_also_an_error() {
        let dir = temp_dir("exclude-all");
        write(
            &dir,
            "Cargo.toml",
            "[workspace]\nmembers = [\"app\"]\nexclude = [\"app\"]\n",
        );
        write(&dir, "app/Cargo.toml", "[package]\nname = \"app\"\n");

        let error = workspace_members(&dir).expect_err("zero members after exclude must error");

        fs::remove_dir_all(&dir).ok();
        assert!(matches!(error, WorkspaceError::NoMembers { .. }));
    }
}
