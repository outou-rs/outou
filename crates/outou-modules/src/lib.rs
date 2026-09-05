//! Resolves `mod name;` declarations across `.rs` and `.rsx` files.
//!
//! Outou resolves the module graph itself instead of leaving it to rustc,
//! because rustc does not know about `.rsx`. Rules:
//!
//! - Candidates for `mod foo;` are, in this order of *listing* (not of
//!   priority): `foo.rsx`, `foo.rs`, `foo/mod.rsx`, `foo/mod.rs`.
//! - More than one existing candidate is an error. There is no implicit
//!   priority; the user must remove the ambiguity ([`ModuleError::Ambiguous`],
//!   ADR 0006).
//! - `#[path = "…"]` is honored. A `.rsx` target goes through the Outou
//!   front end, a `.rs` target is plain Rust. Directory ownership for
//!   both implicit candidates and `#[path]` follows the rustc-validated
//!   model in [`scope`].
//! - `#[cfg(…)]` is not evaluated. Every module is generated and the
//!   attribute is preserved on the generated declaration; rustc decides.
//!   `#[cfg_attr(condition, path = "…")]` is the one unsupported
//!   exception ([`ModuleError::ConditionalPath`]): Phase 0 cannot decide
//!   which of a conditional path's targets to resolve without evaluating
//!   `cfg`.
//!
//! The resolver reuses [`outou_syntax::parse`] to find `mod` items in
//! both `.rs` and `.rsx` files — "there is one compiler" (`AGENTS.md`);
//! it does not run a second scanner over plain Rust.

mod error;
mod generated;
mod graph;
mod probe;
mod resolve;
mod scope;
#[cfg(test)]
mod testutil;

use std::path::{Path, PathBuf};

pub use error::ModuleError;
pub use graph::{ModuleGraph, ModuleGraphIter, ModuleNode, SourceKind, GENERATED_ROOT_STEM};
pub use resolve::{resolve, MAX_MODULE_DEPTH};

/// Candidate file patterns for `mod {name};`, where `{name}` is substituted.
pub const CANDIDATES: [&str; 4] = ["{name}.rsx", "{name}.rs", "{name}/mod.rsx", "{name}/mod.rs"];

/// Returns the candidate paths for `mod {name};` declared in `dir`.
///
/// `name` should already be unraw'd (issue #7 decision 5): callers
/// resolving an actual `mod r#type;` pass `"type"`, not `"r#type"`, since
/// no file on disk is ever spelled with a `r#` prefix.
pub fn candidates(dir: &Path, name: &str) -> Vec<PathBuf> {
    CANDIDATES
        .iter()
        .map(|pattern| dir.join(pattern.replace("{name}", name)))
        .collect()
}

/// Rewrites `path` relative to `base` when it is prefixed by it, so that
/// resolved paths and error messages read `src/foo.rsx` instead of an
/// absolute or working-directory-relative path. Falls back to `path`
/// unchanged when it is not under `base`.
pub(crate) fn relative_to(path: &Path, base: &Path) -> PathBuf {
    path.strip_prefix(base).unwrap_or(path).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_follow_the_documented_order() {
        let got = candidates(Path::new("src"), "components");
        let expected: Vec<PathBuf> = [
            "src/components.rsx",
            "src/components.rs",
            "src/components/mod.rsx",
            "src/components/mod.rs",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        assert_eq!(got, expected);
    }
}
