//! Integration tests for `outou build`'s pipeline (issue #8): every
//! required build-pipeline fixture (`tests/fixtures/modules/{mixed, cfg,
//! cfg-duplicate, raw-ident, root-name, inline}`) and
//! `examples/phase0-app`, generated through the public `outou_cli::build`
//! API, plus the CLI binary's exit code and vocabulary on a syntax error.
//!
//! `tests/fixtures/modules/{path-attr,path-dirs,rs-to-rsx}` were built for
//! the resolver (`outou-modules`) and are exercised there and by
//! `crate::build::plan`'s own unit tests (the `RustDeclaresRsxChild`
//! error); they are not part of this build-pipeline suite.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use outou_cli::build::{build, BuildOptions};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root exists")
}

fn fixtures_dir() -> PathBuf {
    repo_root().join("tests/fixtures/modules")
}

/// Copies `src` into a fresh temp directory and returns it. Every test
/// gets its own directory (a unique suffix from the current time plus the
/// process id) so parallel test threads never collide.
fn copy_to_temp(src: &Path, label: &str) -> PathBuf {
    let dest = std::env::temp_dir().join(format!(
        "outou-cli-build-it-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    copy_dir(src, &dest);
    // `build()` canonicalizes its manifest directory internally (its
    // generated paths — and their `file://` URIs — must be absolute); on
    // some platforms `std::env::temp_dir()` itself is a symlink (macOS:
    // `/var` -> `/private/var`), so canonicalize here too, or every path
    // this test compares against `report.generated_files` would silently
    // mismatch on the un-resolved prefix.
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

fn rel_generated_files(dir: &Path, files: &[PathBuf]) -> HashSet<String> {
    let generated_dir = dir.join("src/.generated");
    files
        .iter()
        .map(|f| {
            f.strip_prefix(&generated_dir)
                .unwrap_or(f)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

struct FixtureCase {
    name: &'static str,
    expected_generated: &'static [&'static str],
}

const FIXTURE_CASES: &[FixtureCase] = &[
    FixtureCase {
        name: "mixed",
        expected_generated: &["crate-root.rs", "components.rs", "components/user.rs"],
    },
    FixtureCase {
        name: "cfg",
        expected_generated: &["crate-root.rs", "optional.rs", "spaced.rs"],
    },
    FixtureCase {
        name: "cfg-duplicate",
        expected_generated: &["crate-root.rs", "imp.rs", "imp-1.rs"],
    },
    FixtureCase {
        name: "raw-ident",
        expected_generated: &["crate-root.rs", "type.rs", "type/child.rs"],
    },
    FixtureCase {
        name: "root-name",
        expected_generated: &["crate-root.rs"],
    },
    FixtureCase {
        name: "inline",
        expected_generated: &["crate-root.rs", "shell/panel.rs"],
    },
];

#[test]
fn builds_every_required_fixture_with_the_expected_generated_files() {
    for case in FIXTURE_CASES {
        let dir = copy_to_temp(&fixtures_dir().join(case.name), case.name);
        let report = build(&BuildOptions::new(&dir))
            .unwrap_or_else(|e| panic!("building fixture {}: {e}", case.name));

        assert!(
            report.built,
            "fixture {} should have an rsx root",
            case.name
        );

        let got = rel_generated_files(&dir, &report.generated_files);
        let expected: HashSet<String> = case
            .expected_generated
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(got, expected, "fixture {}: generated .rs set", case.name);

        for rel in case.expected_generated {
            let generated = dir.join("src/.generated").join(rel);
            assert!(generated.exists(), "fixture {}: missing {rel}", case.name);
            let mut map = generated.clone();
            map.set_extension("rs.map.json");
            assert!(
                map.exists(),
                "fixture {}: missing source map for {rel}",
                case.name
            );
            // Every generated file is complete, balanced Rust text (a
            // cheap syntax sanity check that does not require `syn`).
            let text = fs::read_to_string(&generated).unwrap();
            assert_balanced_braces(&text, case.name, rel);
        }

        fs::remove_dir_all(&dir).ok();
    }
}

fn assert_balanced_braces(text: &str, case: &str, rel: &str) {
    let mut depth = 0i32;
    for byte in text.bytes() {
        match byte {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        assert!(depth >= 0, "fixture {case}: {rel} has unbalanced `}}`");
    }
    assert_eq!(depth, 0, "fixture {case}: {rel} has unbalanced braces");
}

#[test]
fn mixed_fixture_mod_declarations_carry_the_right_path_attribute() {
    let dir = copy_to_temp(&fixtures_dir().join("mixed"), "mixed-paths");
    build(&BuildOptions::new(&dir)).expect("mixed builds");

    let crate_root = fs::read_to_string(dir.join("src/.generated/crate-root.rs")).unwrap();
    assert!(
        crate_root.contains("#[path = \"components.rs\"]\nmod components;"),
        "{crate_root}"
    );

    let components = fs::read_to_string(dir.join("src/.generated/components.rs")).unwrap();
    assert!(
        components.contains("#[path = \"components/user.rs\"]\npub mod user;"),
        "{components}"
    );
    assert!(
        components.contains("#[path = \"../components/button.rs\"]\npub mod button;"),
        "{components}"
    );
    // `button.rs` is plain Rust: never copied into `.generated/`, only
    // referenced from it via a relative `#[path]`.
    assert!(!dir.join("src/.generated/components/button.rs").exists());

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn cfg_fixture_preserves_cfg_attributes_on_the_generated_declaration() {
    let dir = copy_to_temp(&fixtures_dir().join("cfg"), "cfg-preserve");
    build(&BuildOptions::new(&dir)).expect("cfg builds");

    let crate_root = fs::read_to_string(dir.join("src/.generated/crate-root.rs")).unwrap();
    assert!(
        crate_root.contains("#[cfg(feature = \"x\")]\n#[path = \"optional.rs\"]\nmod optional;"),
        "{crate_root}"
    );
    // The original attribute's own (nonstandard) inner spacing is
    // reproduced verbatim; only `#[path]` is rewritten.
    assert!(
        crate_root.contains("#[ cfg(feature = \"y\") ]\n#[path = \"spaced.rs\"]\nmod spaced;"),
        "{crate_root}"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn cfg_duplicate_fixture_gives_each_branch_its_own_path() {
    let dir = copy_to_temp(&fixtures_dir().join("cfg-duplicate"), "cfg-dup-paths");
    build(&BuildOptions::new(&dir)).expect("cfg-duplicate builds");

    let crate_root = fs::read_to_string(dir.join("src/.generated/crate-root.rs")).unwrap();
    assert!(
        crate_root.contains("#[cfg(unix)]\n#[path = \"imp.rs\"]\nmod imp;"),
        "{crate_root}"
    );
    assert!(
        crate_root.contains("#[cfg(windows)]\n#[path = \"imp-1.rs\"]\nmod imp;"),
        "{crate_root}"
    );

    let unix_generated = fs::read_to_string(dir.join("src/.generated/imp.rs")).unwrap();
    assert!(unix_generated.contains("unix imp"), "{unix_generated}");
    let windows_generated = fs::read_to_string(dir.join("src/.generated/imp-1.rs")).unwrap();
    assert!(
        windows_generated.contains("windows imp"),
        "{windows_generated}"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn raw_ident_fixture_keeps_the_raw_spelling_on_the_generated_declaration() {
    let dir = copy_to_temp(&fixtures_dir().join("raw-ident"), "raw-ident");
    build(&BuildOptions::new(&dir)).expect("raw-ident builds");

    let crate_root = fs::read_to_string(dir.join("src/.generated/crate-root.rs")).unwrap();
    assert!(
        crate_root.contains("#[path = \"type.rs\"]\nmod r#type;"),
        "{crate_root}"
    );

    let type_generated = fs::read_to_string(dir.join("src/.generated/type.rs")).unwrap();
    assert!(
        type_generated.contains("#[path = \"type/child.rs\"]\nmod child;"),
        "{type_generated}"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn inline_fixture_leaves_the_inline_module_header_untouched_but_rewrites_its_file_child() {
    let dir = copy_to_temp(&fixtures_dir().join("inline"), "inline");
    build(&BuildOptions::new(&dir)).expect("inline builds");

    let crate_root = fs::read_to_string(dir.join("src/.generated/crate-root.rs")).unwrap();
    assert!(crate_root.contains("mod shell {"), "{crate_root}");
    assert!(
        crate_root.contains("#[path = \"shell/panel.rs\"]\nmod panel;"),
        "{crate_root}"
    );
    assert!(dir.join("src/.generated/shell/panel.rs").exists());

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn root_name_fixture_does_not_collide_with_a_child_named_main() {
    let dir = copy_to_temp(&fixtures_dir().join("root-name"), "root-name");
    let report = build(&BuildOptions::new(&dir)).expect("root-name builds");

    // `mod main { ... }` is inline: it has no file, and no generated unit
    // of its own — only `crate-root.rs` is produced.
    assert_eq!(report.generated_files.len(), 1);
    assert_eq!(
        report.generated_files[0],
        dir.join("src/.generated/crate-root.rs")
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn builds_the_example_app_with_the_expected_generated_files() {
    let dir = copy_to_temp(&repo_root().join("examples/phase0-app"), "phase0-app");
    let report = build(&BuildOptions::new(&dir)).expect("phase0-app builds");

    let got = rel_generated_files(&dir, &report.generated_files);
    let expected: HashSet<String> = ["crate-root.rs", "components.rs"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_eq!(got, expected);

    let crate_root = fs::read_to_string(dir.join("src/.generated/crate-root.rs")).unwrap();
    assert!(
        crate_root.contains("#[path = \"components.rs\"]\nmod components;"),
        "{crate_root}"
    );
    assert!(!crate_root.contains("dioxus_rsx"));
    assert!(!crate_root.contains("PropsBuilder"));

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn stale_generated_files_are_removed_and_unmanaged_files_are_left_alone() {
    let dir = copy_to_temp(&fixtures_dir().join("mixed"), "stale-cleanup");
    fs::create_dir_all(dir.join("src/.generated")).unwrap();
    fs::write(
        dir.join("src/.generated/stale.rs"),
        "// stale, from a removed module",
    )
    .unwrap();
    fs::write(
        dir.join("src/.generated/notes.txt"),
        "not managed by outou build",
    )
    .unwrap();

    let report = build(&BuildOptions::new(&dir)).expect("mixed builds");

    assert!(report
        .removed_files
        .contains(&dir.join("src/.generated/stale.rs")));
    assert!(!dir.join("src/.generated/stale.rs").exists());
    assert!(dir.join("src/.generated/notes.txt").exists());

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_second_build_overwrites_atomically_and_stays_deterministic() {
    let dir = copy_to_temp(&fixtures_dir().join("mixed"), "atomic-rebuild");
    let first = build(&BuildOptions::new(&dir)).expect("first build");
    let first_text = fs::read_to_string(dir.join("src/.generated/crate-root.rs")).unwrap();

    let second = build(&BuildOptions::new(&dir)).expect("second build");
    let second_text = fs::read_to_string(dir.join("src/.generated/crate-root.rs")).unwrap();

    assert_eq!(first_text, second_text);
    assert_eq!(first.generated_files.len(), second.generated_files.len());
    assert!(second.removed_files.is_empty());

    fs::remove_dir_all(&dir).ok();
}

/// `outou build` on a `.rsx` file with a syntax error must exit 1 and
/// print only Outou vocabulary — never backend vocabulary (`rsx!`,
/// `PropsBuilder`, `dioxus_rsx`, `GeneratedNode`, `dioxus`).
#[test]
fn cli_binary_exits_1_and_prints_only_outou_vocabulary_on_a_syntax_error() {
    let dir = std::env::temp_dir().join(format!(
        "outou-cli-build-it-syntax-error-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src/main.rsx"), "fn f() { <div cl").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_outou"))
        .arg("build")
        .arg("--manifest-dir")
        .arg(&dir)
        .output()
        .expect("running the `outou` binary");

    fs::remove_dir_all(&dir).ok();

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    for forbidden in [
        "rsx!",
        "PropsBuilder",
        "dioxus_rsx",
        "GeneratedNode",
        "dioxus",
    ] {
        assert!(
            !stdout.contains(forbidden) && !stderr.contains(forbidden),
            "output must not mention {forbidden:?}\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
}

/// Slower, real-compiler confirmation that the generated `mixed` fixture
/// is accepted by `rustc` itself, not merely well-formed text. Ignored by
/// default (cold `dioxus` build); run explicitly:
///
/// ```sh
/// cargo test -p outou-cli --test build -- --ignored --nocapture
/// ```
#[test]
#[ignore = "builds the full dioxus dependency tree; run explicitly, see module docs"]
fn generated_mixed_fixture_passes_cargo_check() {
    let dir = copy_to_temp(&fixtures_dir().join("mixed"), "cargo-check-mixed");
    build(&BuildOptions::new(&dir)).expect("mixed builds");

    let outou_path = repo_root().join("crates/outou");
    let manifest = format!(
        "[package]\n\
         name = \"outou-cli-it-mixed\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n\
         publish = false\n\
         \n\
         [workspace]\n\
         \n\
         [lib]\n\
         path = \"src/.generated/crate-root.rs\"\n\
         \n\
         [dependencies]\n\
         outou = {{ path = {outou_path:?} }}\n"
    );
    fs::write(dir.join("Cargo.toml"), manifest).unwrap();

    let status = Command::new(env!("CARGO"))
        .arg("check")
        .current_dir(&dir)
        .env("CARGO_TARGET_DIR", repo_root().join("target"))
        .status()
        .expect("running `cargo check` on the generated mixed fixture");

    fs::remove_dir_all(&dir).ok();
    assert!(status.success(), "cargo check failed for `mixed`");
}

/// Same as above, for the full example app. Ignored for the same reason.
#[test]
#[ignore = "builds the full dioxus dependency tree; run explicitly, see module docs"]
fn generated_example_app_passes_cargo_check_via_outou_build() {
    let dir = copy_to_temp(&repo_root().join("examples/phase0-app"), "cargo-check-app");
    build(&BuildOptions::new(&dir)).expect("phase0-app builds");

    // The copied `Cargo.toml`'s `outou = { path = "../../crates/outou" }`
    // is relative to the *original* location; rewrite it to an absolute
    // path so it still resolves from the temp copy.
    let outou_path = repo_root().join("crates/outou");
    let manifest = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    let manifest = manifest.replace(
        "path = \"../../crates/outou\"",
        &format!("path = {outou_path:?}"),
    );
    fs::write(dir.join("Cargo.toml"), manifest).unwrap();

    let status = Command::new(env!("CARGO"))
        .arg("check")
        .current_dir(&dir)
        .env("CARGO_TARGET_DIR", repo_root().join("target"))
        .status()
        .expect("running `cargo check` on the generated example app");

    fs::remove_dir_all(&dir).ok();
    assert!(status.success(), "cargo check failed for the example app");
}

/// Issue #8's clippy requirement, the other half: generated code's own
/// `#![allow(unused_braces)]` (`crates/outou-backend-dioxus`'s
/// `GENERATED_LINT_ALLOWS`) must not silence a lint on the *user's own*
/// expression sitting in the same generated file. Injects a deliberately
/// unused local (`let probe_unused_variable = 1 + 1;`) into a copy of the
/// example app's `App` component, builds it, and runs `cargo clippy
/// --all-targets -- -D warnings` on the result: it must fail, and the
/// failure must name the injected variable — proof the lint fired at the
/// user's own code, not that the whole build merely broke some other way.
/// Ignored for the same reason as the `cargo check` probes above (cold
/// `dioxus` build).
#[test]
#[ignore = "builds the full dioxus dependency tree; run explicitly, see module docs"]
fn user_expressions_keep_their_lints_under_generated_code_allows() {
    let dir = copy_to_temp(&repo_root().join("examples/phase0-app"), "clippy-probe");

    let main_rsx_path = dir.join("src/main.rsx");
    let main_rsx = fs::read_to_string(&main_rsx_path).unwrap();
    let probed = main_rsx.replacen(
        "fn App() -> Element {\n    let user = load_user();",
        "fn App() -> Element {\n    let probe_unused_variable = 1 + 1;\n    let user = load_user();",
        1,
    );
    assert_ne!(probed, main_rsx, "the App() anchor text must still exist");
    fs::write(&main_rsx_path, probed).unwrap();

    build(&BuildOptions::new(&dir)).expect("probed app still builds");

    let outou_path = repo_root().join("crates/outou");
    let manifest = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    let manifest = manifest.replace(
        "path = \"../../crates/outou\"",
        &format!("path = {outou_path:?}"),
    );
    fs::write(dir.join("Cargo.toml"), manifest).unwrap();

    let output = Command::new(env!("CARGO"))
        .arg("clippy")
        .arg("--all-targets")
        .arg("--")
        .arg("-D")
        .arg("warnings")
        .current_dir(&dir)
        .env("CARGO_TARGET_DIR", repo_root().join("target"))
        .output()
        .expect("running `cargo clippy` on the probed example app");

    fs::remove_dir_all(&dir).ok();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "expected clippy to fail on the injected unused variable:\n{stderr}"
    );
    assert!(
        stderr.contains("probe_unused_variable"),
        "clippy's failure must name the injected variable, proving the lint fired on user code, \
         not merely that the build broke some other way:\n{stderr}"
    );
}
