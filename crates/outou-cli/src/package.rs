//! `outou package [--manifest-dir DIR] [--check] [-- <cargo package
//! args>]`: prepares a `.rsx` crate for `cargo publish` (ADR 0008,
//! `docs/phase0/issues/15-outou-package.md`).
//!
//! Published crates ship pre-generated Rust under `src/.generated/` so a
//! consumer needs only `cargo build`: no Outou compiler, CLI or build
//! script (the generated code still depends on the `outou` runtime crate
//! like any ordinary dependency, ADR 0010) — but that only
//! works if the committed generated output is actually what the current
//! `.rsx` sources would produce, and if `cargo package`'s own file list
//! actually includes it (a gitignored `.generated/` or a missing
//! `[package] include` would silently ship a library with no generated
//! Rust at all). [`check_generated`] answers the first question without
//! writing anything (used both by `--check`, for CI, and internally
//! before this command ever calls `cargo`); [`run`] answers the second by
//! diffing a fresh `outou build` against `cargo package --list`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use outou_codegen::Mode;

use crate::build::plan::{self, CrateRoot};
use crate::build::workspace::{self, WorkspaceError};
use crate::build::{self, clean, emit, paths, BuildOptions};

/// Files whose disk content (or presence) does not match what generating
/// every planned unit in memory (always [`Mode::Strict`]) would produce.
/// Never mutates anything on disk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DriftReport {
    /// A managed file exists but its content differs from what
    /// generation would write.
    pub changed: Vec<PathBuf>,
    /// A managed file the current plan expects is missing entirely.
    pub missing: Vec<PathBuf>,
    /// A managed file exists under `src/.generated/` that the current
    /// plan does not produce at all (a renamed or deleted module left it
    /// behind — the same notion `build::clean::clean_stale` acts on, but
    /// this only reports it).
    pub stale: Vec<PathBuf>,
}

impl DriftReport {
    /// Whether every generated file on disk matches the plan, with
    /// nothing missing and nothing stale left behind.
    pub fn is_clean(&self) -> bool {
        self.changed.is_empty() && self.missing.is_empty() && self.stale.is_empty()
    }

    /// Every path this report names, in `changed`, `missing`, `stale`
    /// order, for a stable listing in CLI output.
    pub fn all_paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.changed.iter().chain(&self.missing).chain(&self.stale)
    }
}

