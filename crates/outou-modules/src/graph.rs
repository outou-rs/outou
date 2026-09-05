//! The resolved module graph and its nodes.

use std::path::{Path, PathBuf};

use outou_syntax::Span;

/// Stem used for the crate root's generated file (issue #7 decision 3):
/// `src/.generated/crate-root.rs`, regardless of whether the root file is
/// named `main.rs`/`main.rsx` or `lib.rs`/`lib.rsx`. A fixed stem — rather
/// than the root file's own stem — is what makes a child module later
/// named `main` (`mod main { ... }`) collide with nothing: the root and a
/// child named `main` now generate to different files.
pub const GENERATED_ROOT_STEM: &str = "crate-root";

/// Kind of source file a module lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// Goes through the Outou front end and codegen.
    Rsx,
    /// Plain Rust, copied or referenced as is.
    Rust,
}

/// One module in the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleNode {
    /// Path from the crate root, e.g. `["components", "user"]`. Empty for
    /// the crate root itself. Kept in source spelling (`r#type`, not
    /// `type`) since this is the module's actual Rust path — codegen must
    /// re-emit `mod r#type;` unchanged.
    pub path: Vec<String>,
    /// File that defines the module, relative to the crate root (the
    /// directory containing `src/`), e.g. `src/components/user.rsx`. For
    /// an inline module (`mod m { ... }`) this is the file the inline
    /// block is written in, since an inline module has no file of its
    /// own.
    pub file: PathBuf,
    /// Whether the file is `.rsx` or `.rs`. An inline module inherits the
    /// kind of the file it is written in.
    pub kind: SourceKind,
    /// File containing this module's own `mod` declaration, relative to
    /// the crate root. `None` only for the crate root, which has no
    /// declaring `mod` item.
    pub declared_in: Option<PathBuf>,
    /// The module's visibility/qualifier prefix (`pub`, `pub(crate)`, …),
    /// verbatim, if any. `None` for a bare `mod name;` and for the crate
    /// root.
    pub visibility: Option<String>,
    /// Whether this module was declared inline (`mod m { ... }`) rather
    /// than as a separate file. `false` for the crate root.
    pub is_inline: bool,
    /// `#[cfg(…)]` attributes to preserve on the generated declaration,
    /// verbatim, in source order. Never evaluated (ADR 0006). Empty for
    /// the crate root.
    pub cfg: Vec<String>,
    /// Every attribute on the `mod` declaration, verbatim, in source
    /// order (a superset of `cfg`) — codegen needs these to reproduce the
    /// original attributes on the generated `#[path = "…"] mod x;`
    /// declaration. Empty for the crate root.
    pub attributes: Vec<String>,
    /// Span of the whole `mod` item in its declaring file. `None` for the
    /// crate root, which has no declaring `mod` item.
    pub span: Option<Span>,
    /// Segments used to build [`ModuleNode::generated_path`]: the
    /// crate-relative module path, but with raw identifiers unraw'd and
    /// disambiguated when a sibling under the same parent shares the same
    /// name (issue #7 decision 3) — e.g. two `cfg`-exclusive `mod imp;`
    /// declarations become `["imp"]` and `["imp-1"]`. Empty for the crate
    /// root, whose generated file uses [`GENERATED_ROOT_STEM`] instead.
    pub generated: Vec<String>,
    /// Child modules, in source order.
    pub children: Vec<ModuleNode>,
}

impl ModuleNode {
    /// The generated file path for this module, under `src_dir`, per
    /// ADR 0009 layout (b) (`src/.generated/` + `#[path]`).
    ///
    /// The crate root (empty [`ModuleNode::generated`]) always generates
    /// to `src/.generated/{GENERATED_ROOT_STEM}.rs`, regardless of the
    /// root file's own name. Every other module mirrors
    /// [`ModuleNode::generated`] joined by `/`: `["components"]` becomes
    /// `src/.generated/components.rs`, `["components", "user"]` becomes
    /// `src/.generated/components/user.rs`. `generated` is computed once,
    /// during resolution — this method does no unraw'ing or
    /// disambiguation of its own.
    pub fn generated_path(&self, src_dir: &Path) -> PathBuf {
        let generated_dir = src_dir.join(".generated");
        if self.generated.is_empty() {
            return generated_dir.join(format!("{GENERATED_ROOT_STEM}.rs"));
        }
        let mut out = generated_dir;
        for segment in &self.generated {
            out.push(segment);
        }
        out.set_extension("rs");
        out
    }

