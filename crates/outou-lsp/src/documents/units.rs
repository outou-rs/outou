//! The per-document types [`workspace::Workspace`](super::Workspace)
//! owns: one `.rsx` source ([`RsxDocument`]) and one generated Rust unit
//! overlaid onto rust-analyzer ([`GeneratedUnit`]) for it.

#[cfg(test)]
use std::path::PathBuf;

use outou_sourcemap::{LineIndex, Uri as OutouUri};
use outou_syntax::Diagnostic as SyntaxDiagnostic;

use crate::plan::{ModuleDescriptor, PlannedUnit};

/// The version of the `textDocument/didOpen` a generated unit is opened
/// with on rust-analyzer (at startup, or when a re-plan adds the unit).
/// Every later `didChange` for that unit sends a higher one.
pub const INITIAL_RA_VERSION: i32 = 1;

/// One `.rsx` source file, whether or not the editor currently has it
/// open. Every planned unit's source is loaded from disk at startup so
/// cross-file definition into a file the editor has not opened yet still
/// has a [`LineIndex`] to map through.
pub struct RsxDocument {
    /// Byte offset <-> LSP position conversion, and the current text
    /// itself ([`LineIndex::text`]) — kept as a single copy rather than
    /// duplicated alongside it.
    pub line_index: LineIndex,
    /// LSP document version; `0` for a file only read from disk.
    pub version: i32,
    /// The `outou_syntax::parse` diagnostics for this text.
    pub diagnostics: Vec<SyntaxDiagnostic>,
    /// The `#[component]` functions this text declares, with their
    /// parameters (what tag-name and attribute-name completion offer, and what
    /// hover and definition at a prop name describe).
    /// Derived in [`Self::new`] from the same parse as [`Self::diagnostics`],
    /// together with [`Self::line_index`]: a document is never edited in
    /// place (an edit constructs a new one), so none of these can go stale
    /// (#31 L15). The parse tree itself is not kept: keeping it was measured
    /// once during that change at about thirty times the source text in
    /// resident memory.
    components: Vec<ComponentInfo>,
}

/// One `#[component]` function as completion needs to see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ComponentInfo {
    /// The function's name (the tag name that renders it).
    pub name: String,
    /// Its parameters, i.e. its props, in declaration order.
    pub params: Vec<crate::params::ParamInfo>,
}

#[cfg(test)]
thread_local! {
    /// How many times this thread has parsed `.rsx` text through
    /// [`parse_rsx`], i.e. every parse made by this crate itself (not the
    /// codegen's own, inside `generate_unit`); lets a test prove a path does
    /// not re-parse files.
    pub(crate) static PARSE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// `outou_syntax::parse` for the language server's own per-document and
/// per-request parses (counted under `cfg(test)`, see [`PARSE_COUNT`]).
///
/// Every parse inside this crate goes through here: `clippy.toml` bans
/// `outou_syntax::parse` everywhere else. The codegen's own parse inside
/// `outou_cli::build::emit::generate_unit` is outside this crate and not counted.
pub(crate) fn parse_rsx(text: &str) -> outou_syntax::Parsed {
    #[cfg(test)]
    PARSE_COUNT.with(|count| count.set(count.get() + 1));
    #[allow(clippy::disallowed_methods)]
    outou_syntax::parse(text)
}

impl RsxDocument {
    pub(crate) fn new(text: String, version: i32) -> Self {
        let line_index = LineIndex::new(&text);
        let parsed = parse_rsx(&text);
        let components = crate::plan::component_functions(&parsed.file)
            .into_iter()
            .map(|function| ComponentInfo {
                name: function.name.name.clone(),
                params: crate::params::parse_params(
                    &function.signature.text,
                    function.signature.span.start,
                ),
            })
            .collect();
        Self {
            line_index,
            version,
            diagnostics: parsed.diagnostics,
            components,
        }
    }

    /// The `#[component]` functions this document declares, in source order.
    pub fn components(&self) -> &[ComponentInfo] {
        &self.components
    }
}

/// One generated Rust file currently overlaid onto rust-analyzer.
pub struct GeneratedUnit {
    /// The plan's own record for this unit (paths, `module_paths`).
    pub planned: PlannedUnit,
    /// URI of the generated file, as sent to rust-analyzer.
    pub generated_uri: OutouUri,
    /// URI of the `.rsx` source this unit was generated from.
    pub rsx_uri: OutouUri,
    /// Current generated text.
    pub text: String,
    /// Byte offset <-> LSP position conversion for [`GeneratedUnit::text`].
    pub line_index: LineIndex,
    /// The `textDocument/didOpen`/`didChange` version last sent to
    /// rust-analyzer for this file.
    pub ra_version: i32,
    /// Raw (generated-position) diagnostics rust-analyzer last published
    /// for this file, kept so a `.rsx` edit can republish merged
    /// diagnostics without waiting on a fresh rust-analyzer round trip.
    pub last_ra_diagnostics: Vec<lsp_types::Diagnostic>,
    /// Modules declared directly in this unit's own `.rsx` text, at the
    /// time it was last (re)planned — see [`crate::plan::declared_modules`].
    /// `pub(super)`: [`super::workspace::Workspace`]'s planning methods
    /// read and update this across the module split.
    pub(super) declared_modules: Vec<ModuleDescriptor>,
    /// `span.start` of each module declared in the text this unit was last
    /// planned from, in the order of [`GeneratedUnit::declared_modules`].
    /// These are the keys of `planned.module_paths`; an edit that moves a
    /// declaration without changing the module set re-keys that map from
    /// here ([`crate::plan::rekey_module_paths`], #31 D4). Set only when
    /// the unit is planned, never by a single-unit regeneration.
    pub(crate) plan_module_spans: Vec<u32>,
}

#[cfg(test)]
pub(crate) fn test_generated_unit(
    generated_uri: &OutouUri,
    rsx_uri: &OutouUri,
    text: &str,
) -> GeneratedUnit {
    GeneratedUnit {
        planned: PlannedUnit {
            module_path: Vec::new(),
            source_file: PathBuf::new(),
            source_display: String::new(),
            generated_file: PathBuf::new(),
            map_file: PathBuf::new(),
            module_paths: Default::default(),
            inline_base_dirs: Vec::new(),
        },
        generated_uri: generated_uri.clone(),
        rsx_uri: rsx_uri.clone(),
        text: text.to_string(),
        line_index: LineIndex::new(text),
        ra_version: 0,
        last_ra_diagnostics: Vec::new(),
        declared_modules: Vec::new(),
        plan_module_spans: Vec::new(),
    }
}
