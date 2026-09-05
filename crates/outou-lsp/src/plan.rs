//! Thin wrapper over `outou_cli::build::plan`: "there is one compiler"
//! (`AGENTS.md`) means the language server plans a crate's `.rsx` units
//! with the exact same code `outou build` does, never a second resolver.
//!
//! This module adds only what the LSP needs on top: turning "no `.rsx`
//! crate root" into a degraded-mode signal instead of an error, and
//! collecting the declared module names of one unit's own file so
//! [`crate::documents::Workspace`] can tell whether an edit changed the
//! module graph's shape (in which case the whole crate must be re-planned)
//! or not (in which case only that one unit needs regenerating).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use outou_cli::build::plan::{canonical_manifest_dir, find_crate_root, plan, CrateRoot, PlanError};
pub use outou_cli::build::plan::{Plan, PlannedUnit};
use outou_syntax::ast;

/// Outcome of resolving a workspace root: either a full plan, or a
/// documented degraded mode (Week 5 architecture note: "if `src/main.rsx`/
/// `lib.rsx` is absent, run in a degraded mode that only publishes Outou
/// syntax diagnostics").
pub enum Resolved {
    /// A `.rsx` crate root was found and the whole crate planned.
    Planned {
        /// The crate's manifest directory, canonicalized.
        manifest_dir: PathBuf,
        /// The plan itself.
        plan: Plan,
    },
    /// No `.rsx` crate root exists under `manifest_dir`. rust-analyzer is
    /// never spawned; only Outou syntax diagnostics are published.
    Degraded {
        /// The crate's manifest directory, canonicalized (best effort: the
        /// original, uncanonicalized directory if canonicalization itself
        /// failed, since a missing root need not mean a missing directory).
        manifest_dir: PathBuf,
    },
}

/// Resolves the crate at `manifest_dir`, per [`Resolved`].
pub fn resolve(manifest_dir: &Path) -> Result<Resolved, PlanError> {
    let canonical = match canonical_manifest_dir(manifest_dir) {
        Ok(dir) => dir,
        Err(_) => {
            return Ok(Resolved::Degraded {
                manifest_dir: manifest_dir.to_path_buf(),
            })
        }
    };
    let root = match find_crate_root(&canonical)? {
        CrateRoot::NoRsxRoot => {
            return Ok(Resolved::Degraded {
                manifest_dir: canonical,
            })
        }
        CrateRoot::Rsx(root) => root,
    };
    let planned = plan(&canonical, &root)?;
    Ok(Resolved::Planned {
        manifest_dir: canonical,
        plan: planned,
    })
}

/// Every module name declared anywhere in `file`, at any nesting depth,
/// inline or file-based. Used to detect whether an edit changed the set of
/// modules a unit declares (issue #9 architecture note: "re-plan only when
/// a `mod` declaration set changes"). This is deliberately coarser than
/// "only file-based declarations change the module graph's shape" — an
/// edit to a purely inline module's name also triggers a re-plan under
/// this rule — because it can never miss a graph-shape change, only
/// over-trigger a full re-plan, which is always safe, just not maximally
/// incremental.
pub fn declared_module_names(file: &ast::File) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    collect_from_items(&file.items, &mut names);
    names
}

fn collect_from_items(items: &[ast::Item], names: &mut BTreeSet<String>) {
    for item in items {
        if let ast::Item::Module(module) = item {
            names.insert(module.name.name.clone());
            if let Some(inner) = &module.items {
                collect_from_items(inner, names);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_file_based_and_inline_module_names() {
        let parsed = outou_syntax::parse("mod a; mod b { mod c; }");
        let names = declared_module_names(&parsed.file);
        assert_eq!(
            names,
            ["a", "b", "c"].into_iter().map(String::from).collect()
        );
    }

    #[test]
    fn a_file_with_no_modules_has_an_empty_set() {
        let parsed = outou_syntax::parse("fn f() {}");
        assert!(declared_module_names(&parsed.file).is_empty());
    }
}
