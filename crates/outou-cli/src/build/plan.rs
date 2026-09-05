//! Turns a resolved [`outou_modules::ModuleGraph`] into a concrete, ordered
//! list of generation units: absolute source/output paths and the
//! `module_paths` map each unit's `#[path]` rewrites need
//! (`crates/outou-modules/README.md`'s generated-path convention, ADR
//! 0009 layout (b)).
//!
//! This module does no I/O beyond what [`outou_modules::resolve`] already
//! does (reading every `.rs`/`.rsx` file to find `mod` items) and beyond
//! probing which of the four crate-root candidates exist. It never writes
//! a file; see `crate::build::emit` for that.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use outou_modules::{ModuleError, ModuleGraph, ModuleNode, SourceKind};

/// One of the four crate-root candidates Cargo itself recognizes, at
/// `manifest_dir`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrateRoot {
    /// No `.rsx` crate root exists: either there is no root file at all,
    /// or only a plain `.rs` one. `outou build` has nothing to do.
    NoRsxRoot,
    /// The `.rsx` crate root to build.
    Rsx(PathBuf),
}

/// Errors from [`find_crate_root`] and [`plan`].
#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    /// Both a `.rsx` root (`main.rsx`/`lib.rsx`) and its `.rs` counterpart
    /// exist (`main.rs`/`lib.rs` present alongside an `.rsx` root).
    #[error(
        "both a `.rsx` crate root and a `.rs` crate root exist under `{}`: {rsx} and {rs}; keep exactly one",
        src_dir.display()
    )]
    MixedRoots {
        /// The `src/` directory that was searched.
        src_dir: PathBuf,
        /// The `.rsx` root found, relative to `src_dir`.
        rsx: String,
        /// The `.rs` root found, relative to `src_dir`.
        rs: String,
    },
    /// Both `main.rsx` and `lib.rsx` exist under the same `src/`.
    #[error(
        "both `main.rsx` and `lib.rsx` exist under `{}`; a crate has exactly one root",
        src_dir.display()
    )]
    AmbiguousRsxRoot {
        /// The `src/` directory that was searched.
        src_dir: PathBuf,
    },
    /// Module graph resolution failed.
    #[error(transparent)]
    Module(#[from] ModuleError),
    /// A plain Rust file (`.rs`) declares a `.rsx` module directly. Phase
    /// 0 never rewrites a `.rs` file's `mod` declaration (codegen only
    /// touches generated `.rs` files under `src/.generated/`), so there
    /// is no way to point such a declaration at the `.rsx` child's
    /// generated output.
    #[error(
        "module `{name}` is declared from the plain Rust file `{}`; Phase 0 requires `.rsx` modules to be declared from `.rsx` files or from the crate root",
        declared_in.display()
    )]
    RustDeclaresRsxChild {
        /// The `.rsx` child module's name, as written in `mod {name};`.
        name: String,
        /// The plain Rust file containing the offending `mod` declaration,
        /// relative to the crate root.
        declared_in: PathBuf,
    },
    /// `manifest_dir` could not be resolved to an absolute path (it does
    /// not exist, or is not readable).
    #[error("resolving crate directory `{}`: {source}", path.display())]
    ManifestDir {
        /// The directory that could not be resolved.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
}

/// Resolves `manifest_dir` to an absolute path. `GenerateOptions::generated_uri`
/// / `source_uri` are `file://` + absolute path (documented in
/// `crates/outou-cli/README.md`), so every path this pipeline computes
/// from `manifest_dir` has to start out absolute.
pub fn canonical_manifest_dir(manifest_dir: &Path) -> Result<PathBuf, PlanError> {
    manifest_dir
        .canonicalize()
        .map_err(|source| PlanError::ManifestDir {
            path: manifest_dir.to_path_buf(),
            source,
        })
}

