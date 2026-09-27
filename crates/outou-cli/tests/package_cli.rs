//! Integration tests for `outou package [--check]` (issue #15) run
//! against the `outou` binary itself.
//!
//! No `mod support;`: this binary needs only its own small `copy_dir`
//! helper (no `fixtures_dir`/`repo_root` split it would leave
//! `dead_code`), matching `build_cli.rs`'s and `matrix.rs`'s own rule
//! (`support.rs`'s doc comment).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// The repository root, computed from this crate's own manifest
/// directory so tests work regardless of the current working directory
/// `cargo test` was invoked from.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root exists")
}

/// Copies `tests/fixtures/workspace/ui-kit` into a fresh temp directory
/// and returns it. Every call gets its own directory (time plus an
/// in-process counter, alongside the process id) so parallel test
/// threads — and two calls from the same test, back to back — never
/// collide.
fn copy_ui_kit_to_temp(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let src = repo_root().join("tests/fixtures/workspace/ui-kit");
    let dest = std::env::temp_dir().join(format!(
        "outou-cli-package-cli-it-{label}-{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    copy_dir(&src, &dest);
    dest.canonicalize()
        .unwrap_or_else(|e| panic!("canonicalizing {}: {e}", dest.display()))
}

fn copy_dir(src: &Path, dest: &Path) {
    fs::create_dir_all(dest).unwrap_or_else(|e| panic!("creating {}: {e}", dest.display()));
    for entry in fs::read_dir(src).unwrap_or_else(|e| panic!("reading {}: {e}", src.display())) {
        let entry = entry.unwrap();
        let target = dest.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target)
                .unwrap_or_else(|e| panic!("copying {}: {e}", entry.path().display()));
        }
    }
}

fn run_package_check(manifest_dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_outou"))
        .arg("package")
        .arg("--check")
        .arg("--manifest-dir")
        .arg(manifest_dir)
        .output()
        .expect("running the `outou` binary")
}

/// Runs `outou package --manifest-dir <manifest_dir> -- <extra_args>`
/// (no `--check`).
fn run_package(manifest_dir: &Path, extra_args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_outou"))
        .arg("package")
        .arg("--manifest-dir")
        .arg(manifest_dir)
        .arg("--")
        .args(extra_args)
        .output()
        .expect("running the `outou` binary")
}

/// A fresh temp directory with only the given `Cargo.toml` contents — no
/// `src/`, no real crate. Enough for the workspace-root guard and the
/// passthrough-arg validation, both of which run (and must fail) before
/// `outou package` ever plans a build or calls `cargo`.
fn temp_manifest_only(label: &str, cargo_toml: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let dir = std::env::temp_dir().join(format!(
        "outou-cli-package-cli-manifest-only-{label}-{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
    dir
}

/// `outou package` (either mode) operates on exactly one library crate,
/// the one at `--manifest-dir`; it must never silently iterate a
/// workspace's members. Pointing it at a workspace root (a manifest with
/// `[workspace]` and no `[package]`, `tests/fixtures/workspace`'s own
/// shape) must fail clearly instead.
#[test]
fn check_fails_at_a_workspace_root_and_names_it() {
    let dir = temp_manifest_only(
        "workspace-root",
        "[workspace]\nresolver = \"2\"\nmembers = [\"ui-kit\", \"app\"]\n",
    );

    let output = run_package_check(&dir);

    fs::remove_dir_all(&dir).ok();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("workspace root"),
        "must name the problem as a workspace root: {stderr}"
    );
}

/// The same workspace-root guard applies to the non-`--check` path too
/// (`package::run`'s own top-of-function check), not only to
/// `check_generated`.
#[test]
fn package_without_check_also_fails_at_a_workspace_root() {
    let dir = temp_manifest_only(
        "workspace-root-run",
        "[workspace]\nresolver = \"2\"\nmembers = [\"ui-kit\", \"app\"]\n",
    );

    let output = run_package(&dir, &[]);

    fs::remove_dir_all(&dir).ok();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("workspace root"), "{stderr}");
}