/// Errors from [`check_generated`].
#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    /// Planning the build failed.
    #[error(transparent)]
    Plan(#[from] plan::PlanError),
    /// Generating one unit's Rust in memory failed (a syntax error, most
    /// likely — the same error `outou build` itself would report).
    #[error(transparent)]
    Generate(#[from] emit::EmitError),
    /// Building a unit's source map JSON failed (only possible on an
    /// internal inconsistency; see `emit::EmitError::SourceMapJson`).
    #[error("building source map for `{}`: {source}", path.display())]
    SourceMapJson {
        /// The generated `.rs` file the map is for.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: outou_sourcemap::ToJsonError,
    },
    /// Reading a `.rsx` source file failed.
    #[error("reading `{}`: {source}", path.display())]
    ReadSource {
        /// The file that could not be read.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// Walking `src/.generated/` to find stale managed files failed.
    #[error("walking `{}`: {source}", dir.display())]
    WalkGenerated {
        /// The `.generated` directory being walked.
        dir: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// Reading `manifest_dir/Cargo.toml`'s `[workspace] members` failed.
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
    /// `manifest_dir` is a workspace root (a manifest with `[workspace]`
    /// and no `[package]`), not a single crate. `outou package` (both
    /// `--check` and the real thing) always operates on exactly one
    /// library crate — the one at `--manifest-dir` — and never iterates a
    /// workspace's members on its own: an application member's
    /// `src/.generated/` is gitignored by design (ADR 0008), so silently
    /// walking every member would report spurious drift for it.
    #[error(
        "`{}` is a workspace root; `outou package` works on one library crate at a time; \
         pass `--manifest-dir <member>` for each member that publishes",
        path.display()
    )]
    WorkspaceRoot {
        /// The workspace root manifest directory.
        path: PathBuf,
    },
}

/// Fails with [`CheckError::WorkspaceRoot`] when `manifest_dir` is a
/// workspace root rather than a single crate. Shared by [`check_generated`]
/// and [`run`], which both must refuse to iterate a workspace's members.
fn reject_workspace_root(manifest_dir: &Path) -> Result<(), CheckError> {
    if workspace::workspace_members(manifest_dir)?.is_some() {
        return Err(CheckError::WorkspaceRoot {
            path: manifest_dir.to_path_buf(),
        });
    }
    Ok(())
}

/// Checks whether `opts.manifest_dir`'s committed `src/.generated/`
/// output matches what regenerating every `.rsx` source in memory
/// (always [`Mode::Strict`], the same mode `outou build` uses) would
/// produce. Never writes anything.
///
/// Returns an empty (clean) [`DriftReport`] when the crate has no `.rsx`
/// crate root — a plain `.rs` crate is not drift.
pub fn check_generated(opts: &BuildOptions) -> Result<DriftReport, CheckError> {
    let manifest_dir = plan::canonical_manifest_dir(&opts.manifest_dir)?;
    reject_workspace_root(&manifest_dir)?;
    let root = match plan::find_crate_root(&manifest_dir)? {
        CrateRoot::NoRsxRoot => return Ok(DriftReport::default()),
        CrateRoot::Rsx(root) => root,
    };
    let planned = plan::plan(&manifest_dir, &root)?;

    let mut report = DriftReport::default();
    let mut produced: HashSet<PathBuf> = HashSet::new();

    for unit in &planned.units {
        let source = std::fs::read_to_string(&unit.source_file).map_err(|source| {
            CheckError::ReadSource {
                path: unit.source_file.clone(),
                source,
            }
        })?;
        let generated = emit::generate_unit(unit, &source, Mode::Strict)?;

        let mut map_json = generated
            .source_map
            .to_json(&generated.rust, &[&source])
            .map_err(|source| CheckError::SourceMapJson {
                path: unit.generated_file.clone(),
                source,
            })?;
        paths::relativize_map_json(
            &mut map_json,
            &planned.crate_dir,
            &unit.generated_file,
            &[&unit.source_file],
        );
        let map_text = serde_json::to_string_pretty(&map_json)
            .expect("SourceMapJson always serializes")
            + "\n";

        produced.insert(unit.generated_file.clone());
        produced.insert(unit.map_file.clone());

        compare_expected(&unit.generated_file, generated.rust.as_bytes(), &mut report);
        compare_expected(&unit.map_file, map_text.as_bytes(), &mut report);
    }

    let on_disk = clean::walk_managed_files(&planned.generated_dir).map_err(|source| {
        CheckError::WalkGenerated {
            dir: planned.generated_dir.clone(),
            source,
        }
    })?;
    for path in on_disk {
        if !produced.contains(&path) {
            report.stale.push(path);
        }
    }
    report.stale.sort();

    Ok(report)
}

/// Compares `path`'s on-disk content (if any) against `expected`,
/// recording the result into `report.changed`/`report.missing`. A
/// filesystem error other than "not found" while reading is treated the
/// same as "changed" (something is wrong with this file; either way it
/// needs the operator's attention, and `check_generated` never writes
/// anything that would let it recover on its own).
fn compare_expected(path: &Path, expected: &[u8], report: &mut DriftReport) {
    match std::fs::read(path) {
        Ok(actual) if actual == expected => {}
        Ok(_) => report.changed.push(path.to_path_buf()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            report.missing.push(path.to_path_buf())
        }
        Err(_) => report.changed.push(path.to_path_buf()),
    }
}

/// Runs `outou package`.
///
/// `check_only: true` runs only [`check_generated`] (CI's `generated-drift`
/// job): a clean report prints one line and exits 0; drift lists every
/// affected file and exits 1. Never writes anything and never calls
/// `cargo`.
///
/// Otherwise: regenerates through [`build::build`] (Strict, writing to
/// disk exactly like `outou build`), confirms every generated `.rs` file
/// is actually included in `cargo package --list`'s output (a gitignored
/// `.generated/` or a missing `[package] include` would silently publish
/// a library with no generated Rust at all — the exact Phase 0 trap ADR
/// 0008 exists to avoid), then runs `cargo package` with `extra_args`
/// passed straight through.
pub fn run(manifest_dir: &Path, check_only: bool, extra_args: &[String]) -> ExitCode {
    let manifest_dir = match plan::canonical_manifest_dir(manifest_dir) {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(err) = reject_workspace_root(&manifest_dir) {
        eprintln!("error: {err}");
        return ExitCode::FAILURE;
    }

    if check_only {
        return run_check(&manifest_dir);
    }

    if let Err(message) = validate_passthrough_args(&manifest_dir, extra_args) {
        eprintln!("error: {message}");
        return ExitCode::FAILURE;
    }

    let opts = BuildOptions::new(manifest_dir.clone());
    let report = match build::build(&opts) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };
    if !report.built {
        println!("outou package: no `.rsx` crate root found; nothing to do");
        return ExitCode::SUCCESS;
    }

    if let Err(message) =
        verify_generated_files_are_packaged(&manifest_dir, &report.generated_files)
    {
        eprintln!("error: {message}");
        return ExitCode::FAILURE;
    }

    match run_cargo_package(&manifest_dir, extra_args) {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        // Propagate `cargo package`'s own exit code when it gives one
        // (a signal-terminated process has none): callers scripting
        // against `outou package` see the same code `cargo package`
        // itself would have exited with, not just a generic failure.
        Ok(status) => ExitCode::from(status.code().unwrap_or(1) as u8),
        Err(err) => {
            eprintln!("error: running `cargo package`: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run_check(manifest_dir: &Path) -> ExitCode {
    let opts = BuildOptions::new(manifest_dir.to_path_buf());
    let report = match check_generated(&opts) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };

    if report.is_clean() {
        println!("outou package --check: generated output is up to date");
        return ExitCode::SUCCESS;
    }

    // Crate-relative, not absolute: an operator's own checkout path (or
    // CI's runner-specific one) has no business showing up in this
    // listing (location-independence, ADR 0008).
    for path in &report.changed {
        println!(
            "changed: {}",
            paths::crate_relative_display(manifest_dir, path)
        );
    }
    for path in &report.missing {
        println!(
            "missing: {}",
            paths::crate_relative_display(manifest_dir, path)
        );
    }
    for path in &report.stale {
        println!(
            "stale:   {}",
            paths::crate_relative_display(manifest_dir, path)
        );
    }
    eprintln!(
        "error: generated output does not match `.rsx` sources; run `outou build` and commit the result"
    );
    ExitCode::FAILURE
}

/// Passthrough flags that change which package(s) `cargo package` itself
/// selects. `outou package` only ever regenerated and verified the one
/// crate at `--manifest-dir`; letting any of these through would let
/// `cargo package` ship packages `outou package` never checked.
const REJECTED_SELECTION_FLAGS: &[&str] = &["--manifest-path", "--workspace", "--all", "--exclude"];

/// Rejects passthrough args (the ones after `--`) that would change which
/// package(s) `cargo package` selects, before `outou package` ever plans a
/// build or calls `cargo`.
///
/// `-p`/`--package` is allowed — publishing `outou` alongside a library
/// needs `-- -p outou -p ui-kit`, since `outou` itself is unpublished
/// (ADR 0008) — but only when the crate at `--manifest-dir` is itself one
/// of the named packages; otherwise `cargo package` could ship a crate
/// `outou package` never verified while silently skipping the one it did.
/// Every other `-p` value (e.g. `outou`) is passed through unverified: see
/// the `TODO(phase0)` in `crates/outou-cli/README.md`.
fn validate_passthrough_args(manifest_dir: &Path, extra_args: &[String]) -> Result<(), String> {
    for arg in extra_args {
        let flag_name = arg.split('=').next().unwrap_or(arg.as_str());
        if REJECTED_SELECTION_FLAGS.contains(&flag_name) {
            return Err(format!(
                "`outou package` packages the crate at `--manifest-dir`; `{arg}` would make \
                 `cargo package` select packages `outou package` did not verify"
            ));
        }
    }

    let package_values = extract_package_flag_values(extra_args);
    if package_values.is_empty() {
        return Ok(());
    }

    let own_name = read_package_name(manifest_dir)?;
    if package_values.contains(&own_name) {
        return Ok(());
    }
    Err(format!(
        "`-p`/`--package` was given but none of {package_values:?} is `{own_name}`, the crate at \
         `--manifest-dir`; `outou package` only regenerated and verified that crate, so `cargo \
         package` must include it"
    ))
}

/// Every `-p`/`--package` value in `args`, in every form Cargo accepts:
/// `-p NAME`, `-pNAME`, `--package NAME`, `--package=NAME`.
fn extract_package_flag_values(args: &[String]) -> Vec<String> {
    let mut values = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if let Some(value) = arg.strip_prefix("--package=") {
            values.push(value.to_string());
        } else if arg == "--package" {
            if let Some(value) = iter.next() {
                values.push(value.clone());
            }
        } else if let Some(rest) = arg.strip_prefix("-p") {
            if rest.is_empty() {
                if let Some(value) = iter.next() {
                    values.push(value.clone());
                }
            } else {
                values.push(rest.to_string());
            }
        }
    }
    values
}

/// Reads `manifest_dir/Cargo.toml`'s `[package].name`.
fn read_package_name(manifest_dir: &Path) -> Result<String, String> {
    let manifest_path = manifest_dir.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|err| format!("reading `{}`: {err}", manifest_path.display()))?;
    let value: toml::Value = toml::from_str(&text)
        .map_err(|err| format!("parsing `{}`: {err}", manifest_path.display()))?;
    value
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(|name| name.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("`{}` has no `[package].name`", manifest_path.display()))
}

/// Confirms every path in `generated_files` (absolute) appears in `cargo
/// package --list`'s output for the crate at `manifest_dir`. On failure,
/// returns an actionable message: the two most common causes are the
/// repository's own `.gitignore` still excluding `.generated/` for a
/// crate that publishes it, or a `[package] include` that does not name
/// it.
fn verify_generated_files_are_packaged(
    manifest_dir: &Path,
    generated_files: &[PathBuf],
) -> Result<(), String> {
    let output = Command::new(cargo_program())
        .args(["package", "--list", "--allow-dirty"])
        .current_dir(manifest_dir)
        .output()
        .map_err(|err| format!("running `cargo package --list`: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "`cargo package --list` failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let listed: HashSet<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().replace('\\', "/"))
        .collect();

    let missing: Vec<String> = generated_files
        .iter()
        .map(|path| paths::crate_relative_display(manifest_dir, path))
        .filter(|relative| !listed.contains(relative))
        .collect();

    if missing.is_empty() {
        return Ok(());
    }

    Err(format!(
        "the following generated file(s) will not ship with `cargo package`, \
         even though `outou build` just wrote them: {missing:?}\n\
         This crate's `src/.generated/` is likely gitignored, or its \
         `[package] include` (if it has one) does not name it — a \
         published Outou library must ship its generated Rust (ADR 0008)."
    ))
}

fn run_cargo_package(
    manifest_dir: &Path,
    extra_args: &[String],
) -> std::io::Result<std::process::ExitStatus> {
    Command::new(cargo_program())
        .arg("package")
        .args(extra_args)
        .current_dir(manifest_dir)
        .status()
}

/// The `cargo` binary to invoke: `$CARGO` when set (the same binary
/// `cargo test`/`cargo run` set for their own subprocesses), else the
/// bare `cargo` found on `PATH`.
fn cargo_program() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn copy_fixture(name: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/modules")
            .join(name);
        let dest = std::env::temp_dir().join(format!(
            "outou-cli-package-test-{name}-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        copy_dir(&src, &dest);
        dest.canonicalize()
            .unwrap_or_else(|e| panic!("canonicalizing {}: {e}", dest.display()))
    }

    fn copy_dir(src: &Path, dest: &Path) {
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
    fn a_freshly_built_crate_has_no_drift() {
        let dir = copy_fixture("mixed");
        build::build(&BuildOptions::new(&dir)).expect("mixed builds");

        let report = check_generated(&BuildOptions::new(&dir)).expect("checking succeeds");

        assert!(report.is_clean(), "{report:?}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn editing_the_rsx_source_after_building_is_reported_as_changed() {
        let dir = copy_fixture("mixed");
        build::build(&BuildOptions::new(&dir)).expect("mixed builds");
        let root_rsx = dir.join("src/main.rsx");
        let original = fs::read_to_string(&root_rsx).unwrap();
        fs::write(&root_rsx, format!("{original}\n// edited\n")).unwrap();

        let report = check_generated(&BuildOptions::new(&dir)).expect("checking succeeds");

        assert!(!report.is_clean());
        assert!(
            report
                .changed
                .iter()
                .any(|p| p.file_name().unwrap() == "crate-root.rs"),
            "{report:?}"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_generated_file_is_reported_as_missing() {
        let dir = copy_fixture("mixed");
        build::build(&BuildOptions::new(&dir)).expect("mixed builds");
        fs::remove_file(dir.join("src/.generated/crate-root.rs")).unwrap();

        let report = check_generated(&BuildOptions::new(&dir)).expect("checking succeeds");

        assert!(!report.is_clean());
        assert!(report
            .missing
            .iter()
            .any(|p| p.file_name().unwrap() == "crate-root.rs"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_stale_generated_file_left_over_from_a_removed_module_is_reported() {
        let dir = copy_fixture("mixed");
        build::build(&BuildOptions::new(&dir)).expect("mixed builds");
        let stale = dir.join("src/.generated/leftover.rs");
        fs::write(&stale, "// not produced by the current plan\n").unwrap();

        let report = check_generated(&BuildOptions::new(&dir)).expect("checking succeeds");

        assert!(!report.is_clean());
        assert!(report.stale.contains(&stale), "{report:?}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn two_copies_at_different_absolute_paths_generate_byte_identical_output() {
        let dir_a = copy_fixture("mixed");
        let dir_b = copy_fixture("mixed");
        // Both copies come from the same source fixture, into two
        // distinct temp directories (`copy_fixture`'s own
        // unique-per-call, time-based temp path), so this exercises
        // exactly the location-independence ADR 0008 requires.
        assert_ne!(dir_a, dir_b, "the two copies must land at different paths");
        build::build(&BuildOptions::new(&dir_a)).expect("mixed builds");
        build::build(&BuildOptions::new(&dir_b)).expect("mixed builds");

        let generated_a =
            fs::read(dir_a.join("src/.generated/crate-root.rs")).expect("dir_a generated");
        let generated_b =
            fs::read(dir_b.join("src/.generated/crate-root.rs")).expect("dir_b generated");
        assert_eq!(generated_a, generated_b);

        fs::remove_dir_all(&dir_a).ok();
        fs::remove_dir_all(&dir_b).ok();
    }

    #[test]
    fn a_workspace_root_is_rejected_by_check_generated() {
        let dir = std::env::temp_dir().join(format!(
            "outou-cli-package-workspace-root-{}-{}",
            std::process::id(),
            line!()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\", \"b\"]\n",
        )
        .unwrap();

        let error = check_generated(&BuildOptions::new(&dir)).expect_err("must reject");

        fs::remove_dir_all(&dir).ok();
        assert!(matches!(error, CheckError::WorkspaceRoot { .. }));
        assert!(error.to_string().contains("workspace root"), "{error}");
    }

    #[test]
    fn extract_package_flag_values_understands_every_cargo_form() {
        let args: Vec<String> = ["-p", "a", "-pb", "--package", "c", "--package=d", "-e"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let values = extract_package_flag_values(&args);

        assert_eq!(values, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn extract_package_flag_values_is_empty_when_no_p_flag_is_present() {
        let args: Vec<String> = ["--allow-dirty", "--no-verify"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        assert!(extract_package_flag_values(&args).is_empty());
    }

    /// [`copy_fixture`]'s `mixed` fixture has no `Cargo.toml` (it only
    /// needs `src/` for `build::build`); `validate_passthrough_args`'s `-p`
    /// handling reads `[package].name`, so these tests write a minimal one
    /// naming the crate `mixed`.
    fn copy_fixture_with_manifest(name: &str, package_name: &str) -> PathBuf {
        let dir = copy_fixture(name);
        fs::write(
            dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"{package_name}\"\nversion = \"0.0.1\"\nedition = \"2021\"\n"
            ),
        )
        .unwrap();
        dir
    }

    #[test]
    fn validate_passthrough_args_rejects_selection_flags() {
        let dir = copy_fixture_with_manifest("mixed", "mixed");

        for flag in ["--workspace", "--all", "--manifest-path=x", "--exclude"] {
            let args = vec![flag.to_string()];
            let error = match validate_passthrough_args(&dir, &args) {
                Err(error) => error,
                Ok(()) => panic!("{flag} must be rejected"),
            };
            assert!(error.contains(flag), "{error}");
        }

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn validate_passthrough_args_allows_p_when_it_names_the_manifest_dir_crate() {
        let dir = copy_fixture_with_manifest("mixed", "mixed");
        let args = vec!["-p".to_string(), "mixed".to_string()];

        let result = validate_passthrough_args(&dir, &args);

        fs::remove_dir_all(&dir).ok();
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn validate_passthrough_args_rejects_p_without_the_manifest_dir_crate() {
        let dir = copy_fixture_with_manifest("mixed", "mixed");
        let args = vec!["-p".to_string(), "outou".to_string()];

        let error = validate_passthrough_args(&dir, &args).expect_err("must reject");

        fs::remove_dir_all(&dir).ok();
        assert!(error.contains("mixed"), "{error}");
    }

    #[test]
    fn validate_passthrough_args_allows_no_p_flag_at_all() {
        let dir = copy_fixture_with_manifest("mixed", "mixed");
        let args = vec!["--allow-dirty".to_string()];

        let result = validate_passthrough_args(&dir, &args);

        fs::remove_dir_all(&dir).ok();
        assert!(result.is_ok(), "{result:?}");
    }
}
