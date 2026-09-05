# outou-codegen

The `Backend` trait and the strict (`cargo build`) / recovery (IDE) generation modes.
One compiler serves both the build and the language server; output is deterministic.

Phase 0: Week 4 (Gate 2).

## Public API

```rust,ignore
pub enum Mode { Strict, Recovery }

pub struct GenerateOptions {
    pub generated_uri: Uri,
    pub source_uri: Uri,
    pub module_paths: BTreeMap<String, String>, // module_paths_key(name, own #[path] text) -> `#[path]` value
}

pub fn module_paths_key(name: &str, own_path_attribute: Option<&str>) -> String;

pub struct Generated { pub rust: String, pub source_map: SourceMap }

pub enum Error {
    SyntaxErrors { diagnostics: Vec<outou_syntax::Diagnostic> },
    Unsupported { backend: &'static str, message: String },
}

pub trait Backend {
    fn name(&self) -> &'static str;
    fn generate(&self, parsed: &outou_syntax::Parsed, source: &str, mode: Mode, opts: &GenerateOptions) -> Result<Generated, Error>;
}

pub fn generate(backend: &dyn Backend, parsed: &outou_syntax::Parsed, source: &str, mode: Mode, opts: &GenerateOptions) -> Result<Generated, Error>;
pub fn reject_syntax_errors_in_strict_mode(parsed: &outou_syntax::Parsed, mode: Mode) -> Result<(), Error>;

pub struct Writer { /* ... */ }
impl Writer {
    pub fn new(generated_uri: Uri, source_uri: Uri) -> Self;
    pub fn source(&self) -> SourceId;
    pub fn offset(&self) -> u32;
    pub fn raw(&mut self, text: &str) -> &mut Self;
    pub fn mapped(&mut self, text: &str, sources: &[Span], kind: MappingKind, label: Option<String>);
    pub fn verbatim(&mut self, source: &str, span: Span, kind: MappingKind, label: Option<String>);
    pub fn finish(self) -> (String, SourceMap);
}
```

## Rules

- A backend receives the Outou AST (`outou_syntax::ast`), never a backend-specific AST, and the `source` text `parsed` was parsed from (needed to splice Rust verbatim).
- `Mode::Strict` (`cargo build`): a backend calls `reject_syntax_errors_in_strict_mode` first, so it never has to lower an `ast::ErrorNode` itself — any error-severity diagnostic refuses generation with `Error::SyntaxErrors`, carrying every such diagnostic.
- `Mode::Recovery` (the IDE): error nodes are replaced with placeholders (backend-defined; see `outou-backend-dioxus/README.md`) so rust-analyzer can keep analyzing the rest of the file.
- `GenerateOptions` steers one `generate` call, which lowers exactly one `.rsx` source into exactly one generated Rust file; a multi-file crate calls `generate` once per module (`outou-modules`' `ModuleGraph::generated_units`). `module_paths` maps a lookup key ([`module_paths_key`]) to the `#[path]` value a declaration should get; a module whose key is absent from the map is emitted verbatim, unchanged (the common case for an inline module, or before the module graph has been resolved into paths). The key is the module's bare name, except when the module already carries its own explicit `#[path]` attribute (in source, before rewriting) — appending that attribute's verbatim text disambiguates the `cfg`-exclusive `mod imp;` idiom, where two sibling declarations legally share a name and each needs a different rewritten path (issue #8; `crates/outou-cli`'s planner computes the same key from `ModuleNode::attributes`, `outou-backend-dioxus`'s `lower_module` computes it from `ast::Module::attributes` — both sides derive it from the identical source text).
- `Writer` is the shared "emit" infrastructure every backend reuses instead of hand-rolling its own: `raw` appends synthesized text with no mapping, `mapped`/`verbatim` append text and record a byte-accurate `SourceMapBuilder` mapping in the same call, and `finish` returns the generated text plus the finished `SourceMap`.
- Generation is deterministic: the same source, compiler version, configuration and backend always yield byte-identical Rust and an identical source map (verified for `outou-backend-dioxus` under `crates/outou-backend-dioxus/tests/golden.rs` and `tests/recovery.rs`).
