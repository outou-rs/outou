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
//! `CARGO_TARGET_DIR` is honoured (the repository's `target/` otherwise);
//! point it at an empty directory to test against a cold build. Cargo's
//! progress lines are not part of what is checked.
//!
//! The example is copied to a temporary directory; the repository's own
//! `examples/dogfood-board` is never modified.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

/// Both tests build into one shared target directory. Cargo serializes
/// that with a file lock and prints `Blocking waiting for file lock`; this
/// lock makes the tests take turns instead, so the output of one never
/// depends on what the other is compiling.
static SHARED_TARGET: Mutex<()> = Mutex::new(());

fn lock_shared_target() -> MutexGuard<'static, ()> {
    SHARED_TARGET.lock().unwrap_or_else(|e| e.into_inner())
}

/// Cargo's own status words at the start of a progress line
/// (`   Compiling dioxus-rsx v0.7.10`). They name every dependency, so a
/// cold `target/` would trip the leak scan on the crate names alone.
const CARGO_STATUS_WORDS: &[&str] = &[
    "Compiling",
    "Checking",
    "Blocking",
    "Downloaded",
    "Downloading",
    "Updating",
    "Locking",
    "Adding",
    "Removing",
    "Finished",
    "Fresh",
    "Building",
    "Fetching",
    "Unpacking",
    "Packaging",
    "Running",
    "Dirty",
];

/// Removes ANSI escape sequences (`\x1b[1m\x1b[92m`): CI sets
/// `CARGO_TERM_COLOR=always`, which puts colour codes in front of Cargo's
/// status words and in the middle of diagnostics.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            // CSI: parameter and intermediate bytes, then one final byte.
            for next in chars.by_ref() {
                if ('\x40'..='\x7e').contains(&next) {
                    break;
                }
            }
        }
    }
    out
}

/// Strips colour codes, then removes Cargo's progress lines, which are not
/// diagnostics. Forbidden words are checked on this result, so a colour
/// code inside a word cannot hide it.
fn without_cargo_progress(text: &str) -> String {
    strip_ansi(text)
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !CARGO_STATUS_WORDS.iter().any(|word| {
                trimmed
                    .strip_prefix(word)
                    .is_some_and(|rest| rest.starts_with(' '))
            })
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Removes the temporary board when dropped, so a failed assertion does
/// not leave it behind.
struct TempBoard(PathBuf);

impl TempBoard {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempBoard {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// The target directory shared by the tests: `CARGO_TARGET_DIR` when the
/// caller set one (a cold build in CI or a check), else the repository's.
fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target"))
}

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

fn temp_board() -> TempBoard {
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
    TempBoard(dir)
}

fn outou_cargo(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_outou"))
        .arg("cargo")
        .args(args)
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", target_dir())
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
    let _target = lock_shared_target();
    let board = temp_board();
    let dir = board.path();

    let clean = outou_cargo(dir, &["clippy", "--", "-D", "warnings"]);
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
        outputs.push((line, column, outou_cargo(dir, &["check"])));
    }

    for ((line, column, output), (_, _, code)) in outputs.iter().zip(cases) {
        let stderr = without_cargo_progress(&String::from_utf8_lossy(&output.stderr));
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
        assert_no_backend_text(&strip_ansi(&stdout));
    }
}

/// Issue #26: a key of an unsupported type is reported through
/// `outou cargo` at the key expression in the `.rsx` file, in Outou's words.
#[test]
#[ignore = "builds the full dioxus dependency tree; run explicitly, see module docs"]
fn a_key_of_an_unsupported_type_is_reported_at_the_key_expression() {
    let _target = lock_shared_target();
    let board = temp_board();
    let dir = board.path();
    let file = dir.join("app/src/task_list.rsx");
    let source = fs::read_to_string(&file).unwrap();
    let anchor = "key={task.id}";
    assert!(
        source.contains(anchor),
        "the example changed; update this test"
    );
    // `Status` is an enum: neither a string nor an integer.
    let broken = source.replacen(anchor, "key={task.status}", 1);
    fs::write(&file, &broken).unwrap();
    let offset = broken.find("task.status}").unwrap();
    let line = broken[..offset].matches('\n').count() + 1;
    let column = broken[..offset]
        .rsplit('\n')
        .next()
        .unwrap()
        .chars()
        .count()
        + 1;

    let output = outou_cargo(dir, &["check"]);

    let stderr = without_cargo_progress(&String::from_utf8_lossy(&output.stderr));
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("cannot be used as a JSX `key`"),
        "expected the Outou message in:\n{stderr}"
    );
    assert!(
        stderr.contains(&format!("app/src/task_list.rsx:{line}:{column}")),
        "expected app/src/task_list.rsx:{line}:{column} in:\n{stderr}"
    );
    assert_no_backend_text(&stderr);
}

#[test]
fn colored_progress_lines_are_removed_and_diagnostics_kept() {
    let colored = "\x1b[1m\x1b[92m   Compiling\x1b[0m dioxus-rsx v0.7.10\n\
\x1b[1m\x1b[92m    Finished\x1b[0m `dev` profile\n\
\x1b[1m\x1b[91merror[E0308]\x1b[0m\x1b[1m: mismatched types\x1b[0m\n\
\x1b[1m\x1b[94m --> \x1b[0mapp/src/task_list.rsx:3:9";
    let filtered = without_cargo_progress(colored);
    assert_eq!(
        filtered,
        "error[E0308]: mismatched types\n --> app/src/task_list.rsx:3:9"
    );
    assert_no_backend_text(&filtered);
}

#[test]
fn a_forbidden_word_split_by_colour_codes_is_still_found() {
    let split = "see di\x1b[1moxus here";
    assert_eq!(strip_ansi(split), "see dioxus here");
    assert!(strip_ansi(split).contains("dioxus"));
}

#[test]
fn strip_ansi_leaves_plain_text_and_a_lone_escape_alone() {
    assert_eq!(strip_ansi("plain 日本語"), "plain 日本語");
    assert_eq!(strip_ansi("a\x1bb"), "ab");
}
