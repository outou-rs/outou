//! Generates Rust and a source map for every planned unit and writes both
//! atomically (write to a temporary file in the same directory, then
//! rename — so a reader, or a build racing a rebuild, never observes a
//! half-written file).
//!
//! This calls exactly the same two primitives (`outou_syntax::parse`,
//! `outou_backend_dioxus::DioxusBackend::generate`) that `cargo xtask
//! determinism`'s second, independent code path calls directly: "there is
//! one compiler" (`AGENTS.md`) — only the caller differs.

use std::fs;
use std::path::{Path, PathBuf};

use outou_backend_dioxus::DioxusBackend;
use outou_codegen::{Backend, GenerateOptions, Mode};
use outou_sourcemap::Uri;

use super::plan::{Plan, PlannedUnit};

/// Files written by one [`emit`] run.
#[derive(Debug, Clone, Default)]
pub struct EmitOutput {
    /// Generated `.rs` files, in plan order.
    pub generated_files: Vec<PathBuf>,
    /// Generated `.rs.map.json` files, in plan order.
    pub map_files: Vec<PathBuf>,
}

/// Errors from [`emit`]. Every message is Outou vocabulary: a syntax
/// failure renders `outou_syntax`'s own diagnostics, never the backend's.
#[derive(Debug, thiserror::Error)]
pub enum EmitError {
    /// Reading a `.rsx` source file failed.
    #[error("reading `{}`: {source}", path.display())]
    ReadSource {
        /// The file that could not be read.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// `.rsx` source contains syntax errors and generation is running in
    /// strict mode (`outou build`'s default, and the only mode that ever
    /// reaches `cargo build`).
    #[error("{path}: syntax errors prevent generation\n{rendered}", path = path.display())]
    SyntaxErrors {
        /// The offending `.rsx` file.
        path: PathBuf,
        /// Outou's own rendered diagnostics (never backend vocabulary).
        rendered: String,
    },
    /// The backend refused to lower a construct.
    #[error("{}: {backend}: {message}", path.display())]
    Unsupported {
        /// The `.rsx` file being generated.
        path: PathBuf,
        /// Backend name.
        backend: &'static str,
        /// Explanation in Outou vocabulary.
        message: String,
    },
    /// Building the JSON form of a generated unit's source map failed
    /// (only possible if this crate itself passed inconsistent inputs to
    /// `SourceMap::to_json` — a bug, not a user-facing failure mode).
    #[error("building source map for `{}`: {source}", path.display())]
    SourceMapJson {
        /// The generated `.rs` file the map is for.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: outou_sourcemap::ToJsonError,
    },
    /// Writing a generated file (or its temporary file, or renaming it
    /// into place) failed.
    #[error("writing `{}`: {source}", path.display())]
    Write {
        /// The file that could not be written.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
}

/// Generates every unit in `plan` and writes it (and its source map)
/// atomically under `plan.generated_dir`.
pub fn emit(plan: &Plan, mode: Mode) -> Result<EmitOutput, EmitError> {
    fs::create_dir_all(&plan.generated_dir).map_err(|source| EmitError::Write {
        path: plan.generated_dir.clone(),
        source,
    })?;

    let mut output = EmitOutput::default();
    for unit in &plan.units {
        emit_unit(unit, mode)?;
        output.generated_files.push(unit.generated_file.clone());
        output.map_files.push(unit.map_file.clone());
    }
    Ok(output)
}

/// Generates and writes exactly one unit. Exposed at crate visibility so
/// `cargo xtask determinism`'s independent second code path can reuse the
/// URI/error conventions without going through [`emit`]'s file-management
/// (it writes into its own separate directory tree).
pub(crate) fn emit_unit(unit: &PlannedUnit, mode: Mode) -> Result<(), EmitError> {
    let source = fs::read_to_string(&unit.source_file).map_err(|source| EmitError::ReadSource {
        path: unit.source_file.clone(),
        source,
    })?;

    let generated = generate_unit(unit, &source, mode)?;

    let map_json = generated
        .source_map
        .to_json(&generated.rust, &[&source])
        .map_err(|source| EmitError::SourceMapJson {
            path: unit.generated_file.clone(),
            source,
        })?;
    let map_text =
        serde_json::to_string_pretty(&map_json).expect("SourceMapJson always serializes") + "\n";

    atomic_write(&unit.generated_file, generated.rust.as_bytes())?;
    atomic_write(&unit.map_file, map_text.as_bytes())?;
    Ok(())
}

/// Runs `outou_syntax::parse` + `DioxusBackend::generate` for one unit,
/// without writing anything. Shared by [`emit_unit`] and by the
/// determinism check's independent second call site.
pub fn generate_unit(
    unit: &PlannedUnit,
    source: &str,
    mode: Mode,
) -> Result<outou_codegen::Generated, EmitError> {
    let parsed = outou_syntax::parse(source);
    let mut opts =
        GenerateOptions::new(file_uri(&unit.generated_file), file_uri(&unit.source_file));
    opts.module_paths = unit.module_paths.clone();

    DioxusBackend
        .generate(&parsed, source, mode, &opts)
        .map_err(|err| match err {
            outou_codegen::Error::SyntaxErrors { diagnostics } => EmitError::SyntaxErrors {
                path: unit.source_file.clone(),
                rendered: outou_syntax::render::render_diagnostics(
                    &diagnostics,
                    source,
                    &unit.source_file.display().to_string(),
                ),
            },
            outou_codegen::Error::Unsupported { backend, message } => EmitError::Unsupported {
                path: unit.source_file.clone(),
                backend,
                message,
            },
        })
}

/// A `file://` URI for an absolute path, e.g. `/a/b/c.rs` becomes
/// `file:///a/b/c.rs`. Used for `GenerateOptions::generated_uri`/
/// `source_uri`; determinism normalization strips this absolute prefix
/// back out before comparing two independently generated trees.
pub fn file_uri(path: &Path) -> Uri {
    Uri::new(format!("file://{}", path.display()))
}

/// Writes `contents` to `path` by first writing a sibling temporary file
/// and renaming it into place, so a reader never observes a partial
/// write and a failed write never corrupts a previously good file.
fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), EmitError> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir).map_err(|source| EmitError::Write {
        path: path.to_path_buf(),
        source,
    })?;
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp_path = dir.join(format!(".{file_name}.outou-tmp-{}", std::process::id()));

    fs::write(&tmp_path, contents).map_err(|source| EmitError::Write {
        path: tmp_path.clone(),
        source,
    })?;
    fs::rename(&tmp_path, path).map_err(|source| EmitError::Write {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn unit_for(source_file: PathBuf, generated_file: PathBuf) -> PlannedUnit {
        let mut map_file = generated_file.clone();
        map_file.set_extension("rs.map.json");
        PlannedUnit {
            module_path: Vec::new(),
            source_file,
            generated_file,
            map_file,
            module_paths: BTreeMap::new(),
        }
    }

    #[test]
    fn atomic_write_leaves_no_temp_file_behind() {
        let dir = std::env::temp_dir().join(format!(
            "outou-cli-emit-test-atomic-{}-{}",
            std::process::id(),
            line!()
        ));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("out.rs");

        atomic_write(&target, b"fn main() {}").expect("write succeeds");

        let entries: Vec<_> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap()).collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(fs::read_to_string(&target).unwrap(), "fn main() {}");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn generate_unit_reports_outou_vocabulary_only_on_syntax_error() {
        let dir = std::env::temp_dir().join(format!(
            "outou-cli-emit-test-syntax-error-{}-{}",
            std::process::id(),
            line!()
        ));
        fs::create_dir_all(&dir).unwrap();
        let source_file = dir.join("broken.rsx");
        fs::write(&source_file, "fn f() { <div cl").unwrap();
        let unit = unit_for(source_file, dir.join(".generated/broken.rs"));

        let source = fs::read_to_string(&unit.source_file).unwrap();
        let err = generate_unit(&unit, &source, Mode::Strict).expect_err("broken input errors");
        let rendered = err.to_string();

        fs::remove_dir_all(&dir).ok();

        for forbidden in [
            "rsx!",
            "PropsBuilder",
            "dioxus_rsx",
            "GeneratedNode",
            "dioxus",
        ] {
            assert!(
                !rendered.contains(forbidden),
                "rendered error must not mention {forbidden:?}: {rendered}"
            );
        }
        assert!(matches!(err, EmitError::SyntaxErrors { .. }));
    }
}
