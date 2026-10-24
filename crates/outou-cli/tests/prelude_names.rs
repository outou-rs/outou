//! Pins the public names of `outou::prelude` (issue #29, ADR 0010's A5
//! amendment).
//!
//! The prelude is Outou's API: it does not `pub use` a backend item
//! directly, apart from a short list of exceptions. Reading `crates/outou/
//! src/lib.rs` as syntax (no build, so the test is cheap and sees names
//! behind a `#[cfg]` too) makes both rules checkable: the set of names is
//! exactly [`PINNED`], and every name that comes straight from
//! `crate::__private` is in [`BACKEND_EXCEPTIONS`]. Adding a name to the
//! prelude means changing this list on purpose.
//!
//! What this cannot see, on purpose:
//!
//! - A name that a `macro_rules!` expansion defines or re-exports. The test
//!   reads syntax, it does not expand macros; a macro *definition or call* in
//!   the prelude is therefore a failure, so the hole cannot be opened inside
//!   the prelude module itself.
//! - Whether a `#[cfg(feature = "..")]` on a name changes: the names behind a
//!   cfg are listed as if the feature were on.
//! - A `pub use` that goes through a module other than the two modules the
//!   prelude re-exports from (`state`, `runtime`); those two are read too,
//!   and a `pub use` of a backend path in either is a failure.

use std::fs;
use std::path::PathBuf;

use syn::{Item, UseTree};

/// Every name `use outou::prelude::*;` brings into scope, sorted.
const PINNED: &[&str] = &[
    "Element",
    "Props",
    "State",
    "component",
    "dioxus_core",
    "dioxus_elements",
    "dioxus_signals",
    "launch",
    "provide_context",
    "use_context",
    "use_state",
];

/// The names that may be re-exported straight from `crate::__private`.
/// `dioxus_core`, `dioxus_elements`, `dioxus_signals` and `Props` are the
/// hidden names the backend's macros need in scope (ledger rows 11 and 16);
/// `Element` is the backend's own type; `component` is the marker attribute.
/// Event argument types and `VirtualDom` are also allowed by ADR 0010 but
/// are not exported today; if they are, they are added to both lists.
const BACKEND_EXCEPTIONS: &[&str] = &[
    "Element",
    "Props",
    "component",
    "dioxus_core",
    "dioxus_elements",
    "dioxus_signals",
];

/// The modules (files under `crates/outou/src/`) the prelude re-exports
/// Outou-defined names from. Each must define what the prelude takes from it
/// and must not itself `pub use` a backend item.
const OWN_MODULES: &[(&str, &str)] = &[
    ("crate::state", "state.rs"),
    ("crate::runtime", "runtime.rs"),
];

/// Names old releases of the prelude exported. They must stay gone.
const REMOVED: &[&str] = &[
    "Signal",
    "use_signal",
    "use_context_provider",
    "ReadableExt",
    "WritableExt",
];

/// A name exported by a `pub use` and the module path it comes from.
struct Export {
    name: String,
    from: String,
}

