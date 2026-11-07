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

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use outou_cli::build::plan::{
    canonical_manifest_dir, find_crate_root, plan_with_overlay, CrateRoot, PlanError,
};
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

/// Resolves the crate at `manifest_dir`, per [`Resolved`], reading every
/// file from disk.
pub fn resolve(manifest_dir: &Path) -> Result<Resolved, PlanError> {
    resolve_with_overlay(manifest_dir, &HashMap::new())
}

/// Like [`resolve`], but any file whose canonicalized path matches a key
/// of `overlay` is planned using that text instead of its contents on
/// disk (issue #9 Gate 3 review, M2/HIGH-2): `outou_cli::build::plan`
/// itself has no notion of an editor's unsaved buffers, so this is where
/// the language server's own state (`crate::documents::Workspace`)
/// injects it.
pub fn resolve_with_overlay(
    manifest_dir: &Path,
    overlay: &HashMap<PathBuf, String>,
) -> Result<Resolved, PlanError> {
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
    let planned = plan_with_overlay(&canonical, &root, overlay)?;
    Ok(Resolved::Planned {
        manifest_dir: canonical,
        plan: planned,
    })
}

/// One `mod` declaration's shape, for detecting whether an edit changed
/// the module graph rather than only the identifiers it happens to use
/// (issue #9 Gate 3 review, M2: comparing only a `BTreeSet<String>` of
/// names missed a `#[path]` target changing, or a declaration moving from
/// file-based to inline, while the name stayed the same — either of which
/// changes what `outou_modules::resolve` produces just as much as adding
/// or removing a `mod` does).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleDescriptor {
    /// The declared name (`mod NAME;` / `mod NAME { … }`).
    pub name: String,
    /// The explicit `#[path = "…"]` target, if any.
    pub path_attr: Option<String>,
    /// Whether this is an inline module (`mod name { … }`) rather than a
    /// file-based one (`mod name;`).
    pub is_inline: bool,
    /// How many inline modules enclose this declaration. In a pre-order
    /// list, the depths fix the nesting tree uniquely, so moving a `mod`
    /// into or out of an inline module changes the list.
    pub depth: usize,
}

/// Every module declared anywhere in `file`, at any nesting depth, inline
/// or file-based, **in source order** (so two declarations that swap
/// order, or a duplicate declaration, are also detected as a change).
/// Used by [`crate::documents::Workspace::module_shape_changed`] to
/// decide whether an edit requires a full re-plan.
pub fn declared_modules(file: &ast::File) -> Vec<ModuleDescriptor> {
    let mut out = Vec::new();
    collect_declared_modules(&file.items, 0, &mut out);
    out
}

fn collect_declared_modules(items: &[ast::Item], depth: usize, out: &mut Vec<ModuleDescriptor>) {
    for item in items {
        if let ast::Item::Module(module) = item {
            out.push(ModuleDescriptor {
                name: module.name.name.clone(),
                path_attr: module.path.clone(),
                is_inline: module.items.is_some(),
                depth,
            });
            if let Some(inner) = &module.items {
                collect_declared_modules(inner, depth + 1, out);
            }
        }
    }
}

/// The `span.start` of every module declared anywhere in `file`, in the
/// same order as [`declared_modules`] (source order, inline and file-based,
/// at any depth). These are the keys `PlannedUnit::module_paths` has for
/// the text the unit was planned from; see [`rekey_module_paths`].
pub fn module_span_starts(file: &ast::File) -> Vec<u32> {
    let mut out = Vec::new();
    collect_module_span_starts(&file.items, &mut out);
    out
}

fn collect_module_span_starts(items: &[ast::Item], out: &mut Vec<u32>) {
    for item in items {
        if let ast::Item::Module(module) = item {
            out.push(module.span.start);
            if let Some(inner) = &module.items {
                collect_module_span_starts(inner, out);
            }
        }
    }
}

/// Moves `module_paths` (keyed by declaration `span.start` in the text a
/// unit was planned from, `planned`) onto the declarations of an edited
/// text (`edited`), matching the n-th declaration to the n-th.
///
/// `PlannedUnit::module_paths` is keyed by byte offset (the key
/// `outou_backend_dioxus` looks up), and a keystroke above a `mod` shifts
/// that offset without changing the module set, which used to drop the
/// `#[path]` rewrite from the text sent to rust-analyzer (#31, D4). The
/// n-th declaration in source order stays the same declaration for as long
/// as [`declared_modules`] (names, `#[path]` values, inline or not, and
/// nesting depth, which together fix the declaration tree) is unchanged, which is what
/// `Workspace::module_shape_changed` checks before a unit is regenerated on
/// its own; only a caller that has established that may use this. If the
/// counts differ anyway, `module_paths` is returned unchanged.
pub fn rekey_module_paths(
    module_paths: &BTreeMap<u32, String>,
    planned: &[u32],
    edited: &[u32],
) -> BTreeMap<u32, String> {
    if planned.len() != edited.len() {
        return module_paths.clone();
    }
    module_paths
        .iter()
        .filter_map(|(span_start, path)| {
            let ordinal = planned.iter().position(|start| start == span_start)?;
            Some((edited[ordinal], path.clone()))
        })
        .collect()
}