/// `--workspace` would make `cargo package` select packages `outou
/// package` never verified. It must be rejected before `outou package`
/// even plans a build (fast: no `.rsx` sources, no `cargo` invocation).
#[test]
fn passthrough_workspace_flag_is_rejected_before_cargo_runs() {
    let dir = temp_manifest_only(
        "passthrough-workspace",
        "[package]\nname = \"x\"\nversion = \"0.0.1\"\nedition = \"2021\"\n",
    );

    let output = run_package(&dir, &["--workspace"]);

    fs::remove_dir_all(&dir).ok();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--workspace"),
        "must name the rejected flag: {stderr}"
    );
}

/// `-p`/`--package` is allowed (needed for `-- -p outou -p ui-kit`), but
/// only when the crate at `--manifest-dir` is itself one of the named
/// packages — otherwise `outou package` would be verifying one crate
/// while `cargo package` ships a different one.
#[test]
fn passthrough_package_flag_without_the_manifest_dir_crate_is_rejected() {
    let dir = temp_manifest_only(
        "passthrough-package",
        "[package]\nname = \"x\"\nversion = \"0.0.1\"\nedition = \"2021\"\n",
    );

    let output = run_package(&dir, &["-p", "outou"]);

    fs::remove_dir_all(&dir).ok();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains('x'),
        "must mention the missing own crate: {stderr}"
    );
}

/// A clean copy of `ui-kit` (committed generated output matches its
/// `.rsx` sources) must pass `--check` with exit 0, print no drift, and
/// leave every file on disk exactly as it was.
#[test]
fn check_passes_and_changes_nothing_on_a_clean_copy() {
    let dir = copy_ui_kit_to_temp("clean");
    let before = fs::read(dir.join("src/.generated/crate-root.rs")).unwrap();

    let output = run_package_check(&dir);

    let after = fs::read(dir.join("src/.generated/crate-root.rs")).unwrap();
    fs::remove_dir_all(&dir).ok();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(before, after, "`--check` must never write to disk");
}

/// Editing a `.rsx` source so it no longer matches its committed
/// generated output must fail `--check` (exit 1) and name the affected
/// generated file.
#[test]
fn check_fails_and_names_the_file_after_editing_the_rsx_source() {
    let dir = copy_ui_kit_to_temp("edited");
    let lib_rsx = dir.join("src/lib.rsx");
    let original = fs::read_to_string(&lib_rsx).unwrap();
    fs::write(&lib_rsx, format!("{original}\n// edited by a test\n")).unwrap();

    let output = run_package_check(&dir);

    fs::remove_dir_all(&dir).ok();

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("crate-root.rs"),
        "must name the affected generated file: {stdout}"
    );
}

/// A stale managed file under `src/.generated/` that the current plan
/// does not produce (a module renamed or removed without regenerating)
/// must fail `--check` and be reported as stale.
#[test]
fn check_fails_on_a_stale_generated_file() {
    let dir = copy_ui_kit_to_temp("stale");
    let stale = dir.join("src/.generated/leftover.rs");
    fs::write(&stale, "// not produced by the current plan\n").unwrap();

    let output = run_package_check(&dir);

    fs::remove_dir_all(&dir).ok();

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("leftover.rs"),
        "must name the stale file: {stdout}"
    );
}

/// ADR 0008's whole point: two copies of the same `.rsx` sources, at two
/// different absolute paths, must regenerate byte-identical output —
/// the regression this issue's blocker A fix (crate-relative
/// `source_display`/map JSON) exists for. `outou package --check` on
/// each copy independently passing is the externally observable half of
/// that; this also compares the two copies' generated bytes directly.
#[test]
fn two_copies_at_different_absolute_paths_produce_byte_identical_output_and_both_check_clean() {
    let dir_a = copy_ui_kit_to_temp("loc-a");
    let dir_b = copy_ui_kit_to_temp("loc-b");
    assert_ne!(dir_a, dir_b);

    let check_a = run_package_check(&dir_a);
    let check_b = run_package_check(&dir_b);

    let generated_a = fs::read(dir_a.join("src/.generated/crate-root.rs")).unwrap();
    let generated_b = fs::read(dir_b.join("src/.generated/crate-root.rs")).unwrap();

    fs::remove_dir_all(&dir_a).ok();
    fs::remove_dir_all(&dir_b).ok();

    assert!(check_a.status.success());
    assert!(check_b.status.success());
    assert_eq!(
        generated_a, generated_b,
        "committed generated output must not embed the checkout's own absolute path"
    );
}