/// Looks for a crate root under `manifest_dir/src/`, per the four
/// candidates Cargo itself recognizes (`main.rs`, `main.rsx`, `lib.rs`,
/// `lib.rsx`).
///
/// `manifest_dir` should already be absolute ([`canonical_manifest_dir`]);
/// this function does not canonicalize it itself so tests may pass a
/// relative fixture path directly.
pub fn find_crate_root(manifest_dir: &Path) -> Result<CrateRoot, PlanError> {
    let src_dir = manifest_dir.join("src");
    let main_rsx = src_dir.join("main.rsx");
    let lib_rsx = src_dir.join("lib.rsx");
    let main_rs = src_dir.join("main.rs");
    let lib_rs = src_dir.join("lib.rs");

    let rsx_roots: Vec<(&str, &Path)> = [
        ("main.rsx", main_rsx.as_path()),
        ("lib.rsx", lib_rsx.as_path()),
    ]
    .into_iter()
    .filter(|(_, p)| p.is_file())
    .collect();
    let rs_roots: Vec<(&str, &Path)> =
        [("main.rs", main_rs.as_path()), ("lib.rs", lib_rs.as_path())]
            .into_iter()
            .filter(|(_, p)| p.is_file())
            .collect();

    match rsx_roots.len() {
        0 => Ok(CrateRoot::NoRsxRoot),
        1 => {
            if let Some((rs_name, _)) = rs_roots.first() {
                return Err(PlanError::MixedRoots {
                    src_dir,
                    rsx: rsx_roots[0].0.to_string(),
                    rs: (*rs_name).to_string(),
                });
            }
            Ok(CrateRoot::Rsx(rsx_roots[0].1.to_path_buf()))
        }
        _ => Err(PlanError::AmbiguousRsxRoot { src_dir }),
    }
}

/// One `.rsx` compile unit: an absolute source path, its absolute
/// generated `.rs` path and sidecar `.rs.map.json` path, and the
/// `module_paths` map codegen needs for this unit's own `#[path]`
/// rewrites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedUnit {
    /// This unit's module path (empty for the crate root), for
    /// diagnostics and tests.
    pub module_path: Vec<String>,
    /// Absolute path to the `.rsx` source.
    pub source_file: PathBuf,
    /// Absolute path to the generated `.rs` file.
    pub generated_file: PathBuf,
    /// Absolute path to the generated `.rs.map.json` sidecar.
    pub map_file: PathBuf,
    /// Module name (raw spelling) → `#[path]` string, for this unit's
    /// direct, non-inline children.
    pub module_paths: BTreeMap<String, String>,
}

/// A fully planned build: every generation unit, plus the crate/`src`/
/// `.generated` directories `emit`/`clean` need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Directory containing `Cargo.toml` and `src/`.
    pub crate_dir: PathBuf,
    /// `crate_dir/src`.
    pub src_dir: PathBuf,
    /// `crate_dir/src/.generated`.
    pub generated_dir: PathBuf,
    /// Units to generate, in [`ModuleGraph::generated_units`] order (root
    /// first, depth-first, pre-order).
    pub units: Vec<PlannedUnit>,
}

/// Resolves the module graph at `root` and plans every generation unit.
///
/// `manifest_dir` must be `root`'s crate root directory (the directory
/// containing `root`'s own `src/`).
pub fn plan(manifest_dir: &Path, root: &Path) -> Result<Plan, PlanError> {
    let graph = outou_modules::resolve(root)?;
    check_no_rust_declares_rsx_child(&graph)?;

    let src_dir = manifest_dir.join("src");
    let generated_dir = src_dir.join(".generated");

    let units = graph
        .generated_units()
        .map(|node| planned_unit(node, manifest_dir, &src_dir))
        .collect();

    Ok(Plan {
        crate_dir: manifest_dir.to_path_buf(),
        src_dir,
        generated_dir,
        units,
    })
}

/// Detects the one shape codegen cannot rewrite (see
/// [`PlanError::RustDeclaresRsxChild`]): a plain Rust (`.rs`) node with a
/// non-inline `.rsx` child. Walks the whole graph, not just direct
/// children of generation units, since the offending `.rs` file may be
/// nested arbitrarily deep under other plain `.rs` files.
fn check_no_rust_declares_rsx_child(graph: &ModuleGraph) -> Result<(), PlanError> {
    for node in graph.iter() {
        if node.kind != SourceKind::Rust {
            continue;
        }
        for child in &node.children {
            if child.kind == SourceKind::Rsx && !child.is_inline {
                return Err(PlanError::RustDeclaresRsxChild {
                    name: child.path.last().cloned().unwrap_or_default(),
                    declared_in: child
                        .declared_in
                        .clone()
                        .unwrap_or_else(|| node.file.clone()),
                });
            }
        }
    }
    Ok(())
}

