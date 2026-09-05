# outou-modules

Module graph resolver for crates mixing `.rs` and `.rsx`. Outou resolves the module graph itself instead of leaving it to rustc, because rustc does not know about `.rsx` (see `docs/adr/0006-ambiguous-module-is-an-error.md`).

## Rules

- Candidates for `mod foo;` are `foo.rsx`, `foo.rs`, `foo/mod.rsx`, `foo/mod.rs`. More than one existing *file* is `ModuleError::Ambiguous`; there is no implicit priority between `.rs` and `.rsx`, or between `foo.rsx` and `foo/mod.rsx` — the user removes one file. A directory that happens to share a candidate's name is never itself a candidate.
- `#[path = "…"]` is honored. Directory ownership — both for where an implicit `mod name;` searches and for where an explicit `#[path]` resolves — follows the `DirScope` model in `src/scope.rs`, validated directly against rustc 1.98.1 (see `tests/fixtures/modules/path-dirs/`). The extension of the resolved target decides `SourceKind`.
- `#[cfg(…)]` is never evaluated. Every module is resolved and generated; the attribute is preserved verbatim on `ModuleNode::cfg` (and, alongside every other attribute, on `ModuleNode::attributes`) so codegen can reproduce it on the generated declaration. `#[cfg_attr(condition, path = "…")]` is the one unsupported exception: Phase 0 cannot decide which of a conditional path's targets to resolve without evaluating `cfg`, so it is rejected with `ModuleError::ConditionalPath` instead of silently resolving the un-conditioned file.
- A `mod` reachable through `#[path]` that re-opens a file already open higher up the same chain is `ModuleError::Circular` instead of a stack overflow. Module nesting (crossing files) is additionally capped at `MAX_MODULE_DEPTH` so an acyclic but very long `#[path]` chain cannot overflow the stack either; exceeding it is `ModuleError::TooDeep`.
- An explicit `#[path]` that resolves outside the crate root (an absolute path, or enough `..` segments to escape it) is `ModuleError::OutsideCrate`: Phase 0 requires every module file to live under the crate root.
- Raw identifiers (`mod r#type;`) keep their source spelling on `ModuleNode::path` — codegen must re-emit `mod r#type;` unchanged — but every filesystem lookup and generated-path segment derived from the name strips the `r#` prefix, since no file on disk is ever spelled that way.

See the module doc on `scope` (`src/scope.rs`) for the full directory-ownership model.

## Public API

```rust,ignore
pub fn resolve(root: &Path) -> Result<ModuleGraph, ModuleError>;

pub const MAX_MODULE_DEPTH: usize;
pub const GENERATED_ROOT_STEM: &str; // "crate-root"

pub struct ModuleGraph { pub root: ModuleNode }
impl ModuleGraph {
    pub fn iter(&self) -> ModuleGraphIter<'_>;          // depth-first, pre-order, root first
    pub fn rsx_files(&self) -> impl Iterator<Item = &ModuleNode>;
    pub fn generated_units(&self) -> impl Iterator<Item = &ModuleNode>; // like rsx_files, but skips inline modules
}

pub struct ModuleNode {
    pub path: Vec<String>,             // e.g. ["components", "user"]; empty for the crate root; raw spelling kept
    pub file: PathBuf,                 // e.g. "src/components/user.rsx", relative to the crate root
    pub kind: SourceKind,              // Rsx | Rust
    pub declared_in: Option<PathBuf>,  // file containing this module's own `mod` item; None only for the crate root
    pub visibility: Option<String>,    // "pub", "pub(crate)", …, verbatim; None for a bare `mod name;`
    pub is_inline: bool,               // declared as `mod m { ... }` rather than a separate file
    pub cfg: Vec<String>,              // "#[cfg(…)]" attributes, verbatim
    pub attributes: Vec<String>,       // every attribute on the `mod` item, verbatim
    pub span: Option<Span>,            // the `mod` item's span; None for the crate root
    pub generated: Vec<String>,        // segments for generated_path, already unraw'd and disambiguated
    pub children: Vec<ModuleNode>,
}
impl ModuleNode {
    pub fn generated_path(&self, src_dir: &Path) -> PathBuf;
    pub fn attributes_without_path(&self) -> impl Iterator<Item = &str>;
}

pub enum SourceKind { Rsx, Rust }

pub enum ModuleError {
    Ambiguous { .. }, NotFound { .. }, Io { .. },
    Circular { .. }, TooDeep { .. },
    ConditionalPath { .. }, OutsideCrate { .. },
}

pub const CANDIDATES: [&str; 4];
pub fn candidates(dir: &Path, name: &str) -> Vec<PathBuf>;
```

All paths recorded on `ModuleGraph`/`ModuleNode` and inside `ModuleError` are relative to the crate root (the directory containing `src/`), not to the process's current directory. Every `ModuleError` variant carries `declared_in` (the file containing the offending `mod` item) and `span` (that item's span in `declared_in`).

## Generated-path convention (ADR 0009, layout (b))

Every module's generated file lives under `src/.generated/`. The crate root always uses the reserved stem `GENERATED_ROOT_STEM` (`"crate-root"`), regardless of whether the root file is named `main.rs`/`main.rsx` or `lib.rs`/`lib.rsx` — a fixed stem, rather than the root file's own name, is what lets a child module later named `main` (`mod main { ... }`) generate to a different file than the root. Every other module mirrors `ModuleNode::generated` joined by `/`: `["components"]` → `src/.generated/components.rs`, `["components", "user"]` → `src/.generated/components/user.rs`. `generated` is computed once, during resolution, from the module's path with raw identifiers unraw'd and — when a sibling under the same parent shares the same (unraw'd) name, as in the `cfg`-exclusive `mod imp;` idiom — disambiguated with a `-{n}` suffix in source order (`imp`, `imp-1`, `imp-2`, …), keeping the scheme injective without an error case. `ModuleNode::generated_path(src_dir)` computes the final path; it does no unraw'ing or disambiguation of its own.

Codegen is expected to emit, at the declaring site, `#[path = "<relative path to generated_path>"]` plus the module's own `cfg`/`attributes_without_path()` (never `attributes`, which still includes the original `#[path]` — codegen replaces, never reproduces it, since two `#[path]` attributes on one declaration would leave rustc using whichever is written first), so the original `#[cfg(…)]` (and any other non-`path` attribute) survives on the generated `mod` declaration. Because a file loaded via `#[path]` is mod-rs-like for its own children (see `scope`), a `#[path]` written *inside* a generated file resolves against `src/.generated/` the same way: `generated_units()` iterates exactly the modules codegen must emit one file per, skipping inline modules (which have no file of their own — see `rsx_files` vs `generated_units` above).

Phase 0: Week 4 (Gate 2).
