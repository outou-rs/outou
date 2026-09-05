//! In-memory state: every `.rsx` document this server knows about and the
//! generated Rust unit overlaid onto rust-analyzer for it.
//!
//! There is one [`Workspace`] per `initialize`d root. It owns the plan
//! (`crate::plan`), the [`Registry`] every position/location mapping goes
//! through, and generates every unit in [`Mode::Recovery`] — never
//! [`Mode::Strict`]: the language server must keep working while the file
//! is half-typed, which is exactly what recovery mode is for.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::PathBuf;

use outou_cli::build::emit::generate_unit;
use outou_codegen::Mode;
use outou_sourcemap::{LineIndex, Registry, Uri as OutouUri};
use outou_syntax::Diagnostic as SyntaxDiagnostic;

use crate::plan::{self, Plan, PlannedUnit, Resolved};

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
    /// Module names declared directly in this unit's own `.rsx` text, at
    /// the time it was last (re)planned — see
    /// [`crate::plan::declared_module_names`].
    declared_modules: BTreeSet<String>,
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
        declared_modules: BTreeSet::new(),
    }
}

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
        let declared_modules = plan::declared_module_names(&outou_syntax::parse(&source).file);

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
    /// module set of its own unit (see [`plan::declared_module_names`]).
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
        let new_modules = plan::declared_module_names(&outou_syntax::parse(new_text).file);
        new_modules != unit.declared_modules
    }

    /// Re-resolves the crate root and regenerates every unit from disk
    /// text, except that `rsx_uri`'s own unit uses `override_text` instead
    /// (the editor's in-memory buffer, which may not be saved yet).
    pub fn replan(&mut self, rsx_uri: &str, override_text: &str) -> Result<(), LoadError> {
        let root_source = self
            .generated
            .get(
                self.rsx_to_generated
                    .get(rsx_uri)
                    .map(String::as_str)
                    .unwrap_or(""),
            )
            .map(|u| u.planned.source_file.clone());

        match plan::resolve(&self.manifest_dir)? {
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
                    if root_source.as_deref() == Some(unit.source_file.as_path()) {
                        self.load_unit_with_text(unit, override_text)?;
                    } else {
                        self.load_unit(unit)?;
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

    fn load_unit_with_text(&mut self, unit: &PlannedUnit, text: &str) -> Result<(), LoadError> {
        let generated = generate_unit(unit, text, Mode::Recovery)?;
        let rsx_uri = outou_sourcemap::file_uri(&unit.source_file);
        let generated_uri = outou_sourcemap::file_uri(&unit.generated_file);
        let declared_modules = plan::declared_module_names(&outou_syntax::parse(text).file);

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
        let declared_modules = plan::declared_module_names(&outou_syntax::parse(new_text).file);

        self.rsx.insert(
            rsx_uri.to_string(),
            RsxDocument::new(new_text.to_string(), version),
        );
        if let Some(unit) = self.generated.get_mut(&generated_uri) {
            unit.text = generated.rust.clone();
            unit.line_index = LineIndex::new(&generated.rust);
            unit.declared_modules = declared_modules;
        }
        self.registry = std::mem::take(&mut self.registry).with_map(generated.source_map);
        Ok(Some(generated_uri))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::Instant;

    /// Gate 3's fixed target program (`docs/phase0.md`, issue #9): a real,
    /// multi-file `.rsx` crate, not a synthetic fixture, so these tests
    /// exercise the same planner/codegen path the gate3 integration test
    /// (`tests/gate3.rs`) drives through the real LSP protocol.
    fn phase0_app_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/phase0-app")
            .canonicalize()
            .expect("examples/phase0-app exists")
    }

    #[test]
    fn loads_every_unit_of_the_gate3_fixture() {
        let outcome = Workspace::load(&phase0_app_dir()).expect("plans and generates");
        let LoadOutcome::Planned(workspace) = outcome else {
            panic!("examples/phase0-app has a `.rsx` crate root");
        };
        // `main.rsx` (crate root) and `components.rsx`.
        assert_eq!(workspace.generated.len(), 2);
        assert_eq!(workspace.rsx.len(), 2);
    }

    /// A single-file edit regenerates only that unit — issue #9's
    /// performance budget item ("a single-file edit never regenerates the
    /// whole crate") and the architecture note's own requirement.
    /// `components.rsx`'s generated text (and declared-module set) must be
    /// byte-identical before and after editing `main.rsx`.
    #[test]
    fn regenerating_one_unit_does_not_touch_another() {
        let outcome = Workspace::load(&phase0_app_dir()).expect("plans and generates");
        let LoadOutcome::Planned(mut workspace) = outcome else {
            panic!("examples/phase0-app has a `.rsx` crate root");
        };
        let main_rsx_uri = workspace
            .rsx
            .keys()
            .find(|uri| uri.ends_with("main.rsx"))
            .expect("main.rsx is a known unit")
            .clone();
        let components_generated_uri = workspace
            .generated
            .keys()
            .find(|uri| uri.ends_with("components.rs"))
            .expect("components.rs is a known unit")
            .clone();
        let components_text_before = workspace.generated[&components_generated_uri].text.clone();

        let new_main_text = workspace.rsx[&main_rsx_uri].line_index.text().to_string()
            + "\n// a trailing comment\n";
        workspace
            .regenerate(&main_rsx_uri, &new_main_text, 2)
            .expect("regenerating a syntactically valid edit succeeds");

        assert_eq!(
            workspace.generated[&components_generated_uri].text, components_text_before,
            "editing main.rsx must not change components.rs's generated text"
        );
    }

    /// Perf smoke test for issue #9's budget ("incremental `.rsx` ->
    /// generated Rust: perceived as instantaneous"): regenerating one
    /// small-to-medium real unit is a single `outou_syntax::parse` +
    /// `DioxusBackend::generate` call (`outou_cli::build::emit::generate_unit`),
    /// the same primitive `outou build` uses per file — no LSP-specific
    /// overhead beyond that. 50ms is a generous bound (typical runs are
    /// well under 1ms for a file this size); this exists to catch a
    /// catastrophic regression, not to pin an exact number.
    #[test]
    fn regenerating_one_unit_is_fast() {
        let outcome = Workspace::load(&phase0_app_dir()).expect("plans and generates");
        let LoadOutcome::Planned(mut workspace) = outcome else {
            panic!("examples/phase0-app has a `.rsx` crate root");
        };
        let main_rsx_uri = workspace
            .rsx
            .keys()
            .find(|uri| uri.ends_with("main.rsx"))
            .expect("main.rsx is a known unit")
            .clone();
        let text = workspace.rsx[&main_rsx_uri].line_index.text().to_string();

        let start = Instant::now();
        workspace
            .regenerate(&main_rsx_uri, &text, 2)
            .expect("regenerating succeeds");
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 50,
            "regenerating one unit took {elapsed:?}, expected well under 50ms"
        );
    }
}