fn planned_unit(node: &ModuleNode, crate_dir: &Path, src_dir: &Path) -> PlannedUnit {
    let generated_file = node.generated_path(src_dir);
    let mut map_file = generated_file.clone();
    map_file.set_extension("rs.map.json");
    let generated_dir_of_unit = generated_file
        .parent()
        .expect("generated_path always has a parent")
        .to_path_buf();

    let mut module_paths = BTreeMap::new();
    collect_module_paths(
        node,
        &generated_dir_of_unit,
        crate_dir,
        src_dir,
        &mut module_paths,
    );

    PlannedUnit {
        module_path: node.path.clone(),
        source_file: crate_dir.join(&node.file),
        generated_file,
        map_file,
        module_paths,
    }
}

/// Gathers the flat `module_paths` map for one generation unit.
///
/// `GenerateOptions::module_paths` is keyed only by module *name*, with no
/// notion of nesting, because one physical generated file can contain
/// several levels of *inline* modules (`mod m { mod n; }`) whose content
/// all lives in that same file. So this walks past every inline child
/// (recursively — an inline module can itself contain further inline
/// modules) to find the file-based, non-inline descendants that actually
/// need a `#[path]` rewrite in this unit's text, without crossing into a
/// child that is itself a separate generation unit (a non-inline child's
/// own children are that child's concern, computed when *it* is planned).
fn collect_module_paths(
    node: &ModuleNode,
    unit_generated_dir: &Path,
    crate_dir: &Path,
    src_dir: &Path,
    out: &mut BTreeMap<String, String>,
) {
    for child in &node.children {
        if child.is_inline {
            collect_module_paths(child, unit_generated_dir, crate_dir, src_dir, out);
            continue;
        }
        let name = child.path.last().cloned().unwrap_or_default();
        let target = match child.kind {
            SourceKind::Rsx => child.generated_path(src_dir),
            SourceKind::Rust => crate_dir.join(&child.file),
        };
        let path_string = relative_path_string(unit_generated_dir, &target);
        let own_path_attribute = child
            .attributes
            .iter()
            .find(|text| outou_syntax::parser::attribute_meta_path(text) == Some("path"))
            .map(String::as_str);
        let key = outou_codegen::module_paths_key(&name, own_path_attribute);
        out.insert(key, path_string);
    }
}