/// Every `#[component]` function declared anywhere in `file`, at any
/// nesting depth (top level or inside an inline module). Used by
/// [`crate::complete`] to answer tag-name completion locally, without ever
/// asking rust-analyzer: a `.rsx` position is either a tag name or it is
/// not, and rust-analyzer only ever sees the *expanded* Rust, where that
/// distinction has already been lost.
pub fn component_functions(file: &ast::File) -> Vec<&ast::Function> {
    let mut out = Vec::new();
    collect_component_functions(&file.items, &mut out);
    out
}

fn collect_component_functions<'a>(items: &'a [ast::Item], out: &mut Vec<&'a ast::Function>) {
    for item in items {
        match item {
            ast::Item::Function(function) if function.is_component => out.push(function),
            ast::Item::Module(module) => {
                if let Some(inner) = &module.items {
                    collect_component_functions(inner, out);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_file_based_and_inline_module_descriptors_in_source_order() {
        let parsed = outou_syntax::parse("mod a; mod b { mod c; }");
        let descriptors = declared_modules(&parsed.file);
        let names: Vec<&str> = descriptors.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn a_file_with_no_modules_has_no_declarations() {
        let parsed = outou_syntax::parse("fn f() {}");
        assert!(declared_modules(&parsed.file).is_empty());
    }

    /// M2: a `#[path]` target change (same name) must compare unequal — a
    /// `BTreeSet<String>` of names alone would have missed this.
    #[test]
    fn a_changed_path_attribute_is_a_different_descriptor() {
        let before = outou_syntax::parse("#[path = \"a.rs\"] mod m;");
        let after = outou_syntax::parse("#[path = \"b.rs\"] mod m;");
        assert_ne!(
            declared_modules(&before.file),
            declared_modules(&after.file)
        );
    }

    /// Moving a declaration from file-based to inline (same name) must
    /// also compare unequal.
    #[test]
    fn switching_between_inline_and_file_based_is_a_different_descriptor() {
        let file_based = outou_syntax::parse("mod m;");
        let inline = outou_syntax::parse("mod m {}");
        assert_ne!(
            declared_modules(&file_based.file),
            declared_modules(&inline.file)
        );
    }

    #[test]
    fn component_functions_finds_top_level_and_inline_components_only() {
        let parsed = outou_syntax::parse(
            "fn plain() {}\n\
             #[component]\n\
             fn Greeting(name: String) -> Element { <h1>{name}</h1> }\n\
             mod nested {\n\
                 #[component]\n\
                 fn Inner() -> Element { <p>hi</p> }\n\
             }",
        );
        let names: Vec<&str> = component_functions(&parsed.file)
            .iter()
            .map(|f| f.name.name.as_str())
            .collect();
        assert_eq!(names, vec!["Greeting", "Inner"]);
    }

    fn spans(source: &str) -> Vec<u32> {
        module_span_starts(&outou_syntax::parse(source).file)
    }

    #[test]
    fn module_span_starts_follow_declared_modules_order_and_nesting() {
        let source = "mod a; mod b { mod c; } mod d;";
        let starts = spans(source);
        assert_eq!(
            starts.len(),
            declared_modules(&outou_syntax::parse(source).file).len()
        );
        assert!(
            starts.windows(2).all(|pair| pair[0] < pair[1]),
            "{starts:?}"
        );
    }

    #[test]
    fn rekey_follows_each_declaration_when_text_is_typed_above_it() {
        let planned = spans("mod a; mod b { mod a; }");
        let edited = spans("// typed\nmod a; mod b { mod a; }");
        let paths: BTreeMap<u32, String> = [
            (planned[0], "outer-a.rs".to_string()),
            (planned[2], "inner-a.rs".to_string()),
        ]
        .into();

        let rekeyed = rekey_module_paths(&paths, &planned, &edited);

        assert_eq!(rekeyed.len(), 2);
        assert_eq!(rekeyed[&edited[0]], "outer-a.rs");
        assert_eq!(rekeyed[&edited[2]], "inner-a.rs");
        assert!(!rekeyed.contains_key(&edited[1]));
    }

    #[test]
    fn rekey_is_the_identity_when_the_text_did_not_move() {
        let planned = spans("mod a; mod b;");
        let paths: BTreeMap<u32, String> = [(planned[1], "b.rs".to_string())].into();
        assert_eq!(rekey_module_paths(&paths, &planned, &planned), paths);
    }

    #[test]
    fn rekey_leaves_the_map_alone_when_the_declaration_count_differs() {
        let planned = spans("mod a;");
        let edited = spans("// x\nmod a; mod b;");
        let paths: BTreeMap<u32, String> = [(planned[0], "a.rs".to_string())].into();
        assert_eq!(rekey_module_paths(&paths, &planned, &edited), paths);
    }

    #[test]
    fn rekey_drops_a_key_that_is_not_a_planned_declaration() {
        let planned = spans("mod a;");
        let edited = spans("// x\nmod a;");
        let paths: BTreeMap<u32, String> = [(planned[0] + 1, "a.rs".to_string())].into();
        assert!(rekey_module_paths(&paths, &planned, &edited).is_empty());
    }
}