    /// This module's attributes with the `#[path = "…"]` attribute
    /// removed, in source order. Codegen replaces, never reproduces, the
    /// original `#[path]` (issue #7 decision 4): it always writes its own
    /// `#[path = ".generated/…"]` pointing at [`ModuleNode::generated_path`],
    /// so reproducing the original attribute verbatim alongside it would
    /// leave two `#[path]` attributes on one declaration — and rustc
    /// takes the first, silently feeding the raw `.rsx` source to itself
    /// (issue #7 finding 4's `TwoPath` case) whenever codegen's own
    /// attribute is not the one written first.
    pub fn attributes_without_path(&self) -> impl Iterator<Item = &str> {
        self.attributes
            .iter()
            .filter(|text| outou_syntax::parser::attribute_meta_path(text) != Some("path"))
            .map(String::as_str)
    }
}

/// The resolved module graph of one crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleGraph {
    /// The crate root (`main.rs`, `main.rsx`, `lib.rs` or `lib.rsx`).
    pub root: ModuleNode,
}

impl ModuleGraph {
    /// Iterates every node in the graph, depth-first, pre-order, starting
    /// with the root. Sibling order matches source order.
    pub fn iter(&self) -> ModuleGraphIter<'_> {
        ModuleGraphIter {
            stack: vec![&self.root],
        }
    }

    /// Iterates every node whose [`SourceKind`] is [`SourceKind::Rsx`].
    ///
    /// This includes inline `.rsx` modules — a node whose own generated
    /// compile unit is its *parent's* file, not its own (see
    /// [`ModuleGraph::generated_units`] for the codegen-unit iterator
    /// that excludes them).
    pub fn rsx_files(&self) -> impl Iterator<Item = &ModuleNode> {
        self.iter().filter(|node| node.kind == SourceKind::Rsx)
    }

    /// Iterates every node that is its own generated compile unit: an
    /// `.rsx` module that is *not* inline (issue #7 decision 4). An inline
    /// module (`mod m { ... }`) has no file of its own — its `.rsx`
    /// content lives inside its parent's file — so driving codegen off
    /// [`ModuleGraph::rsx_files`] would emit one generated file per inline
    /// module from the *whole* of the parent file it is written in,
    /// duplicating that parent's own generated unit.
    pub fn generated_units(&self) -> impl Iterator<Item = &ModuleNode> {
        self.iter()
            .filter(|node| node.kind == SourceKind::Rsx && !node.is_inline)
    }
}

/// Depth-first, pre-order iterator over a [`ModuleGraph`]'s nodes.
///
/// Returned by [`ModuleGraph::iter`].
pub struct ModuleGraphIter<'a> {
    stack: Vec<&'a ModuleNode>,
}

