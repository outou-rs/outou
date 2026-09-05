//! The Cargo command matrix (issue #10, Phase 0 Step 6): after `outou
//! build`, does the *rest* of the ordinary Cargo workflow — `cargo
//! build`/`check`/`test`/`clippy`/`package`/`publish` — work on real
//! Outou output, for both an application (`examples/phase0-app`) and a
//! small workspace of one `.rsx` library and one `.rsx` application
//! (`tests/fixtures/workspace/{ui-kit,app}`)?
//!
//! Most of these are `#[ignore]`d (a cold `dioxus` build, exactly like
//! `build_compile.rs`'s probes) and grouped into one test per target so
//! each target's temp copy and `outou build` run only once; run them
//! explicitly:
//!
//! ```sh
//! cargo test -p outou-cli --test matrix -- --ignored --nocapture
//! ```
//!
//! A few fast checks that need no `cargo`/`rustc` invocation at all (the
//! workspace-aware `outou build` itself, and the library fixture's
//! generation determinism) run as part of the normal suite.

// No `mod support;`: this binary needs only `repo_root`/`copy_to_temp`,
// not `fixtures_dir` — `support.rs`'s own doc comment says a helper
// needed by only one file belongs in that file instead, private, so
// pulling in the shared module here would just leave `fixtures_dir`
// flagged `dead_code` in this binary's own compilation (`build_cli.rs`
// follows the same rule and declares no `mod support;` either).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use outou_cli::build::{build, BuildOptions};

/// The repository root, computed from this crate's own manifest
/// directory so tests work regardless of the current working directory
/// `cargo test` was invoked from.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root exists")
}

/// Copies `src` into a fresh temp directory and returns it (canonicalized
/// for the same reason `support::copy_to_temp` is: `build()` canonicalizes
/// its manifest directory internally, and macOS's `TMPDIR` is itself
/// under a symlink).
fn copy_to_temp(src: &Path, label: &str) -> PathBuf {
    let dest = std::env::temp_dir().join(format!(
        "outou-cli-matrix-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    copy_dir(src, &dest);
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

/// `tests/fixtures/workspace`, under the repository root.
fn workspace_fixture_dir() -> PathBuf {
    repo_root().join("tests/fixtures/workspace")
}

/// Runs `cargo <args>` in `dir`, sharing this repository's own `target/`
/// directory across every matrix test (like `build_compile.rs`) so a cold
/// `dioxus` build only ever happens once per `cargo test` invocation, not
/// once per test.
fn cargo(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO"))
        .args(args)
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", repo_root().join("target"))
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "running `cargo {}` in {}: {e}",
                args.join(" "),
                dir.display()
            )
        })
}

