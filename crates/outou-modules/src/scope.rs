//! Directory-ownership model for module resolution (issue #7 decision 2),
//! validated against rustc 1.98.1 by building the tree exercised by
//! `tests/fixtures/modules/path-dirs/` and confirming `cargo check` loads
//! exactly the files this model predicts, with every decoy file untouched.
//!
//! Two directories matter while resolving one file's items, bundled here
//! as one [`DirScope`]:
//!
//! - `dir` is the directory an explicit `#[path = "…"]` is *always*
//!   resolved against, regardless of `relative` — rustc's own rule:
//!   `#[path]` is relative to the file it is written in, never to any
//!   directory implied by module nesting.
//! - `relative`, when set, is the extra segment an *implicit* `mod name;`
//!   candidate search also descends into, on top of `dir`
//!   ([`DirScope::lookup_dir`]).
//!
//! The scope for a module's own children is built fresh at three points:
//!
//! - The crate root ([`DirScope::root`]): `{ dir: src/, relative: None }`.
//! - Opening a *file* for `mod name;` ([`DirScope::child_of_file`]): the
//!   new `dir` is always the opened file's own directory. Whether
//!   `relative` is set depends on how the file was found: a mod-rs-style
//!   match (`name/mod.rs`) or a file loaded via `#[path]` behaves like the
//!   crate root for its own children (`relative: None` — a path-loaded
//!   file is "mod-rs-like"); a plain-file match (`name.rs`) descends one
//!   level further for its own children (`relative: Some(name)`).
//! - Entering an *inline* `mod m { ... }` ([`DirScope::child_of_inline`]):
//!   without its own `#[path]`, `{ dir: lookup_dir.join(m), relative: None
//!   }`; with `#[path = p]` on the inline module itself, `{ dir:
//!   dir.join(p), relative: None }` — using the *current* scope's `dir`,
//!   not its `lookup_dir`.

use std::path::{Path, PathBuf};

/// Where an implicit `mod name;` candidate search and an explicit
/// `#[path]` target resolve, for one point in the module tree. See the
/// module doc for the three rules that build one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirScope {
    /// Directory an explicit `#[path = "…"]` is resolved against.
    pub(crate) dir: PathBuf,
    /// Extra segment an implicit `mod name;` candidate search descends
    /// into, on top of `dir`.
    pub(crate) relative: Option<String>,
}

impl DirScope {
    /// The scope at the crate root: candidate search starts directly in
    /// `src_dir`.
    pub(crate) fn root(src_dir: &Path) -> Self {
        DirScope {
            dir: src_dir.to_path_buf(),
            relative: None,
        }
    }

    /// Directory an implicit `mod name;` candidate search runs in:
    /// `dir.join(relative)` when set, `dir` otherwise.
    pub(crate) fn lookup_dir(&self) -> PathBuf {
        match &self.relative {
            Some(relative) => self.dir.join(relative),
            None => self.dir.clone(),
        }
    }

    /// The child scope after opening `target`, the file resolved for a
    /// `mod name;` (or `#[path]`-pinned) declaration. `unraw_name` is the
    /// module's own name with any `r#` prefix stripped (issue #7 decision
    /// 5); `is_mod_rs_style` is whether `target` was found as
    /// `name/mod.rs`/`name/mod.rsx` (or was reached via `#[path]`, which
    /// behaves the same way) rather than as a plain `name.rs`/`name.rsx`.
    pub(crate) fn child_of_file(target: &Path, unraw_name: &str, is_mod_rs_style: bool) -> Self {
        let dir = target
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(""));
        let relative = (!is_mod_rs_style).then(|| unraw_name.to_string());
        DirScope { dir, relative }
    }

    /// The child scope entered for an inline module (`mod m { ... }`)
    /// declared in this scope, given its own explicit `#[path]` target
    /// (`explicit_path`), if any.
    pub(crate) fn child_of_inline(&self, unraw_name: &str, explicit_path: Option<&str>) -> Self {
        let dir = match explicit_path {
            Some(target) => self.dir.join(target),
            None => self.lookup_dir().join(unraw_name),
        };
        DirScope {
            dir,
            relative: None,
        }
    }
}