impl<'a> Iterator for ModuleGraphIter<'a> {
    type Item = &'a ModuleNode;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.stack.pop()?;
        for child in node.children.iter().rev() {
            self.stack.push(child);
        }
        Some(node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(path: &[&str], file: &str) -> ModuleNode {
        let generated: Vec<String> = path.iter().map(|s| s.to_string()).collect();
        ModuleNode {
            path: path.iter().map(|s| s.to_string()).collect(),
            file: PathBuf::from(file),
            kind: SourceKind::Rsx,
            declared_in: if path.is_empty() {
                None
            } else {
                Some(PathBuf::from(file))
            },
            visibility: None,
            is_inline: false,
            cfg: Vec::new(),
            attributes: Vec::new(),
            span: None,
            generated,
            children: Vec::new(),
        }
    }

    #[test]
    fn generated_path_root_uses_the_reserved_stem() {
        let root = leaf(&[], "src/lib.rsx");
        assert_eq!(
            root.generated_path(Path::new("src")),
            PathBuf::from("src/.generated/crate-root.rs")
        );
    }

    #[test]
    fn generated_path_for_a_child_mirrors_its_module_path() {
        let node = leaf(&["components", "user"], "src/components/user.rsx");
        assert_eq!(
            node.generated_path(Path::new("src")),
            PathBuf::from("src/.generated/components/user.rs")
        );
    }

    #[test]
    fn generated_path_disambiguates_duplicate_siblings() {
        let mut node = leaf(&["imp"], "src/windows.rsx");
        node.generated = vec!["imp-1".to_string()];
        assert_eq!(
            node.generated_path(Path::new("src")),
            PathBuf::from("src/.generated/imp-1.rs")
        );
    }

    #[test]
    fn generated_path_unraws_segments() {
        let mut node = leaf(&["r#type"], "src/type.rsx");
        node.generated = vec!["type".to_string()];
        assert_eq!(
            node.generated_path(Path::new("src")),
            PathBuf::from("src/.generated/type.rs")
        );
    }

    #[test]
    fn iter_is_depth_first_pre_order() {
        let button = leaf(&["components", "button"], "src/components/button.rs");
        let user = leaf(&["components", "user"], "src/components/user.rsx");
        let mut components = leaf(&["components"], "src/components.rsx");
        components.children = vec![button, user];
        let mut root = leaf(&[], "src/main.rsx");
        root.children = vec![components];
        let graph = ModuleGraph { root };

        let paths: Vec<Vec<String>> = graph.iter().map(|node| node.path.clone()).collect();
        assert_eq!(
            paths,
            vec![
                Vec::<String>::new(),
                vec!["components".to_string()],
                vec!["components".to_string(), "button".to_string()],
                vec!["components".to_string(), "user".to_string()],
            ]
        );
    }

    #[test]
    fn rsx_files_filters_by_kind() {
        let mut button = leaf(&["components", "button"], "src/components/button.rs");
        button.kind = SourceKind::Rust;
        let user = leaf(&["components", "user"], "src/components/user.rsx");
        let mut components = leaf(&["components"], "src/components.rsx");
        components.children = vec![button, user];
        let mut root = leaf(&[], "src/main.rsx");
        root.children = vec![components];
        let graph = ModuleGraph { root };

        let rsx: Vec<PathBuf> = graph.rsx_files().map(|node| node.file.clone()).collect();
        assert_eq!(
            rsx,
            vec![
                PathBuf::from("src/main.rsx"),
                PathBuf::from("src/components.rsx"),
                PathBuf::from("src/components/user.rsx"),
            ]
        );
    }

    #[test]
    fn generated_units_skips_inline_modules() {
        let mut inline_tests = leaf(&["components", "tests"], "src/components.rsx");
        inline_tests.is_inline = true;
        let mut components = leaf(&["components"], "src/components.rsx");
        components.children = vec![inline_tests];
        let mut root = leaf(&[], "src/main.rsx");
        root.children = vec![components];
        let graph = ModuleGraph { root };

        let units: Vec<PathBuf> = graph.generated_units().map(|n| n.file.clone()).collect();
        assert_eq!(
            units,
            vec![
                PathBuf::from("src/main.rsx"),
                PathBuf::from("src/components.rsx"),
            ]
        );
    }

    #[test]
    fn attributes_without_path_drops_only_the_path_attribute() {
        let mut node = leaf(&["platform"], "src/unix.rsx");
        node.attributes = vec![
            "#[cfg(unix)]".to_string(),
            "#[path = \"unix.rsx\"]".to_string(),
        ];
        let kept: Vec<&str> = node.attributes_without_path().collect();
        assert_eq!(kept, vec!["#[cfg(unix)]"]);
    }
}
