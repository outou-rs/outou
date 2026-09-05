//! Real-compiler confirmation, not merely well-formed text (issue #8):
//! `outou build`'s generated output actually compiles under `rustc`, and
//! generated code's own `#![allow(unused_braces)]` never leaks onto a
//! user's own redundant braces in a file that does not need it (split out
//! of one `tests/build.rs`, issue #8 fix list step 10; fixture-generation
//! and CLI-binary tests live in `build_paths.rs`/`build_cli.rs`).
//!
//! Most of these probes are `#[ignore]`d (a cold `dioxus` build); run them
//! explicitly:
//!
//! ```sh
//! cargo test -p outou-cli --test build_compile -- --ignored --nocapture
//! ```

mod support;

use std::fs;
use std::process::Command;

use outou_cli::build::{build, BuildOptions};

use support::{copy_to_temp, fixtures_dir, repo_root};

/// Builds `fixture_name`, writes a minimal `Cargo.toml` around the
/// generated `crate-root.rs` (no `outou`/`dioxus` dependency: this
/// fixture's own `.rsx` files never emit JSX), and runs `cargo test`,
/// asserting it passes. `label` names the temp directory.
fn assert_generated_fixture_binds_correctly(fixture_name: &str, label: &str) {
    let dir = copy_to_temp(&fixtures_dir().join(fixture_name), label);
    build(&BuildOptions::new(&dir)).expect("fixture builds");

    let manifest = format!(
        "[package]\n\
         name = \"outou-cli-it-{label}\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n\
         publish = false\n\
         \n\
         [workspace]\n\
         \n\
         [lib]\n\
         path = \"src/.generated/crate-root.rs\"\n"
    );
    fs::write(dir.join("Cargo.toml"), manifest).unwrap();

    let output = Command::new(env!("CARGO"))
        .arg("test")
        .current_dir(&dir)
        .env("CARGO_TARGET_DIR", repo_root().join("target"))
        .output()
        .expect("running `cargo test` on the generated fixture");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let success = output.status.success();

    fs::remove_dir_all(&dir).ok();

    assert!(
        success,
        "cargo test failed for fixture {fixture_name}\nstdout: {stdout}\nstderr: {stderr}"
    );
}

/// Real-compiler confirmation, not merely well-formed text: `a::helper`'s
/// `LABEL` must actually be `"a"` and `b::helper`'s `"b"` once `rustc`
/// itself resolves both `#[path]` declarations. Unlike the `cargo check`
/// probes elsewhere in this file, this fixture needs neither `dioxus` nor
/// `outou` (no JSX at all), so it runs as part of the normal suite rather
/// than being `#[ignore]`d.
#[test]
fn generated_inline_dup_fixture_binds_each_sibling_to_its_own_file() {
    assert_generated_fixture_binds_correctly("inline-dup", "cargo-check-inline-dup");
}

/// Same proof for the `.rsx` + `.rs` mixed sibling shape (N2).
#[test]
fn generated_inline_dup_rs_fixture_binds_each_sibling_to_its_own_file() {
    assert_generated_fixture_binds_correctly("inline-dup-rs", "cargo-check-inline-dup-rs");
}

/// Slower, real-compiler confirmation that the generated `mixed` fixture
/// is accepted by `rustc` itself, not merely well-formed text. Ignored by
/// default (cold `dioxus` build); run explicitly, see the module doc.
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

/// Same as above, for the `inline` fixture: this is the test whose
/// absence let HIGH-1 (issue #8) ship — `tests/build_paths.rs`'s own text
/// assertions caught the *wrong* value (`"shell/panel.rs"`) as correct,
/// and nothing actually ran `rustc` over it until this probe existed.
#[test]
#[ignore = "builds the full dioxus dependency tree; run explicitly, see module docs"]
fn generated_inline_fixture_passes_cargo_check() {
    let dir = copy_to_temp(&fixtures_dir().join("inline"), "cargo-check-inline");
    build(&BuildOptions::new(&dir)).expect("inline builds");

    let outou_path = repo_root().join("crates/outou");
    let manifest = format!(
        "[package]\n\
         name = \"outou-cli-it-inline\"\n\
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
        .expect("running `cargo check` on the generated inline fixture");

    fs::remove_dir_all(&dir).ok();
    assert!(status.success(), "cargo check failed for `inline`");
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

/// Issue #8 fix list step 7, decision 4(a): the old probe
/// (`user_expressions_keep_their_lints_under_generated_code_allows`)
/// injected an *unused variable*, which `#![allow(unused_braces)]` was
/// never going to suppress in the first place — it did not actually test
/// the claim in its own name. A real `take({ x })` probe (braces that
/// really are redundant, the exact lint the generated allow silences)
/// shows the corrected claim is true: `examples/phase0-app` has no
/// nested-JSX island prop anywhere, so after step 7's brace-less
/// lowering (decision 4(b)) its generated file carries no
/// `#![allow(unused_braces)]` at all, and a user's own redundant braces
/// in that file are still caught by `cargo clippy -- -D warnings`.
/// Ignored for the same reason as the `cargo check` probes above (cold
/// `dioxus` build).
#[test]
#[ignore = "builds the full dioxus dependency tree; run explicitly, see module docs"]
fn a_users_own_redundant_braces_still_fail_clippy_in_a_file_with_no_lint_allow() {
    let dir = copy_to_temp(
        &repo_root().join("examples/phase0-app"),
        "clippy-braces-probe",
    );

    let main_rsx_path = dir.join("src/main.rsx");
    let main_rsx = fs::read_to_string(&main_rsx_path).unwrap();
    let probed = main_rsx.replacen(
        "fn main() {\n    let _ = App;\n}",
        "fn main() {\n    let _ = App;\n    let _ = take({ 1 });\n}\n\nfn take(x: i32) -> i32 {\n    x\n}",
        1,
    );
    assert_ne!(
        probed, main_rsx,
        "the fn main() anchor text must still exist"
    );
    fs::write(&main_rsx_path, probed).unwrap();

    build(&BuildOptions::new(&dir)).expect("probed app still builds");

    // The generated file must not carry the allow at all: nothing in this
    // file (not even the injected probe, which is plain Rust) needs the
    // island's own synthesized braces.
    let generated = fs::read_to_string(dir.join("src/.generated/crate-root.rs")).unwrap();
    assert!(
        !generated.contains("unused_braces"),
        "no attribute island in this file needs braces, so no allow should be emitted:\n{generated}"
    );

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
        "expected clippy to fail on the user's own redundant braces:\n{stderr}"
    );
    assert!(
        stderr.contains("unused_braces"),
        "clippy's failure must name unused_braces, proving the lint fired on the user's own \
         code in a file with no generated allow:\n{stderr}"
    );
}