/// Strips a leading `r#` raw-identifier prefix, e.g. `r#type` -> `type`
/// (issue #7 decision 5). Applied at exactly three places: candidate file
/// names, the inline/child directory segment (both here, via
/// `unraw_name`), and generated segments ([`crate::generated`]). Public so
/// that `outou-cli`'s planner (issue #8 fix list step 3) can apply the
/// exact same rule when computing an inline module's own generated-file
/// base directory, rather than re-deriving it.
/// [`crate::ModuleNode::path`] itself keeps the raw spelling — codegen
/// must re-emit `mod r#type;` unchanged — only filesystem and
/// generated-file segments derived from a name are ever unraw'd.
pub fn unraw(name: &str) -> &str {
    name.strip_prefix("r#").unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unraw_strips_the_leading_raw_prefix() {
        assert_eq!(unraw("r#type"), "type");
        assert_eq!(unraw("plain"), "plain");
    }

    #[test]
    fn dir_scope_root_looks_up_directly_in_src_dir() {
        let scope = DirScope::root(Path::new("src"));
        assert_eq!(scope.lookup_dir(), PathBuf::from("src"));
    }

    #[test]
    fn dir_scope_lookup_dir_joins_relative_when_set() {
        let scope = DirScope {
            dir: PathBuf::from("src"),
            relative: Some("plain".to_string()),
        };
        assert_eq!(scope.lookup_dir(), PathBuf::from("src/plain"));
    }

    #[test]
    fn dir_scope_child_of_plain_file_descends_one_level_further() {
        let child = DirScope::child_of_file(Path::new("src/plain.rs"), "plain", false);
        assert_eq!(child.dir, PathBuf::from("src"));
        assert_eq!(child.relative.as_deref(), Some("plain"));
        assert_eq!(child.lookup_dir(), PathBuf::from("src/plain"));
    }

    #[test]
    fn dir_scope_child_of_mod_rs_style_file_stays_at_its_own_directory() {
        let child = DirScope::child_of_file(Path::new("src/plain/mod.rs"), "plain", true);
        assert_eq!(child.dir, PathBuf::from("src/plain"));
        assert_eq!(child.relative, None);
        assert_eq!(child.lookup_dir(), PathBuf::from("src/plain"));
    }

    #[test]
    fn dir_scope_child_of_path_loaded_file_behaves_like_mod_rs() {
        // A `#[path]`-loaded file is "mod-rs-like": its own children look
        // up directly in its containing directory.
        let child = DirScope::child_of_file(Path::new("src/somewhere/other.rs"), "other", true);
        assert_eq!(child.dir, PathBuf::from("src/somewhere"));
        assert_eq!(child.relative, None);
    }

    #[test]
    fn dir_scope_child_of_inline_without_path_joins_the_lookup_dir() {
        let scope = DirScope {
            dir: PathBuf::from("src"),
            relative: Some("plain".to_string()),
        };
        let child = scope.child_of_inline("boxed", None);
        assert_eq!(child.dir, PathBuf::from("src/plain/boxed"));
        assert_eq!(child.relative, None);
    }

    #[test]
    fn dir_scope_child_of_inline_with_path_joins_dir_not_lookup_dir() {
        // The inline module's own `#[path]` is resolved against the
        // *current* file's directory (`dir`), not the lookup directory
        // that its un-annotated siblings would search in.
        let scope = DirScope {
            dir: PathBuf::from("src"),
            relative: Some("plain".to_string()),
        };
        let child = scope.child_of_inline("scoped", Some("tf"));
        assert_eq!(child.dir, PathBuf::from("src/tf"));
        assert_eq!(child.relative, None);
    }
}
