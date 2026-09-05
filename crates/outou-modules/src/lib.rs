//! Resolves `mod name;` declarations across `.rs` and `.rsx` files.
//!
//! Outou resolves the module graph itself instead of leaving it to rustc,
//! because rustc does not know about `.rsx`. Rules:
//!
//! - Candidates for `mod foo;` are, in this order of *listing* (not of
//!   priority): `foo.rsx`, `foo.rs`, `foo/mod.rsx`, `foo/mod.rs`.
//! - More than one existing candidate is an error. There is no implicit
//!   priority; the user must remove the ambiguity.
//! - `#[path = "…"]` is honored. A `.rsx` target goes through the Outou
//!   front end, a `.rs` target is plain Rust.
//! - `#[cfg(…)]` is not evaluated. Every module is generated and the
//!   attribute is preserved on the generated declaration; rustc decides.

use std::path::{Path, PathBuf};

/// Candidate file patterns for `mod {name};`, where `{name}` is substituted.
pub const CANDIDATES: [&str; 4] = ["{name}.rsx", "{name}.rs", "{name}/mod.rsx", "{name}/mod.rs"];

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
    /// Path from the crate root, e.g. `["components", "user"]`.
    pub path: Vec<String>,
    /// File that defines the module.
    pub file: PathBuf,
    /// Whether the file is `.rsx` or `.rs`.
    pub kind: SourceKind,
    /// `#[cfg(…)]` attributes to preserve on the generated declaration.
    pub cfg: Vec<String>,
    /// Child modules.
    pub children: Vec<ModuleNode>,
}

/// The resolved module graph of one crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleGraph {
    /// The crate root (`main.rs`, `main.rsx`, `lib.rs` or `lib.rsx`).
    pub root: ModuleNode,
}

/// Resolution errors, phrased for the user.
#[derive(Debug, thiserror::Error)]
pub enum ModuleError {
    /// Several candidate files exist for one `mod` declaration.
    #[error("ambiguous Outou module `{name}`: {candidates:?} all exist; keep exactly one")]
    Ambiguous {
        /// Module name.
        name: String,
        /// Every existing candidate.
        candidates: Vec<PathBuf>,
    },
    /// No candidate file exists.
    #[error("file not found for module `{name}`; looked for {candidates:?}")]
    NotFound {
        /// Module name.
        name: String,
        /// Every candidate that was tried.
        candidates: Vec<PathBuf>,
    },
    /// I/O failure while reading a source file.
    #[error("cannot read `{path}`: {source}")]
    Io {
        /// Offending file.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
}

/// Returns the candidate paths for `mod {name};` declared in `dir`.
pub fn candidates(dir: &Path, name: &str) -> Vec<PathBuf> {
    CANDIDATES
        .iter()
        .map(|pattern| dir.join(pattern.replace("{name}", name)))
        .collect()
}

/// Resolves the module graph starting at `root` (a `main.rs`, `main.rsx`,
/// `lib.rs` or `lib.rsx`).
pub fn resolve(root: &Path) -> Result<ModuleGraph, ModuleError> {
    let _ = root;
    todo!("outou-modules: resolver is implemented in Phase 0, Week 4 (Gate 2)")
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
