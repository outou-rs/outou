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

use outou_modules::{unraw, ModuleError, ModuleGraph, ModuleNode, SourceKind};

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
    /// Two module declarations inside one generation unit produced the
    /// same `module_paths` key (the declaration's own `span.start`).
    /// `outou_modules::resolve` guarantees every declaration span is
    /// unique within its own source file, so this should never actually
    /// trigger; it exists as a defensive check against a future resolver
    /// or planner bug producing a silently wrong `#[path]` rewrite (issue
    /// #8 fix list step 2) rather than one `mod` declaration silently
    /// clobbering another's path in the map.
    #[error(
        "internal error: two module declarations in `{}` share declaration position {span_start}",
        unit_source.display()
    )]
    DuplicateModuleDeclaration {
        /// The unit whose own text contains both declarations.
        unit_source: PathBuf,
        /// The span start both declarations share.
        span_start: u32,
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
    /// Declaration span start (`ModuleNode::span.start`, within this
    /// unit's own source file) → `#[path]` string, for every non-inline
    /// child reachable from this unit without crossing into another
    /// generation unit (issue #8 fix list step 2).
    pub module_paths: BTreeMap<u32, String>,
    /// Every distinct directory an inline module's own `#[path]`-resolution
    /// base descends into while planning this unit (issue #8 fix list
    /// step 3, N1). `emit` must `create_dir_all` each of these before
    /// writing: rustc resolves a nested `#[path]`'s `..` components
    /// relative to this directory, and that resolution fails if the
    /// directory does not physically exist — even when no generated file
    /// is ever written directly into it (e.g. an inline module whose only
    /// child is a plain `.rs` file reached through a `../` escape).
    pub inline_base_dirs: Vec<PathBuf>,
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
        .collect::<Result<Vec<_>, _>>()?;

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

fn planned_unit(
    node: &ModuleNode,
    crate_dir: &Path,
    src_dir: &Path,
) -> Result<PlannedUnit, PlanError> {
    let generated_file = node.generated_path(src_dir);
    let mut map_file = generated_file.clone();
    map_file.set_extension("rs.map.json");
    let generated_dir_of_unit = generated_file
        .parent()
        .expect("generated_path always has a parent")
        .to_path_buf();
    let source_file = crate_dir.join(&node.file);

    let mut module_paths = BTreeMap::new();
    let mut inline_base_dirs = Vec::new();
    collect_module_paths(
        node,
        &generated_dir_of_unit,
        crate_dir,
        src_dir,
        &source_file,
        &mut module_paths,
        &mut inline_base_dirs,
    )?;

    Ok(PlannedUnit {
        module_path: node.path.clone(),
        source_file,
        generated_file,
        map_file,
        module_paths,
        inline_base_dirs,
    })
}

/// Gathers the flat `module_paths` map (and every inline base directory
/// that must exist before `emit` writes anything, [`PlannedUnit::inline_base_dirs`])
/// for one generation unit.
///
/// `GenerateOptions::module_paths` is keyed by declaration `span.start`
/// (issue #8 fix list step 2), with no notion of nesting, because one
/// physical generated file can contain several levels of *inline* modules
/// (`mod m { mod n; }`) whose content all lives in that same file. So this
/// walks past every inline child (recursively — an inline module can
/// itself contain further inline modules) to find the file-based,
/// non-inline descendants that actually need a `#[path]` rewrite in this
/// unit's text, without crossing into a child that is itself a separate
/// generation unit (a non-inline child's own children are that child's
/// concern, computed when *it* is planned).
///
/// `base_dir` tracks the directory rustc resolves both an inline child's
/// own `#[path]` and a nested descendant's `#[path]` against (issue #8 fix
/// list step 3, decision 1 — this is exactly `outou_modules::scope::DirScope`'s
/// model, specialized to the fact that `relative` is always `None` inside
/// a generated file, since every generated file is either the crate root
/// or `#[path]`-loaded — i.e. mod-rs-like, confirmed against real rustc):
/// starting at `unit_generated_dir`, entering an inline module `m` grows
/// `base_dir` by `m`'s own `#[path]` value if it has one, else by
/// `unraw(m)`.
///
/// A span start is unique within one source file, so two entries can only
/// collide from a resolver or planner bug; [`PlanError::DuplicateModuleDeclaration`]
/// is returned rather than silently letting the second insert clobber the
/// first (which is exactly how the pre-fix name-keyed map silently
/// mis-bound two sibling `mod helper;` declarations, HIGH-2).
fn collect_module_paths(
    node: &ModuleNode,
    base_dir: &Path,
    crate_dir: &Path,
    src_dir: &Path,
    unit_source: &Path,
    out: &mut BTreeMap<u32, String>,
    inline_base_dirs: &mut Vec<PathBuf>,
) -> Result<(), PlanError> {
    for child in &node.children {
        if child.is_inline {
            let own_name = child.path.last().map(String::as_str).unwrap_or_default();
            let child_base_dir = match own_path_attribute_value(&child.attributes) {
                Some(explicit) => base_dir.join(explicit),
                None => base_dir.join(unraw(own_name)),
            };
            if !inline_base_dirs.contains(&child_base_dir) {
                inline_base_dirs.push(child_base_dir.clone());
            }
            collect_module_paths(
                child,
                &child_base_dir,
                crate_dir,
                src_dir,
                unit_source,
                out,
                inline_base_dirs,
            )?;
            continue;
        }
        let target = match child.kind {
            SourceKind::Rsx => child.generated_path(src_dir),
            SourceKind::Rust => crate_dir.join(&child.file),
        };
        let path_string = relative_path_string(base_dir, &target);
        let span_start = child
            .span
            .expect("only the crate root has no span, and the crate root is never a child")
            .start;
        if out.insert(span_start, path_string).is_some() {
            return Err(PlanError::DuplicateModuleDeclaration {
                unit_source: unit_source.to_path_buf(),
                span_start,
            });
        }
    }
    Ok(())
}

/// Best-effort extraction of an inline module's own `#[path = "…"]`
/// string value from its collected attribute texts, mirroring
/// `outou_syntax::parser::item::extract_path_attribute` (private to that
/// crate) for the one case this crate needs — the same pattern
/// `outou-backend-dioxus`'s `module::is_path_attribute` already follows
/// for detecting (not extracting) a `#[path]` attribute without adding a
/// cross-crate dependency for it.
fn own_path_attribute_value(attributes: &[String]) -> Option<&str> {
    for attribute in attributes {
        if outou_syntax::parser::attribute_meta_path(attribute) != Some("path") {
            continue;
        }
        let after_path = attribute.find("path")? + "path".len();
        let rest = &attribute[after_path..];
        let quote_start = rest.find('"')? + 1;
        let after_quote = &rest[quote_start..];
        let quote_end = after_quote.find('"')?;
        return Some(&after_quote[..quote_end]);
    }
    None
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
mod tests;