fn source(file: &str) -> syn::File {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../outou/src")
        .join(file);
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    syn::parse_file(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn flatten(tree: &UseTree, prefix: &str, out: &mut Vec<Export>) {
    match tree {
        UseTree::Path(path) => flatten(&path.tree, &format!("{prefix}{}::", path.ident), out),
        UseTree::Name(name) => out.push(Export {
            name: name.ident.to_string(),
            from: prefix.trim_end_matches("::").to_owned(),
        }),
        UseTree::Rename(rename) => out.push(Export {
            name: rename.rename.to_string(),
            from: prefix.trim_end_matches("::").to_owned(),
        }),
        UseTree::Glob(_) => panic!("the prelude must not glob re-export (`{prefix}*`)"),
        UseTree::Group(group) => {
            for item in &group.items {
                flatten(item, prefix, out);
            }
        }
    }
}

fn kind(item: &Item) -> &'static str {
    match item {
        Item::Const(_) => "const",
        Item::Static(_) => "static",
        Item::Macro(_) => "macro",
        Item::ExternCrate(_) => "extern crate",
        Item::Union(_) => "union",
        Item::ForeignMod(_) => "extern block",
        Item::TraitAlias(_) => "trait alias",
        _ => "item",
    }
}

/// Everything public that the `prelude` module defines or re-exports.
fn prelude_exports() -> Vec<Export> {
    let file = source("lib.rs");
    let items = file
        .items
        .iter()
        .find_map(|item| match item {
            Item::Mod(module) if module.ident == "prelude" => {
                module.content.as_ref().map(|(_, items)| items)
            }
            _ => None,
        })
        .expect("`pub mod prelude { .. }` in crates/outou/src/lib.rs");

    let mut exports = Vec::new();
    for item in items {
        match item {
            Item::Use(item_use) if matches!(item_use.vis, syn::Visibility::Public(_)) => {
                flatten(&item_use.tree, "", &mut exports);
            }
            Item::Use(_) => {}
            Item::Fn(item) if matches!(item.vis, syn::Visibility::Public(_)) => {
                exports.push(Export {
                    name: item.sig.ident.to_string(),
                    from: "<defined in prelude>".to_owned(),
                });
            }
            Item::Struct(_) | Item::Enum(_) | Item::Trait(_) | Item::Type(_) | Item::Mod(_) => {
                panic!("define public items outside the prelude and re-export them by name")
            }
            // A constant, a static, a macro, an `extern crate`, a union or an
            // `extern` block can all introduce a name (`pub const Signal: () = ();`)
            // or hide one from this scan: none belongs in the prelude.
            Item::Const(_)
            | Item::Static(_)
            | Item::Macro(_)
            | Item::ExternCrate(_)
            | Item::Union(_)
            | Item::ForeignMod(_)
            | Item::TraitAlias(_)
            | Item::Verbatim(_) => {
                panic!(
                    "the prelude may only `pub use` names and define `pub fn`s: found a {}",
                    kind(item)
                )
            }
            Item::Fn(_) | Item::Impl(_) => {}
            _ => panic!("unrecognised item in the prelude; extend prelude_exports"),
        }
    }
    exports
}

#[test]
fn the_prelude_exports_exactly_the_pinned_names() {
    let mut names: Vec<String> = prelude_exports().into_iter().map(|e| e.name).collect();
    names.sort();
    let pinned: Vec<String> = PINNED.iter().map(|name| (*name).to_owned()).collect();
    assert_eq!(
        names, pinned,
        "the names of `outou::prelude` changed; if that is intended, update PINNED \
         (and ADR 0010's amendment table)"
    );
}

#[test]
fn only_the_listed_backend_items_are_re_exported_directly() {
    for export in prelude_exports() {
        if export.from.contains("__private") || export.from.contains("dioxus") {
            assert!(
                BACKEND_EXCEPTIONS.contains(&export.name.as_str()),
                "`{}` is re-exported from `{}`: wrap it under an Outou name instead \
                 (ADR 0010, A5 amendment)",
                export.name,
                export.from
            );
        }
    }
}

#[test]
fn the_old_state_names_are_gone() {
    let names: Vec<String> = prelude_exports().into_iter().map(|e| e.name).collect();
    for removed in REMOVED {
        assert!(
            !names.iter().any(|name| name == removed),
            "`{removed}` was removed from the prelude in issue #29"
        );
    }
}

/// The names a module of the facade defines itself (public `fn`, `struct`,
/// `enum`, `type`, `trait`), and the `pub use` re-exports it contains.
fn module_items(file: &str) -> (Vec<String>, Vec<Export>) {
    let mut defined = Vec::new();
    let mut re_exported = Vec::new();
    for item in source(file).items {
        match item {
            Item::Use(item_use) if !matches!(item_use.vis, syn::Visibility::Inherited) => {
                flatten(&item_use.tree, "", &mut re_exported);
            }
            Item::Fn(item) if matches!(item.vis, syn::Visibility::Public(_)) => {
                defined.push(item.sig.ident.to_string());
            }
            Item::Struct(item) if matches!(item.vis, syn::Visibility::Public(_)) => {
                defined.push(item.ident.to_string());
            }
            Item::Enum(item) if matches!(item.vis, syn::Visibility::Public(_)) => {
                defined.push(item.ident.to_string());
            }
            Item::Type(item) if matches!(item.vis, syn::Visibility::Public(_)) => {
                defined.push(item.ident.to_string());
            }
            Item::Trait(item) if matches!(item.vis, syn::Visibility::Public(_)) => {
                defined.push(item.ident.to_string());
            }
            _ => {}
        }
    }
    (defined, re_exported)
}

#[test]
fn the_modules_the_prelude_takes_names_from_define_them_and_re_export_no_backend_item() {
    for (module, file) in OWN_MODULES {
        let (defined, re_exported) = module_items(file);
        for export in &re_exported {
            assert!(
                !export.from.contains("dioxus") && !export.from.contains("__private"),
                "{file} re-exports `{}` from `{}`: wrap it under an Outou name instead \
                 (ADR 0010, A5 amendment)",
                export.name,
                export.from
            );
        }
        for export in prelude_exports() {
            if export.from.starts_with(module) {
                assert!(
                    defined.contains(&export.name),
                    "the prelude takes `{}` from `{module}`, which does not define it \
                     (a `pub use` there could swap in a backend item)",
                    export.name
                );
            }
        }
    }
}