fn assert_success(output: &Output, what: &str) {
    assert!(
        output.status.success(),
        "{what} failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

// ---------------------------------------------------------------------
// examples/phase0-app
// ---------------------------------------------------------------------

/// Rewrites `phase0-app`'s copied `Cargo.toml` so its `outou` path
/// dependency (relative to the original location) resolves from a temp
/// copy instead, exactly like `build_compile.rs`'s own probes.
fn fix_up_copied_app_manifest(dir: &Path) {
    let outou_path = repo_root().join("crates/outou");
    let manifest = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    let manifest = manifest.replace(
        "path = \"../../crates/outou\"",
        &format!("path = {outou_path:?}"),
    );
    fs::write(dir.join("Cargo.toml"), manifest).unwrap();
}

/// The full matrix for `examples/phase0-app`: `outou build`, then `cargo
/// build`/`check`/`test`/`clippy`, then `cargo package --list` (ADR 0008:
/// an application never publishes — `phase0-app`'s own `Cargo.toml` sets
/// `publish = false` — so `cargo publish --dry-run` is meaningless here;
/// `package --list` is the closest useful check, confirming the `.rsx`
/// sources ship and `src/.generated/` does not, since it is gitignored
/// for applications and never even exists in a fresh checkout).
///
/// One test, one temp copy, one `outou build`: every command below reuses
/// it, so the cold `dioxus` build this triggers only happens once.
#[test]
#[ignore = "builds the full dioxus dependency tree; run explicitly, see module docs"]
fn example_app_cargo_matrix() {
    let dir = copy_to_temp(&repo_root().join("examples/phase0-app"), "matrix-app");
    fix_up_copied_app_manifest(&dir);

    let report = build(&BuildOptions::new(&dir)).expect("outou build succeeds on phase0-app");
    assert!(report.built, "phase0-app has an `.rsx` crate root");

    assert_success(&cargo(&dir, &["build"]), "cargo build");
    assert_success(&cargo(&dir, &["check"]), "cargo check");

    let test_output = cargo(&dir, &["test"]);
    assert_success(&test_output, "cargo test");
    let test_stdout = String::from_utf8_lossy(&test_output.stdout);
    assert!(
        test_stdout.contains("components::tests::plain_rust_tests_still_run"),
        "the `#[cfg(test)] mod tests` inside `components.rsx` must actually run:\n{test_stdout}"
    );

    assert_success(
        &cargo(&dir, &["clippy", "--all-targets", "--", "-D", "warnings"]),
        "cargo clippy --all-targets -- -D warnings",
    );

    // ADR 0008 / `ui_kit`'s own finding below: `cargo package --list`
    // shows what a `cargo publish` would ship. An application is never
    // published (`publish = false`), so this is the useful check here,
    // not `cargo publish --dry-run`.
    let package_output = cargo(&dir, &["package", "--list", "--allow-dirty"]);
    assert_success(&package_output, "cargo package --list");
    let packaged = String::from_utf8_lossy(&package_output.stdout);
    assert!(packaged.contains("src/main.rsx"), "{packaged}");
    assert!(packaged.contains("src/components.rsx"), "{packaged}");
    assert!(
        !packaged.contains(".generated"),
        "an application's `src/.generated/` must never ship (ADR 0008; it does not even \
         exist in a fresh checkout — it is gitignored):\n{packaged}"
    );

    fs::remove_dir_all(&dir).ok();
}

/// Genuine Phase 0 finding (issue #10 deliverable 2, `crates/outou-cli/README.md`):
/// `cargo test --doc` requires a `[lib]` target to link a doc test
/// against. `examples/phase0-app` is `[[bin]]`-only (ADR 0009's
/// application layout), so `cargo test --doc` refuses outright — before
/// even considering whether any individual doc comment contains JSX —
/// regardless of how many plain, JSX-free `///` doc tests
/// `components.rsx` carries (it carries one, on `initial`). This is a
/// Cargo-level constraint, independent of Outou; `cargo test --doc -p
/// ui-kit` (`tests/fixtures/workspace`) exercises the same doc-comment
/// shape successfully, because `ui-kit` is a library.
///
/// Cheap: `cargo test --doc` fails at the manifest-check stage, before
/// compiling anything (no `dioxus` build), so this does not need
/// `#[ignore]`.
#[test]
fn example_app_doc_tests_cannot_run_because_there_is_no_library_target() {
    let dir = copy_to_temp(
        &repo_root().join("examples/phase0-app"),
        "matrix-app-doctest",
    );
    fix_up_copied_app_manifest(&dir);
    build(&BuildOptions::new(&dir)).expect("outou build succeeds on phase0-app");

    let output = cargo(&dir, &["test", "--doc"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    fs::remove_dir_all(&dir).ok();

    assert!(
        !output.status.success(),
        "`cargo test --doc` was expected to fail for a bin-only crate:\n{stderr}"
    );
    assert!(
        stderr.contains("no library targets found"),
        "expected Cargo's own bin-only-crate message:\n{stderr}"
    );
}

// ---------------------------------------------------------------------
// tests/fixtures/workspace (ui-kit + app)
// ---------------------------------------------------------------------

/// `outou build --manifest-dir <workspace root>` builds every member with
/// an `.rsx` crate root in one pass (the `[workspace] members` reader in
/// `crate::build::workspace`). Fast: no `cargo`/`rustc` invocation.
#[test]
fn outou_build_builds_every_workspace_member_in_one_pass() {
    let dir = copy_to_temp(&workspace_fixture_dir(), "workspace-build");

    let report = build(&BuildOptions::new(&dir)).expect("the workspace fixture builds");

    assert!(report.built);
    let generated: Vec<String> = report
        .generated_files
        .iter()
        .map(|p| {
            p.strip_prefix(&dir)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    for expected in [
        "ui-kit/src/.generated/crate-root.rs",
        "ui-kit/src/.generated/widgets.rs",
        "ui-kit/src/.generated/widgets/button.rs",
        "ui-kit/src/.generated/extra.rs",
        "ui-kit/src/.generated/circle.rs",
        "app/src/.generated/crate-root.rs",
    ] {
        assert!(
            generated.contains(&expected.to_string()),
            "missing {expected} in {generated:?}"
        );
    }
    // `util.rs` is plain Rust: resolved and referenced via `#[path]`, but
    // never itself copied into `.generated/`.
    assert!(!dir.join("ui-kit/src/.generated/util.rs").exists());

    fs::remove_dir_all(&dir).ok();
}

/// Determinism for a library (ADR 0008): re-running `outou build` over
/// the already-committed `ui-kit` fixture must reproduce its generated
/// `.rs` files identically, up to each tree's own root path. Only the
/// `.rs` files are compared, not their `.rs.map.json` sidecars (`cargo
/// xtask determinism` already covers those structurally); a generated
/// `.rs` file's own header comment embeds the generating machine's
/// absolute `file://` source path (`crates/outou-cli/README.md`), so it
/// necessarily differs between the committed originals and a temp copy's
/// regeneration — normalized away here exactly like `cargo xtask
/// determinism`'s own `normalize`. Fast: no `cargo`/`rustc` invocation.
#[test]
fn outou_build_regenerates_ui_kit_generated_output_byte_identically() {
    let fixture_root = workspace_fixture_dir();
    let original_dir = fixture_root.join("ui-kit/src/.generated");
    let mut originals = Vec::new();
    collect_rs_files(&original_dir, &mut originals);
    assert!(
        !originals.is_empty(),
        "the committed `ui-kit/src/.generated/` fixture must exist and contain `.rs` files"
    );

    let dir = copy_to_temp(&fixture_root, "workspace-determinism");
    build(&BuildOptions::new(&dir)).expect("the workspace fixture builds");

    for original in &originals {
        let relative = original.strip_prefix(&original_dir).unwrap();
        let regenerated = dir.join("ui-kit/src/.generated").join(relative);
        let original_text = fs::read_to_string(original).unwrap();
        let regenerated_text = fs::read_to_string(&regenerated)
            .unwrap_or_else(|e| panic!("reading {}: {e}", regenerated.display()));
        assert_eq!(
            normalize_root(&original_text, &fixture_root),
            normalize_root(&regenerated_text, &dir),
            "{} must regenerate identically, up to its own root path",
            relative.display()
        );
    }

    fs::remove_dir_all(&dir).ok();
}

/// Replaces every occurrence of `root`'s own absolute path with a fixed
/// placeholder, exactly like `cargo xtask determinism`'s own `normalize`:
/// a generated file's header comment embeds the absolute path it was
/// generated from, so two otherwise-identical trees rooted at different
/// locations only become comparable once that is normalized away.
fn normalize_root(text: &str, root: &Path) -> String {
    text.replace(&root.display().to_string(), "<root>")
}

/// Every `.rs` file (not `.rs.map.json`) under `dir`, recursively.
fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// The full matrix for the `tests/fixtures/workspace` fixture: `outou
/// build` over the whole workspace, then `cargo build --workspace`,
/// `cargo test --workspace` (asserting both the library's own `#[cfg(test)]`
/// module and its doc test actually run), `cargo clippy --workspace
/// --all-targets -- -D warnings`, `cargo build -p ui-kit --features
/// extra`, and the packaging/publishing checks (`cargo package --list`
/// for both members, `cargo publish --dry-run --allow-dirty -p ui-kit`).
///
/// One test, one temp copy, one `outou build`.
#[test]
#[ignore = "builds the full dioxus dependency tree; run explicitly, see module docs"]
fn workspace_fixture_cargo_matrix() {
    let dir = copy_to_temp(&workspace_fixture_dir(), "matrix-workspace");
    fix_up_workspace_fixture_manifest(&dir, "ui-kit");
    fix_up_workspace_fixture_manifest(&dir, "app");

    let report = build(&BuildOptions::new(&dir)).expect("the workspace fixture builds");
    assert!(report.built);

    assert_success(
        &cargo(&dir, &["build", "--workspace"]),
        "cargo build --workspace",
    );

    let test_output = cargo(&dir, &["test", "--workspace"]);
    assert_success(&test_output, "cargo test --workspace");
    let test_stdout = String::from_utf8_lossy(&test_output.stdout);
    assert!(
        test_stdout.contains("plain_rust_tests_still_run_in_a_library_crate"),
        "ui-kit's own `#[cfg(test)]` module must run:\n{test_stdout}"
    );
    assert!(
        test_stdout.contains("crate-root.rs - add_one") && test_stdout.contains("... ok"),
        "ui-kit's plain-Rust doc test must run under `cargo test --workspace` \
         (issue #10 deliverable 2 — this is the library where it actually does):\n{test_stdout}"
    );

    assert_success(
        &cargo(
            &dir,
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
        "cargo clippy --workspace --all-targets -- -D warnings",
    );

    assert_success(
        &cargo(&dir, &["build", "-p", "ui-kit", "--features", "extra"]),
        "cargo build -p ui-kit --features extra",
    );

    // `app` never publishes (`publish = false`); its `.rsx` sources ship,
    // its `src/.generated/` does not (application layout, ADR 0008).
    let app_package = cargo(&dir, &["package", "--list", "--allow-dirty", "-p", "app"]);
    assert_success(&app_package, "cargo package --list -p app");
    let app_packaged = String::from_utf8_lossy(&app_package.stdout);
    assert!(app_packaged.contains("src/main.rsx"), "{app_packaged}");
    assert!(!app_packaged.contains(".generated"), "{app_packaged}");

    // `ui-kit` is a library (ADR 0008: published crates ship
    // pre-generated Rust) and explicitly `include`s `src/.generated/**/*`
    // — and yet, per the genuine Phase 0 finding recorded in its own
    // `Cargo.toml` and in `crates/outou-cli/README.md`, Cargo's package
    // builder unconditionally drops every dot-prefixed path component
    // from the packaged file set, `include` notwithstanding. The `.rsx`
    // sources still ship; `src/.generated/crate-root.rs` does not, so
    // `cargo publish --dry-run` fails outright (asserted below) rather
    // than merely warning.
    let lib_package = cargo(
        &dir,
        &["package", "--list", "--allow-dirty", "-p", "ui-kit"],
    );
    assert_success(&lib_package, "cargo package --list -p ui-kit");
    let lib_packaged = String::from_utf8_lossy(&lib_package.stdout);
    assert!(lib_packaged.contains("src/lib.rsx"), "{lib_packaged}");
    assert!(lib_packaged.contains("src/widgets.rsx"), "{lib_packaged}");
    assert!(
        !lib_packaged.contains(".generated"),
        "genuine Phase 0 finding (crates/outou-cli/README.md): Cargo drops dot-prefixed \
         paths from a package regardless of `include`, so `src/.generated/` never ships \
         even for a library that asks for it:\n{lib_packaged}"
    );

    let publish_output = cargo(
        &dir,
        &["publish", "--dry-run", "--allow-dirty", "-p", "ui-kit"],
    );
    let publish_stderr = String::from_utf8_lossy(&publish_output.stderr);
    assert!(
        !publish_output.status.success(),
        "`cargo publish --dry-run` was expected to fail (see the finding above): \
         {publish_stderr}"
    );
    assert!(
        publish_stderr.contains("no targets specified in the manifest"),
        "expected Cargo's own missing-lib-target message, since `src/.generated/crate-root.rs` \
         (the `[lib] path`) never made it into the package:\n{publish_stderr}"
    );

    fs::remove_dir_all(&dir).ok();
}

/// Rewrites one workspace-fixture member's `outou`/`ui-kit` path
/// dependencies (relative to the original fixture location) so they
/// resolve from a temp copy instead.
fn fix_up_workspace_fixture_manifest(dir: &Path, member: &str) {
    let outou_path = repo_root().join("crates/outou");
    let manifest_path = dir.join(member).join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path).unwrap();
    let manifest = manifest.replace(
        "path = \"../../../../crates/outou\", version = \"0.0.1\"",
        &format!("path = {outou_path:?}, version = \"0.0.1\""),
    );
    let manifest = manifest.replace(
        "path = \"../../../../crates/outou\"",
        &format!("path = {outou_path:?}"),
    );
    fs::write(&manifest_path, manifest).unwrap();
}
