//! [`Workspace`]: the plan (`crate::plan`), the [`Registry`] every
//! position/location mapping goes through, and every unit
//! ([`super::GeneratedUnit`]/[`super::RsxDocument`]), generated in
//! [`Mode::Recovery`] — never [`Mode::Strict`]: the language server must
//! keep working while the file is half-typed, which is exactly what
//! recovery mode is for.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use outou_cli::build::emit::generate_unit;
use outou_codegen::Mode;
use outou_sourcemap::{LineIndex, Registry};

use super::units::{GeneratedUnit, RsxDocument};
use crate::plan::{self, Plan, PlannedUnit, Resolved};
use crate::uri;

/// Everything this server knows about one crate root.
pub struct Workspace {
    /// The crate's manifest directory (containing `Cargo.toml` and `src/`).
    pub manifest_dir: PathBuf,
    /// `Some` unless the crate has no `.rsx` crate root (degraded mode).
    pub plan: Option<Plan>,
    /// Reverse-mapping registry: generated URI -> source map -> `.rsx` URIs.
    pub registry: Registry,
    /// Every known `.rsx` document, keyed by its URI string.
    pub rsx: HashMap<String, RsxDocument>,
    /// Every generated unit, keyed by its generated URI string.
    pub generated: HashMap<String, GeneratedUnit>,
    /// `.rsx` URI string -> generated URI string, for the common case of
    /// mapping a document the editor just edited to its overlay.
    pub rsx_to_generated: HashMap<String, String>,
}

