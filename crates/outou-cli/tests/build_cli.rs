//! Integration tests for the `outou` CLI binary itself (issue #8): exit
//! code and vocabulary on a syntax error, and the removed `--mode` flag
//! (split out of one `tests/build.rs`, issue #8 fix list step 10;
//! fixture-generation tests live in `build_paths.rs`, real-compiler
//! probes in `build_compile.rs`).

use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use outou_cli::build::{build, BuildOptions};

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

/// Decision 3 (fix list step 1): `outou build --mode recovery` is removed
/// entirely — `outou build` only ever runs in Strict mode now, and
/// `--mode` is no longer a recognized flag at all.
#[test]
fn cli_rejects_the_removed_mode_flag() {
    let dir = std::env::temp_dir().join(format!(
        "outou-cli-build-it-mode-removed-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src/main.rsx"), "fn main() {}").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_outou"))
        .arg("build")
        .arg("--manifest-dir")
        .arg(&dir)
        .arg("--mode")
        .arg("recovery")
        .output()
        .expect("running the `outou` binary");

    fs::remove_dir_all(&dir).ok();

    assert!(
        !output.status.success(),
        "the `--mode` flag must no longer be accepted"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected argument") || stderr.contains("--mode"),
        "stderr should report the unrecognized `--mode` flag: {stderr}"
    );
}

/// `outou build --mode recovery` used to write invented placeholders into
/// the Cargo target on a syntax error (HIGH-3). With `--mode` removed, a
/// broken `.rsx` crate root must leave `src/.generated/` with no
/// `crate-root.rs` at all — never a placeholder file `cargo build` could
/// silently pick up.
#[test]
fn a_broken_crate_root_leaves_no_generated_file_behind() {
    let dir = std::env::temp_dir().join(format!(
        "outou-cli-build-it-broken-root-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src/main.rsx"), "<div cl").unwrap();

    build(&BuildOptions::new(&dir)).expect_err("broken source fails the build");

    assert!(!dir.join("src/.generated/crate-root.rs").exists());

    fs::remove_dir_all(&dir).ok();
}
