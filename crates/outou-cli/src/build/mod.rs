//! `outou build`: resolves the module graph, generates Rust for every
//! `.rsx` compile unit under `src/.generated/` (ADR 0009, layout (b)),
//! and removes stale generated files. Exposed as a plain library function
//! ([`build`]) rather than only through the `outou` binary, so `cargo
//! xtask determinism` and this crate's own tests can call it directly —
//! "there is one compiler" (`AGENTS.md`).
//!
//! Split into three small modules with one job each:
//! [`plan`] turns a resolved module graph into concrete output paths and
//! `#[path]` rewrites (no I/O beyond resolution itself); [`emit`]
//! generates and atomically writes each unit; [`clean`] removes stale
//! managed files [`emit`] did not just (re)write.

pub mod clean;
pub mod emit;
pub mod plan;

use std::collections::HashSet;
use std::path::PathBuf;

pub use emit::EmitError;
pub use plan::{CrateRoot, Plan, PlanError, PlannedUnit};

/// Options for one [`build`] run.
///
/// `outou build` always runs in [`outou_codegen::Mode::Strict`] — the only
/// mode `cargo build` may ever see (decision 3, issue #8 fix list step 1).
/// Recovery mode remains available directly through
/// [`emit::generate_unit`] for callers that need it (`outou-lsp`, and
/// `cargo xtask determinism`'s independent second code path); it was never
/// something `outou build` itself should have exposed as a flag, since a
/// Recovery-mode placeholder written into `src/.generated/` would silently
/// reach `cargo build`.
#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// Directory containing the crate's `Cargo.toml` and `src/`.
    pub manifest_dir: PathBuf,
}

impl BuildOptions {
    /// Options for a build of the crate at `manifest_dir`.
    pub fn new(manifest_dir: impl Into<PathBuf>) -> Self {
        Self {
            manifest_dir: manifest_dir.into(),
        }
    }
}

/// Outcome of a successful [`build`].
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// `false` when the crate has no `.rsx` crate root (only a plain
    /// `.rs` one, or none at all): "nothing to do".
    pub built: bool,
    /// Generated `.rs` files written, in plan order.
    pub generated_files: Vec<PathBuf>,
    /// Generated `.rs.map.json` files written, in plan order.
    pub map_files: Vec<PathBuf>,
    /// Stale managed files removed from `src/.generated/`.
    pub removed_files: Vec<PathBuf>,
}

/// Errors from [`build`].
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    /// Planning the build failed (module resolution, an ambiguous or
    /// mixed crate root, or a `.rs` file declaring a `.rsx` child).
    #[error(transparent)]
    Plan(#[from] PlanError),
    /// Generating or writing a unit failed.
    #[error(transparent)]
    Emit(#[from] EmitError),
    /// Removing a stale generated file failed.
    #[error("cleaning stale generated files under `{}`: {source}", dir.display())]
    Clean {
        /// The `.generated` directory being cleaned.
        dir: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
}

/// Runs `outou build` for the crate at `opts.manifest_dir`.
///
/// Returns `Ok(Report { built: false, .. })`, doing nothing else, when the
/// crate has no `.rsx` crate root — a plain `.rs` crate is not a failure.
pub fn build(opts: &BuildOptions) -> Result<Report, BuildError> {
    let manifest_dir = plan::canonical_manifest_dir(&opts.manifest_dir)?;
    let root = match plan::find_crate_root(&manifest_dir)? {
        CrateRoot::NoRsxRoot => return Ok(Report::default()),
        CrateRoot::Rsx(root) => root,
    };

    let planned = plan::plan(&manifest_dir, &root)?;
    let output = emit::emit(&planned)?;

    let produced: HashSet<PathBuf> = output
        .generated_files
        .iter()
        .cloned()
        .chain(output.map_files.iter().cloned())
        .collect();
    let removed = clean::clean_stale(&planned.generated_dir, &produced).map_err(|source| {
        BuildError::Clean {
            dir: planned.generated_dir.clone(),
            source,
        }
    })?;

    Ok(Report {
        built: true,
        generated_files: output.generated_files,
        map_files: output.map_files,
        removed_files: removed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn copy_fixture(name: &str) -> PathBuf {
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/modules")
            .join(name);
        let dest = std::env::temp_dir().join(format!(
            "outou-cli-build-test-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        copy_dir(&src, &dest);
        // `build()` canonicalizes its manifest directory internally, so
        // tests comparing against `report.generated_files`/`removed_files`
        // need the same canonical form (macOS: `TMPDIR` is itself under a
        // symlink, `/var` -> `/private/var`).
        dest.canonicalize()
            .unwrap_or_else(|e| panic!("canonicalizing {}: {e}", dest.display()))
    }

    fn copy_dir(src: &std::path::Path, dest: &std::path::Path) {
        fs::create_dir_all(dest).unwrap();
        for entry in fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let target = dest.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), &target).unwrap();
            }
        }
    }

    #[test]
    fn builds_the_mixed_fixture_and_writes_expected_files() {
        let dir = copy_fixture("mixed");
        let report = build(&BuildOptions::new(&dir)).expect("mixed builds");

        assert!(report.built);
        assert_eq!(report.generated_files.len(), 3);
        for file in &report.generated_files {
            assert!(file.exists(), "{}", file.display());
        }
        for file in &report.map_files {
            assert!(file.exists(), "{}", file.display());
        }
        assert!(dir.join("src/.generated/crate-root.rs").exists());
        assert!(dir.join("src/.generated/components.rs").exists());
        assert!(dir.join("src/.generated/components/user.rs").exists());
        // button.rs is plain Rust: never copied into `.generated/`.
        assert!(!dir.join("src/.generated/components/button.rs").exists());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cleans_stale_files_and_leaves_unmanaged_ones_alone() {
        let dir = copy_fixture("mixed");
        fs::create_dir_all(dir.join("src/.generated")).unwrap();
        fs::write(dir.join("src/.generated/stale.rs"), "// stale").unwrap();
        fs::write(dir.join("src/.generated/notes.txt"), "keep me").unwrap();

        let report = build(&BuildOptions::new(&dir)).expect("mixed builds");

        assert!(report
            .removed_files
            .contains(&dir.join("src/.generated/stale.rs")));
        assert!(!dir.join("src/.generated/stale.rs").exists());
        assert!(dir.join("src/.generated/notes.txt").exists());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn building_a_plain_rust_crate_does_nothing() {
        let dir = std::env::temp_dir().join(format!(
            "outou-cli-build-test-plain-only-{}-{}",
            std::process::id(),
            line!()
        ));
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();

        let report = build(&BuildOptions::new(&dir)).expect("no error for a plain crate");

        fs::remove_dir_all(&dir).ok();
        assert!(!report.built);
        assert!(report.generated_files.is_empty());
    }

    #[test]
    fn a_syntax_error_fails_the_build_in_strict_mode() {
        let dir = std::env::temp_dir().join(format!(
            "outou-cli-build-test-syntax-error-{}-{}",
            std::process::id(),
            line!()
        ));
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/main.rsx"), "fn f() { <div cl").unwrap();

        let err = build(&BuildOptions::new(&dir)).expect_err("broken source fails strict mode");

        fs::remove_dir_all(&dir).ok();
        assert!(matches!(
            err,
            BuildError::Emit(EmitError::SyntaxErrors { .. })
        ));
    }
}
