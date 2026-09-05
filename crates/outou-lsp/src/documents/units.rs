//! The per-document types [`workspace::Workspace`](super::Workspace)
//! owns: one `.rsx` source ([`RsxDocument`]) and one generated Rust unit
//! overlaid onto rust-analyzer ([`GeneratedUnit`]) for it.

#[cfg(test)]
use std::path::PathBuf;

use outou_sourcemap::{LineIndex, Uri as OutouUri};
use outou_syntax::Diagnostic as SyntaxDiagnostic;

use crate::plan::{ModuleDescriptor, PlannedUnit};

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
    /// The last `outou_syntax::parse` diagnostics for this text.
    pub diagnostics: Vec<SyntaxDiagnostic>,
}

impl RsxDocument {
    pub(crate) fn new(text: String, version: i32) -> Self {
        let line_index = LineIndex::new(&text);
        let diagnostics = outou_syntax::parse(&text).diagnostics;
        Self {
            line_index,
            version,
            diagnostics,
        }
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
    }
}
