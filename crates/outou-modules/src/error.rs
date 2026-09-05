//! Resolution errors.
//!
//! Every variant is phrased in Outou vocabulary and names the declaring
//! `mod` and the file(s) involved, per ADR 0006. Every variant also names
//! `declared_in` (the file whose text contains the `mod` declaration) and
//! `span` (the declaration's span in that file), so a caller can point the
//! user at the exact offending line (issue #7 fix list item 1).

use std::path::PathBuf;

use outou_syntax::Span;

/// Resolution errors, phrased for the user.
#[derive(Debug, thiserror::Error)]
pub enum ModuleError {
    /// Several candidate files exist for one `mod` declaration. There is
    /// no implicit priority between `.rs` and `.rsx`, or between
    /// `foo.rsx` and `foo/mod.rsx` (ADR 0006): the user removes one file.
    #[error("ambiguous Outou module `{name}` declared in `{}`: {candidates:?} all exist; keep exactly one", declared_in.display())]
    Ambiguous {
        /// Module name, as written in `mod {name};`.
        name: String,
        /// File containing the `mod` declaration, relative to the crate
        /// root.
        declared_in: PathBuf,
        /// Span of the `mod` declaration in `declared_in`.
        span: Span,
        /// Every existing candidate, in `CANDIDATES` order, relative to
        /// the crate root (the directory containing `src/`).
        candidates: Vec<PathBuf>,
    },
    /// No candidate file exists for a `mod name;` declaration, or the
    /// file named by `#[path = "…"]` does not exist.
    #[error("file not found for Outou module `{name}` declared in `{}`; looked for {candidates:?}", declared_in.display())]
    NotFound {
        /// Module name, as written in `mod {name};`.
        name: String,
        /// File containing the `mod` declaration, relative to the crate
        /// root.
        declared_in: PathBuf,
        /// Span of the `mod` declaration in `declared_in`.
        span: Span,
        /// Every candidate that was tried, relative to the crate root.
        candidates: Vec<PathBuf>,
    },
    /// I/O failure while reading a source file that a candidate search or
    /// `#[path]` resolved to.
    #[error("cannot read Outou module `{name}` at `{}`, declared in `{}`: {source}", path.display(), declared_in.display())]
    Io {
        /// Module name, as written in `mod {name};`.
        name: String,
        /// Offending file, relative to the crate root.
        path: PathBuf,
        /// File containing the `mod` declaration, relative to the crate
        /// root.
        declared_in: PathBuf,
        /// Span of the `mod` declaration in `declared_in`.
        span: Span,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// A `mod` declaration reachable through `#[path]` re-opens a file
    /// already open higher up the same chain (decision 1). Detected via
    /// canonical file identity (falling back to the lexical path when
    /// canonicalization fails), since Outou's own resolution recursion —
    /// unlike rustc's, which is bounded by ordinary filesystem nesting —
    /// can be driven arbitrarily deep by `#[path]` alone and previously
    /// aborted the process with a stack overflow instead of reporting an
    /// error.
    #[error("circular Outou module `{name}` declared in `{}`: {}", declared_in.display(), render_chain(chain))]
    Circular {
        /// Module name, as written in `mod {name};`.
        name: String,
        /// File containing the re-entering `mod` declaration, relative to
        /// the crate root.
        declared_in: PathBuf,
        /// Span of the `mod` declaration in `declared_in`.
        span: Span,
        /// The file chain from the first occurrence back to itself,
        /// relative to the crate root, in traversal order.
        chain: Vec<PathBuf>,
    },
    /// Module nesting (inline or across files) exceeded
    /// [`crate::MAX_MODULE_DEPTH`]. A cap independent of cycle detection is
    /// needed because canonical identity does not bound acyclic depth: a
    /// long, acyclic `#[path]` chain can still overflow the stack.
    #[error(
        "Outou module `{name}` declared in `{}` nests deeper than the limit of {limit} modules",
        declared_in.display()
    )]
    TooDeep {
        /// Module name, as written in `mod {name};`.
        name: String,
        /// File containing the `mod` declaration, relative to the crate
        /// root.
        declared_in: PathBuf,
        /// Span of the `mod` declaration in `declared_in`.
        span: Span,
        /// The depth limit that was exceeded.
        limit: usize,
    },
    /// A `#[cfg_attr(condition, path = "…")]` on a `mod` declaration:
    /// Phase 0 does not evaluate `cfg`, so it cannot decide which of the
    /// attribute's possible targets to resolve (ADR 0006's "`#[cfg(…)]` is
    /// not evaluated" extended to `cfg_attr`'s conditional `path`).
    #[error(
        "conditional module path is not supported: `{attribute}` on Outou module `{name}` declared in `{}`; write one `#[cfg]`-gated `mod` declaration per target with a direct `#[path = \"…\"]`",
        declared_in.display()
    )]
    ConditionalPath {
        /// Module name, as written in `mod {name};`.
        name: String,
        /// File containing the `mod` declaration, relative to the crate
        /// root.
        declared_in: PathBuf,
        /// Span of the `mod` declaration in `declared_in`.
        span: Span,
        /// The offending attribute, verbatim.
        attribute: String,
    },
    /// An explicit `#[path = "…"]` resolves to a file outside the crate
    /// root (an absolute path, or enough `..` segments to escape it).
    /// Phase 0 requires every module file to live under the crate root.
    #[error(
        "Outou module `{name}` declared in `{}` resolves to `{}`, outside the crate; Phase 0 requires module files under the crate root",
        declared_in.display(),
        path.display()
    )]
    OutsideCrate {
        /// Module name, as written in `mod {name};`.
        name: String,
        /// File containing the `mod` declaration, relative to the crate
        /// root.
        declared_in: PathBuf,
        /// Span of the `mod` declaration in `declared_in`.
        span: Span,
        /// The out-of-crate target, as written (not made relative, since
        /// it is not under the crate root).
        path: PathBuf,
    },
}

/// Renders a [`ModuleError::Circular`] chain as `a -> b -> c`.
fn render_chain(chain: &[PathBuf]) -> String {
    chain
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(" -> ")
}