/// Errors from [`Workspace::load`].
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// Planning the crate failed.
    #[error(transparent)]
    Plan(#[from] outou_cli::build::plan::PlanError),
    /// A planned unit's source file could not be read.
    #[error("reading `{}`: {source}", path.display())]
    ReadSource {
        /// The file that could not be read.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// Recovery-mode generation failed for a unit. This should not
    /// normally happen — [`Mode::Recovery`] never rejects a file for
    /// syntax errors — but the backend can still refuse an unsupported
    /// construct.
    #[error(transparent)]
    Emit(#[from] outou_cli::build::EmitError),
}

/// What [`Workspace::load`] found at a given root.
pub enum LoadOutcome {
    /// A `.rsx` crate root was found and every unit generated.
    Planned(Box<Workspace>),
    /// No `.rsx` crate root exists; the caller runs in degraded mode
    /// (Outou syntax diagnostics only, no rust-analyzer).
    Degraded {
        /// The crate directory that was searched, canonicalized.
        manifest_dir: PathBuf,
    },
}

impl Workspace {
    /// Resolves `manifest_dir` and, unless the crate has no `.rsx` root
    /// (degraded mode), generates every unit in [`Mode::Recovery`] and
    /// builds the initial [`Registry`].
    pub fn load(manifest_dir: &std::path::Path) -> Result<LoadOutcome, LoadError> {
        match plan::resolve(manifest_dir)? {
            Resolved::Degraded { manifest_dir } => Ok(LoadOutcome::Degraded { manifest_dir }),
            Resolved::Planned { manifest_dir, plan } => {
                let mut workspace = Self {
                    manifest_dir,
                    plan: Some(plan.clone()),
                    registry: Registry::new(),
                    rsx: HashMap::new(),
                    generated: HashMap::new(),
                    rsx_to_generated: HashMap::new(),
                };
                for unit in &plan.units {
                    workspace.load_unit(unit)?;
                }
                Ok(LoadOutcome::Planned(Box::new(workspace)))
            }
        }
    }

    /// Generates one unit fresh from disk and installs it (used at load
    /// time and whenever a full re-plan is required).
    fn load_unit(&mut self, unit: &PlannedUnit) -> Result<(), LoadError> {
        let source =
            fs::read_to_string(&unit.source_file).map_err(|source| LoadError::ReadSource {
                path: unit.source_file.clone(),
                source,
            })?;
        let generated = generate_unit(unit, &source, Mode::Recovery)?;

        let rsx_uri = outou_sourcemap::file_uri(&unit.source_file);
        let generated_uri = outou_sourcemap::file_uri(&unit.generated_file);
        let declared_modules = plan::declared_modules(&outou_syntax::parse(&source).file);

        let line_index = LineIndex::new(&generated.rust);
        self.rsx
            .insert(rsx_uri.as_str().to_string(), RsxDocument::new(source, 0));
        self.generated.insert(
            generated_uri.as_str().to_string(),
            GeneratedUnit {
                planned: unit.clone(),
                generated_uri: generated_uri.clone(),
                rsx_uri: rsx_uri.clone(),
                text: generated.rust,
                line_index,
                ra_version: 0,
                last_ra_diagnostics: Vec::new(),
                declared_modules,
            },
        );
        self.rsx_to_generated.insert(
            rsx_uri.as_str().to_string(),
            generated_uri.as_str().to_string(),
        );
        self.registry = std::mem::take(&mut self.registry).with_map(generated.source_map);
        Ok(())
    }

    /// Whether editing `rsx_uri` with `new_text` changes the declared
    /// module set of its own unit (see [`plan::declared_modules`]).
    /// `false` when `rsx_uri` is not a known unit at all (nothing to
    /// compare against; treated conservatively as "no shape change" so
    /// the caller falls back to single-unit regeneration).
    pub fn module_shape_changed(&self, rsx_uri: &str, new_text: &str) -> bool {
        let Some(generated_uri) = self.rsx_to_generated.get(rsx_uri) else {
            return false;
        };
        let Some(unit) = self.generated.get(generated_uri) else {
            return false;
        };
        let new_modules = plan::declared_modules(&outou_syntax::parse(new_text).file);
        new_modules != unit.declared_modules
    }

    /// Re-resolves the crate root and regenerates every unit, preferring
    /// each currently open document's own in-memory buffer over its
    /// contents on disk (issue #9 Gate 3 review, M2/HIGH-2): planning
    /// used to always re-read every file from disk, so a `mod`
    /// declaration typed into an unsaved buffer never entered the module
    /// graph at all, and saving then wrote a generated root with the new
    /// `mod` line but no matching `#[path]` — non-compiling Rust. See
    /// [`Self::build_overlay`] for exactly which buffers are used.
    ///
    /// TODO(phase0): if [`Self::load_unit`]/[`Self::load_unit_with_text`]
    /// fails for one unit *after* `self.generated`/`self.rsx` have
    /// already been cleared below, this returns `Err` with the workspace
    /// left emptied rather than restored to its pre-`replan` state. Not
    /// reachable by any known input in Phase 0 ([`Mode::Recovery`] does
    /// not reject syntax errors, so this can only fire for
    /// [`LoadError::ReadSource`]/[`LoadError::Emit`] on a still-unsupported
    /// construct) and out of scope for issue #9's M2 fix; a fully
    /// transactional re-plan (compute the new state, then swap it in only
    /// on success) is the eventual fix.
    pub fn replan(&mut self, rsx_uri: &str, override_text: &str) -> Result<(), LoadError> {
        let overlay = self.build_overlay(rsx_uri, override_text);

        match plan::resolve_with_overlay(&self.manifest_dir, &overlay)? {
            Resolved::Degraded { .. } => {
                // The edit made the crate root itself disappear (or
                // ambiguous); there is nothing left to plan. Keep the
                // previous state rather than discarding it silently.
                Ok(())
            }
            Resolved::Planned { plan, .. } => {
                self.plan = Some(plan.clone());
                self.registry = Registry::new();
                self.generated.clear();
                self.rsx_to_generated.clear();
                let previous_versions: HashMap<String, i32> = std::mem::take(&mut self.rsx)
                    .into_iter()
                    .map(|(uri, doc)| (uri, doc.version))
                    .collect();
                for unit in &plan.units {
                    match overlay_text_for(&overlay, &unit.source_file) {
                        Some(text) => self.load_unit_with_text(unit, text)?,
                        None => self.load_unit(unit)?,
                    }
                }
                for (uri, doc) in self.rsx.iter_mut() {
                    if let Some(v) = previous_versions.get(uri) {
                        doc.version = *v;
                    }
                }
                Ok(())
            }
        }
    }

    /// Builds the buffer overlay a re-plan should use: every currently
    /// *open* `.rsx` document's text (`RsxDocument::version != 0` — see
    /// that field's own doc comment: `0` means "read from disk, never
    /// opened"), plus `rsx_uri`'s own `override_text`, which is not yet
    /// reflected in `self.rsx` at the point [`Self::replan`] is called
    /// (the caller passes the edit's new text before storing it). Keys
    /// are canonicalized where possible so they line up with
    /// `outou_modules::resolve_with_overlay`'s own lookup identity.
    /// TODO(phase0) (issue #9 Gate 3 review, HIGH-2's SKIP item): only
    /// currently *open* documents (`RsxDocument::version != 0`) are
    /// overlaid; a file edited by some other tool while this server is
    /// running, without ever being opened in this editor session, is
    /// still read from disk. A full workspace virtual-filesystem
    /// abstraction (watching every `.rsx` file for external changes, not
    /// only editor buffers) is out of scope for Phase 0's LSP feasibility
    /// question.
    fn build_overlay(&self, rsx_uri: &str, override_text: &str) -> HashMap<PathBuf, String> {
        let mut overlay = HashMap::new();
        for (doc_uri, doc) in &self.rsx {
            if doc.version != 0 {
                if let Some(path) = uri::outou_uri_str_to_path(doc_uri) {
                    overlay.insert(
                        canonical_or_lexical(&path),
                        doc.line_index.text().to_string(),
                    );
                }
            }
        }
        if let Some(path) = uri::outou_uri_str_to_path(rsx_uri) {
            overlay.insert(canonical_or_lexical(&path), override_text.to_string());
        }
        overlay
    }

    fn load_unit_with_text(&mut self, unit: &PlannedUnit, text: &str) -> Result<(), LoadError> {
        let generated = generate_unit(unit, text, Mode::Recovery)?;
        let rsx_uri = outou_sourcemap::file_uri(&unit.source_file);
        let generated_uri = outou_sourcemap::file_uri(&unit.generated_file);
        let declared_modules = plan::declared_modules(&outou_syntax::parse(text).file);

        self.rsx.insert(
            rsx_uri.as_str().to_string(),
            RsxDocument::new(text.to_string(), 0),
        );
        self.generated.insert(
            generated_uri.as_str().to_string(),
            GeneratedUnit {
                planned: unit.clone(),
                generated_uri: generated_uri.clone(),
                rsx_uri: rsx_uri.clone(),
                text: generated.rust.clone(),
                line_index: LineIndex::new(&generated.rust),
                ra_version: 0,
                last_ra_diagnostics: Vec::new(),
                declared_modules,
            },
        );
        self.rsx_to_generated.insert(
            rsx_uri.as_str().to_string(),
            generated_uri.as_str().to_string(),
        );
        self.registry = std::mem::take(&mut self.registry).with_map(generated.source_map);
        Ok(())
    }

    /// Regenerates the single unit for `rsx_uri` from `new_text`, updating
    /// the document, the generated text/line index and the registry.
    /// Returns the regenerated unit's generated URI, or `None` if
    /// `rsx_uri` is not a known unit.
    pub fn regenerate(
        &mut self,
        rsx_uri: &str,
        new_text: &str,
        version: i32,
    ) -> Result<Option<String>, LoadError> {
        let Some(generated_uri) = self.rsx_to_generated.get(rsx_uri).cloned() else {
            return Ok(None);
        };
        let planned = self
            .generated
            .get(&generated_uri)
            .map(|u| u.planned.clone());
        let Some(planned) = planned else {
            return Ok(None);
        };

        let generated = generate_unit(&planned, new_text, Mode::Recovery)?;
        let declared_modules = plan::declared_modules(&outou_syntax::parse(new_text).file);

        self.rsx.insert(
            rsx_uri.to_string(),
            RsxDocument::new(new_text.to_string(), version),
        );
        if let Some(unit) = self.generated.get_mut(&generated_uri) {
            unit.text = generated.rust.clone();
            unit.line_index = LineIndex::new(&generated.rust);
            unit.declared_modules = declared_modules;
            // The diagnostics rust-analyzer last published were for the
            // *previous* generated text; keeping them around republished
            // stale rustc errors for lines the current buffer no longer
            // has (issue #9 Gate 3 review, M6/HIGH-4, confirmed live: a
            // fixed type error kept re-publishing the same five stale
            // diagnostics for 3+ seconds after the buffer was corrected,
            // clearing only on an unrelated `mod` edit). A fresh
            // `publishDiagnostics` from rust-analyzer for the new text
            // will repopulate this; until then, none is more honest than
            // stale.
            unit.last_ra_diagnostics = Vec::new();
        }
        self.registry = std::mem::take(&mut self.registry).with_map(generated.source_map);
        Ok(Some(generated_uri))
    }
}

/// Canonicalizes `path` for overlay-key identity, falling back to the
/// lexical path unchanged when canonicalization fails (mirrors
/// `outou_modules::resolve`'s own `canonicalize_or_lexical`, which this
/// must agree with for a planning overlay entry to actually be found).
fn canonical_or_lexical(path: &std::path::Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Looks up `source_file` in `overlay`, trying its canonical identity
/// first and falling back to a raw lexical match.
fn overlay_text_for<'a>(
    overlay: &'a HashMap<PathBuf, String>,
    source_file: &std::path::Path,
) -> Option<&'a str> {
    overlay
        .get(&canonical_or_lexical(source_file))
        .or_else(|| overlay.get(source_file))
        .map(String::as_str)
}

#[cfg(test)]
mod tests;
