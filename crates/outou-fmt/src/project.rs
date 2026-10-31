//! What the formatted file's project says about how it is to be formatted:
//! the Rust edition of the crate it belongs to.
//!
//! `rustfmt.toml` discovery itself is `rustfmt`'s job: it is run with the
//! formatted file's directory as its working directory (`crate::ctx`), and
//! `rustfmt` searches that directory and its parents.

use std::path::{Path, PathBuf};

/// The edition used when no `Cargo.toml` names one. Matches this
/// workspace's own edition.
pub(crate) const DEFAULT_EDITION: &str = "2021";

/// The directory `rustfmt` should run in for the file at `source_path`: the
/// file's own directory, made absolute so it does not depend on later
/// changes of the working directory. `None` when it does not exist.
pub(crate) fn work_dir_of(source_path: &Path) -> Option<PathBuf> {
    absolute_parent(source_path).filter(|dir| dir.is_dir())
}

/// The absolute directory `source_path` would live in, whether or not it
/// exists yet (an unsaved editor buffer in a new folder).
fn absolute_parent(source_path: &Path) -> Option<PathBuf> {
    // Resolve a symlinked file to its real location, as the language server
    // sees it, so the real project's `rustfmt.toml` and `Cargo.toml` apply.
    let resolved = std::fs::canonicalize(source_path)
        .or_else(|_| std::path::absolute(source_path))
        .ok()?;
    Some(resolved.parent()?.to_path_buf())
}

/// The edition of the crate the file at `source_path` belongs to: the
/// `edition` of the nearest `Cargo.toml` with a `[package]` table, honouring
/// `edition.workspace = true` through the nearest enclosing
/// `[workspace.package]`. `None` when there is no such manifest, or it names
/// no usable edition.
pub(crate) fn detect_edition(source_path: &Path) -> Option<String> {
    let dir = absolute_parent(source_path)?;
    for ancestor in dir.ancestors() {
        let Some(manifest) = read_manifest(&ancestor.join("Cargo.toml")) else {
            continue;
        };
        // The nearest manifest decides. One without a `[package]` is a
        // workspace root (or virtual manifest): its `[workspace.package]`
        // edition applies, and no enclosing crate's does.
        let Some(package) = manifest.get("package").and_then(|value| value.as_table()) else {
            return manifest_workspace_edition(&manifest);
        };
        return match package.get("edition") {
            Some(toml::Value::String(edition)) => Some(edition.clone()),
            Some(toml::Value::Table(table))
                if table.get("workspace").and_then(toml::Value::as_bool) == Some(true) =>
            {
                workspace_edition(ancestor)
            }
            _ => None,
        };
    }
    None
}

/// `[workspace.package] edition` of the nearest manifest at or above
/// `member_dir` that has a `[workspace]` table.
fn workspace_edition(member_dir: &Path) -> Option<String> {
    member_dir.ancestors().find_map(|ancestor| {
        let manifest = read_manifest(&ancestor.join("Cargo.toml"))?;
        manifest.get("workspace")?;
        manifest_workspace_edition(&manifest)
    })
}

fn manifest_workspace_edition(manifest: &toml::Table) -> Option<String> {
    manifest
        .get("workspace")?
        .as_table()?
        .get("package")?
        .get("edition")?
        .as_str()
        .map(str::to_string)
}

fn read_manifest(path: &Path) -> Option<toml::Table> {
    std::fs::read_to_string(path).ok()?.parse().ok()
}