/// Lexical relative path from directory `from_dir` to file `to`, joined
/// with `/` regardless of platform, so `#[path = "…"]` strings are
/// portable and deterministic. Both arguments must already share the same
/// root (both absolute, or both relative to the same base); this never
/// touches the filesystem.
fn relative_path_string(from_dir: &Path, to: &Path) -> String {
    let from_components: Vec<_> = from_dir.components().collect();
    let to_components: Vec<_> = to.components().collect();

    let mut common = 0;
    while common < from_components.len()
        && common < to_components.len()
        && from_components[common] == to_components[common]
    {
        common += 1;
    }

    let mut segments: Vec<String> = Vec::new();
    for _ in common..from_components.len() {
        segments.push("..".to_string());
    }
    for component in &to_components[common..] {
        segments.push(component.as_os_str().to_string_lossy().into_owned());
    }

    segments.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/modules")
    }

    #[test]
    fn find_crate_root_locates_the_rsx_root() {
        let dir = fixtures_dir().join("mixed");
        let root = find_crate_root(&dir).expect("mixed has an rsx root");
        assert_eq!(root, CrateRoot::Rsx(dir.join("src/main.rsx")));
    }

    #[test]
    fn find_crate_root_reports_no_root_when_only_plain_rust_exists() {
        let dir = std::env::temp_dir().join(format!(
            "outou-cli-plan-test-plain-only-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();

        let root = find_crate_root(&dir).expect("no error for a plain crate");

        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(root, CrateRoot::NoRsxRoot);
    }

    #[test]
    fn find_crate_root_rejects_mixed_roots() {
        let dir = std::env::temp_dir().join(format!(
            "outou-cli-plan-test-mixed-roots-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.join("src/main.rsx"), "fn main() {}").unwrap();

        let err = find_crate_root(&dir).expect_err("mixed roots must be rejected");

        std::fs::remove_dir_all(&dir).ok();
        assert!(matches!(err, PlanError::MixedRoots { .. }), "{err:?}");
    }

    #[test]
    fn plan_mixed_fixture_computes_module_paths_for_rsx_and_rust_children() {
        let dir = fixtures_dir().join("mixed");
        let root = dir.join("src/main.rsx");
        let planned = plan(&dir, &root).expect("mixed resolves");

        assert_eq!(planned.units.len(), 3);

        let root_unit = &planned.units[0];
        assert!(root_unit.module_path.is_empty());
        assert_eq!(
            root_unit.generated_file,
            dir.join("src/.generated/crate-root.rs")
        );
        assert_eq!(
            root_unit.module_paths.get("components").map(String::as_str),
            Some("components.rs")
        );

        let components_unit = planned
            .units
            .iter()
            .find(|u| u.module_path == vec!["components".to_string()])
            .expect("components unit");
        assert_eq!(
            components_unit.module_paths.get("user").map(String::as_str),
            Some("components/user.rs")
        );
        assert_eq!(
            components_unit
                .module_paths
                .get("button")
                .map(String::as_str),
            Some("../components/button.rs")
        );
    }

    #[test]
    fn plan_rejects_a_plain_rust_file_declaring_an_rsx_child() {
        let dir = fixtures_dir().join("rs-to-rsx");
        let root = dir.join("src/main.rsx");
        let err = plan(&dir, &root).expect_err("plain.rs declares plain/deep.rsx");
        match err {
            PlanError::RustDeclaresRsxChild { name, declared_in } => {
                assert_eq!(name, "deep");
                assert_eq!(declared_in, PathBuf::from("src/plain.rs"));
            }
            other => panic!("expected RustDeclaresRsxChild, got {other:?}"),
        }
    }

    #[test]
    fn plan_rejects_an_rsx_child_declared_via_path_from_a_rust_file() {
        let dir = fixtures_dir().join("path-attr");
        let root = dir.join("src/main.rsx");
        let err = plan(&dir, &root).expect_err("somewhere/other.rs declares nested_from_rust.rsx");
        assert!(
            matches!(err, PlanError::RustDeclaresRsxChild { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn plan_cfg_duplicate_fixture_disambiguates_generated_names() {
        let dir = fixtures_dir().join("cfg-duplicate");
        let root = dir.join("src/main.rsx");
        let planned = plan(&dir, &root).expect("cfg-duplicate resolves");

        // Both `mod imp;` declarations share a name, but each carries its
        // own explicit `#[path]`, so `module_paths_key` disambiguates them
        // into two distinct entries rather than one clobbering the other.
        let root_unit = &planned.units[0];
        assert_eq!(planned.units.len(), 3);
        let unix_key = outou_codegen::module_paths_key("imp", Some("#[path = \"unix.rsx\"]"));
        let windows_key = outou_codegen::module_paths_key("imp", Some("#[path = \"windows.rsx\"]"));
        assert_eq!(
            root_unit.module_paths.get(&unix_key).map(String::as_str),
            Some("imp.rs")
        );
        assert_eq!(
            root_unit.module_paths.get(&windows_key).map(String::as_str),
            Some("imp-1.rs")
        );

        let generated_names: Vec<PathBuf> = planned
            .units
            .iter()
            .map(|u| u.generated_file.clone())
            .collect();
        assert!(generated_names.contains(&dir.join("src/.generated/imp.rs")));
        assert!(generated_names.contains(&dir.join("src/.generated/imp-1.rs")));
    }

    #[test]
    fn plan_inline_fixture_has_no_module_path_entry_for_inline_children() {
        let dir = fixtures_dir().join("inline");
        let root = dir.join("src/main.rsx");
        let planned = plan(&dir, &root).expect("inline resolves");

        // `mod shell { mod panel; }`: `shell` is inline (no file of its
        // own), so the root's module_paths has no "shell" entry, and
        // `shell`'s own generated unit (its parent's file, `crate-root.rs`)
        // carries the `panel` entry instead.
        let root_unit = &planned.units[0];
        assert!(!root_unit.module_paths.contains_key("shell"));
        assert_eq!(
            root_unit.module_paths.get("panel").map(String::as_str),
            Some("shell/panel.rs")
        );
    }
}
