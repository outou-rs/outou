//! `outou cargo` against the real Rust compiler (issue #25 acceptance):
//! `examples/dogfood-board` with a deliberate type error must report the
//! error at the `.rsx` line and column, with no `src/.generated` path and
//! no backend vocabulary.
//!
//! `#[ignore]`d like the other tests that build the Dioxus dependency tree
//! (a cold `target/` takes minutes). Run explicitly with:
//!
//! ```sh
//! cargo test -p outou-cli --test cargo_cli_real -- --ignored --nocapture
//! ```
//!
//! The example is copied to a temporary directory; the repository's own
//! `examples/dogfood-board` is never modified.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

/// Substrings that must never reach a user (`tests/leak.rs`'s list).
const FORBIDDEN: &[&str] = &[
    ".generated",
    "dioxus",
    "rsx!",
    "PropsBuilder",
    "IntoDynNode",
    "dioxus_elements",
    "registry",
    "long-type",
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root exists")
}

/// Copies the dogfood board without its build products.
fn copy_board(src: &Path, dest: &Path) {
    fs::create_dir_all(dest).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name == "target" || name == ".generated" {
            continue;
        }
        let target = dest.join(&name);
        if entry.file_type().unwrap().is_dir() {
            copy_board(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

fn temp_board() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "outou-cli-cargo-real-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    copy_board(&repo_root().join("examples/dogfood-board"), &dir);
    let dir = dir.canonicalize().unwrap();
    // The example reaches the facade through a relative path.
    let manifest = dir.join("app/Cargo.toml");
    let text = fs::read_to_string(&manifest).unwrap();
    let outou_path = repo_root().join("crates/outou");
    fs::write(
        &manifest,
        text.replace(
            "path = \"../../../crates/outou\"",
            &format!("path = {outou_path:?}"),
        ),
    )
    .unwrap();
    dir
}

fn outou_cargo(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_outou"))
        .arg("cargo")
        .args(args)
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", repo_root().join("target"))
        .env("NO_COLOR", "1")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .output()
        .expect("running `outou cargo`")
}

fn assert_no_backend_text(text: &str) {
    for forbidden in FORBIDDEN {
        assert!(
            !text.contains(forbidden),
            "output must not contain {forbidden:?}:\n{text}"
        );
    }
}

#[test]
#[ignore = "builds the full dioxus dependency tree; run explicitly, see module docs"]
fn dogfood_board_type_error_is_reported_at_the_rsx_position_and_a_clean_board_passes() {
    let dir = temp_board();

    let clean = outou_cargo(&dir, &["clippy", "--", "-D", "warnings"]);
    assert!(
        clean.status.success(),
        "a clean board passes `outou cargo clippy -- -D warnings`:\n{}",
        String::from_utf8_lossy(&clean.stderr)
    );

    let file = dir.join("app/src/task_list.rsx");
    let source = fs::read_to_string(&file).unwrap();
    let anchor = "    let id = task.id;\n";
    assert!(
        source.contains(anchor),
        "the example changed; update this test"
    );

    // A type error, and a closure where a number is expected: the second
    // one names the closure's position in rustc's own prose
    // (`found closure `{closure@...}``).
    let cases = [
        ("    let wrong: u32 = \"oops\";\n", "\"oops\"", "E0308"),
        ("    let wrong: u32 = || 1;\n", "|| 1", "E0308"),
    ];
    let mut outputs = Vec::new();
    for (statement, needle, _) in cases {
        let broken = source.replacen(anchor, &format!("{anchor}{statement}"), 1);
        fs::write(&file, &broken).unwrap();
        let offset = broken.find(needle).unwrap();
        let line = broken[..offset].matches('\n').count() + 1;
        let column = broken[..offset]
            .rsplit('\n')
            .next()
            .unwrap()
            .chars()
            .count()
            + 1;
        outputs.push((line, column, outou_cargo(&dir, &["check"])));
    }

    fs::remove_dir_all(&dir).ok();

    for ((line, column, output), (_, _, code)) in outputs.iter().zip(cases) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(!output.status.success(), "{stderr}");
        assert!(
            stderr.contains(&format!("error[{code}]: mismatched types")),
            "{stderr}"
        );
        assert!(
            stderr.contains(&format!("app/src/task_list.rsx:{line}:{column}")),
            "expected app/src/task_list.rsx:{line}:{column} in:\n{stderr}"
        );
        assert_no_backend_text(&stderr);
        assert_no_backend_text(&stdout);
    }
}
