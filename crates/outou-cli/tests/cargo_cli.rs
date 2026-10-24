//! Integration tests for `outou cargo <build|check|clippy|test>` (issue
//! #25, A5; `docs/backend-leakage.md` row 39).
//!
//! Every test runs the real `outou` binary against a copy of the
//! `tests/fixtures/modules/mixed` crate, with `$CARGO` pointing at a tiny
//! shell script instead of the real Cargo. The script records the
//! arguments it was given, prints canned `--message-format=json` output
//! (built here from the real generated file, so every byte offset is the
//! one rustc would report) and exits with a chosen code. That keeps these
//! tests fast and independent of a Dioxus build; the genuine rustc round
//! trip is covered by the `#[ignore]`d tests in `cargo_cli_real.rs`.

#![cfg(unix)]

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::{json, Value};

use support::{copy_to_temp, fixtures_dir};

/// Substrings that must never reach a user (`tests/leak.rs`'s list).
const FORBIDDEN: &[&str] = &[
    ".generated",
    "dioxus",
    "rsx!",
    "PropsBuilder",
    "IntoDynNode",
    "dioxus-core-macro",
];

struct Fixture {
    dir: PathBuf,
    fake_cargo: PathBuf,
    args_file: PathBuf,
    stdout_file: PathBuf,
}

impl Fixture {
    /// A built copy of the `mixed` fixture plus a fake `cargo`.
    fn new(label: &str) -> Self {
        let dir = copy_to_temp(&fixtures_dir().join("mixed"), label);
        let status = Command::new(env!("CARGO_BIN_EXE_outou"))
            .args(["build", "--manifest-dir"])
            .arg(&dir)
            .output()
            .expect("running outou build");
        assert!(status.status.success(), "outou build failed");

        let fake_cargo = dir.join("fake-cargo.sh");
        let args_file = dir.join("fake-cargo.args");
        let stdout_file = dir.join("fake-cargo.stdout");
        fs::write(
            &fake_cargo,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$FAKE_ARGS_FILE\"\ncat \"$FAKE_STDOUT_FILE\"\nexit \"$FAKE_EXIT\"\n",
        )
        .unwrap();
        fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(&stdout_file, "").unwrap();
        Self {
            dir,
            fake_cargo,
            args_file,
            stdout_file,
        }
    }

    fn set_cargo_stdout(&self, lines: &[String]) {
        fs::write(&self.stdout_file, lines.join("\n") + "\n").unwrap();
    }

    fn run(&self, args: &[&str], exit: i32) -> Output {
        Command::new(env!("CARGO_BIN_EXE_outou"))
            .arg("cargo")
            .args(args)
            .current_dir(&self.dir)
            .env("CARGO", &self.fake_cargo)
            .env("FAKE_ARGS_FILE", &self.args_file)
            .env("FAKE_STDOUT_FILE", &self.stdout_file)
            .env("FAKE_EXIT", exit.to_string())
            .env("NO_COLOR", "1")
            .output()
            .expect("running the `outou` binary")
    }

    fn cargo_args(&self) -> Option<Vec<String>> {
        fs::read_to_string(&self.args_file)
            .ok()
            .map(|text| text.lines().map(str::to_string).collect())
    }

    fn manifest_path(&self) -> String {
        self.dir.join("Cargo.toml").display().to_string()
    }

    fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.dir.join(relative)).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.dir).ok();
    }
}

/// 1-based `(line, char column)` of the first `needle` in `text`.
fn line_col(text: &str, needle: &str) -> (usize, usize) {
    let offset = text.find(needle).expect("needle in text");
    let before = &text[..offset];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().unwrap().chars().count() + 1;
    (line, col)
}

/// A rustc JSON span for the first `needle` in `text`, which is the
/// content of the file named `file_name`.
fn span(file_name: &str, text: &str, needle: &str, primary: bool, label: Option<&str>) -> Value {
    let start = text.find(needle).expect("needle in generated text");
    let end = start + needle.len();
    let (line, col) = line_col(text, needle);
    let line_text = text.lines().nth(line - 1).unwrap();
    json!({
        "file_name": file_name,
        "byte_start": start,
        "byte_end": end,
        "line_start": line,
        "line_end": line,
        "column_start": col,
        "column_end": col + needle.chars().count(),
        "is_primary": primary,
        "label": label,
        "suggested_replacement": null,
        "suggestion_applicability": null,
        "expansion": null,
        "text": [{
            "text": line_text,
            "highlight_start": col,
            "highlight_end": col + needle.chars().count(),
        }],
    })
}

fn message(
    manifest: &str,
    level: &str,
    code: Option<&str>,
    text: &str,
    spans: Vec<Value>,
) -> String {
    message_with_children(manifest, level, code, text, spans, Vec::new())
}

/// A child `note:` / `help:` of a diagnostic.
fn child(level: &str, text: &str, spans: Vec<Value>) -> Value {
    json!({"level": level, "message": text, "code": null, "spans": spans, "children": [], "rendered": null})
}

/// A span in a file outside the workspace (a registry dependency).
fn external_span(label: Option<&str>) -> Value {
    json!({
        "file_name": "/home/u/.cargo/registry/src/index.crates.io-1/outou-0.0.1/src/state.rs",
        "byte_start": 0, "byte_end": 4, "line_start": 39, "line_end": 39,
        "column_start": 1, "column_end": 5, "is_primary": false, "label": label,
        "suggested_replacement": null, "suggestion_applicability": null,
        "expansion": null,
        "text": [{"text": "pub fn use_state<T: 'static>(init: impl FnOnce() -> T) -> State<T> {", "highlight_start": 1, "highlight_end": 5}],
    })
}

fn message_with_children(
    manifest: &str,
    level: &str,
    code: Option<&str>,
    text: &str,
    spans: Vec<Value>,
    children: Vec<Value>,
) -> String {
    let rendered = match spans.first() {
        Some(first) => format!(
            "{level}: {text}\n --> {}:{}:{}\n",
            first["file_name"].as_str().unwrap(),
            first["line_start"],
            first["column_start"]
        ),
        None => format!("{level}: {text}\n"),
    };
    json!({
        "reason": "compiler-message",
        "package_id": "mixed 0.0.0 (path+file:///x)",
        "manifest_path": manifest,
        "target": {"kind": ["bin"], "name": "mixed"},
        "message": {
            "$message_type": "diagnostic",
            "message": text,
            "code": code.map(|c| json!({"code": c, "explanation": null})),
            "level": level,
            "spans": spans,
            "children": children,
            "rendered": rendered,
        },
    })
    .to_string()
}

fn assert_no_backend_text(output: &Output) {
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for forbidden in FORBIDDEN {
        assert!(
            !text.contains(forbidden),
            "output must not contain {forbidden:?}:\n{text}"
        );
    }
}

#[test]
fn outou_build_stays_generate_only_and_never_runs_cargo() {
    let fixture = Fixture::new("cargo-cli-build-only");
    let output = Command::new(env!("CARGO_BIN_EXE_outou"))
        .args(["build", "--manifest-dir"])
        .arg(&fixture.dir)
        .env("CARGO", &fixture.fake_cargo)
        .env("FAKE_ARGS_FILE", &fixture.args_file)
        .env("FAKE_STDOUT_FILE", &fixture.stdout_file)
        .env("FAKE_EXIT", "0")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        fixture.cargo_args().is_none(),
        "`outou build` must not invoke cargo"
    );
}

#[test]
fn a_type_error_is_reported_at_the_rsx_line_and_column() {
    let fixture = Fixture::new("cargo-cli-rsx-position");
    let generated = fixture.read("src/.generated/crate-root.rs");
    let rsx = fixture.read("src/main.rsx");
    // `UserCard` inside the verbatim `use` line of `main.rsx`.
    let needle = "user::UserCard";
    let (line, col) = line_col(&rsx, needle);
    fixture.set_cargo_stdout(&[message(
        &fixture.manifest_path(),
        "error",
        Some("E0432"),
        "unresolved import `components::user::UserCard`",
        vec![span(
            "src/.generated/crate-root.rs",
            &generated,
            needle,
            true,
            Some("no `UserCard` in `components::user`"),
        )],
    )]);

    let output = fixture.run(&["check"], 101);

    assert_eq!(output.status.code(), Some(101), "exit code is cargo's");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error[E0432]: unresolved import"),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!("src/main.rsx:{line}:{col}")),
        "expected src/main.rsx:{line}:{col} in:\n{stderr}"
    );
    assert!(stderr.contains("no `UserCard` in `components::user`"));
    assert!(stderr.contains("use components::{button::Button, user::UserCard};"));
    assert_no_backend_text(&output);
}

#[test]
fn cargo_receives_the_json_message_format_and_the_forwarded_arguments() {
    let fixture = Fixture::new("cargo-cli-args");
    fixture.run(&["clippy", "--all-targets", "--", "-D", "warnings"], 0);
    assert_eq!(
        fixture.cargo_args().unwrap(),
        [
            "clippy",
            "--message-format=json-diagnostic-rendered-ansi",
            "--all-targets",
            "--",
            "-D",
            "warnings"
        ]
    );
}

#[test]
fn generation_runs_before_cargo_and_a_syntax_error_stops_it() {
    let fixture = Fixture::new("cargo-cli-strict");
    fs::write(fixture.dir.join("src/main.rsx"), "fn f() { <div cl").unwrap();
    let output = fixture.run(&["check"], 0);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        fixture.cargo_args().is_none(),
        "cargo must not run when generation fails"
    );
    assert_no_backend_text(&output);
}

#[test]
fn lines_that_are_not_cargo_json_pass_through_to_stdout() {
    let fixture = Fixture::new("cargo-cli-passthrough");
    fixture.set_cargo_stdout(&[
        "running 1 test".to_string(),
        r#"{"not":"cargo json"}"#.to_string(),
        r#"{"reason":"compiler-artifact","filenames":["x"]}"#.to_string(),
        "test result: ok. 1 passed".to_string(),
    ]);
    let output = fixture.run(&["test"], 0);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("running 1 test"), "{stdout}");
    assert!(stdout.contains(r#"{"not":"cargo json"}"#), "{stdout}");
    assert!(stdout.contains("test result: ok. 1 passed"), "{stdout}");
    assert!(
        !stdout.contains("compiler-artifact"),
        "cargo's own artifact records are not user output: {stdout}"
    );
    assert!(output.status.success());
}

#[test]
fn fix_is_rejected_before_anything_is_generated_or_run() {
    let fixture = Fixture::new("cargo-cli-fix");
    for args in [
        &["clippy", "--fix"][..],
        &["clippy", "--fix", "--allow-dirty"][..],
    ] {
        let output = fixture.run(args, 0);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("--fix"), "{stderr}");
        assert!(fixture.cargo_args().is_none());
    }
}

#[test]
fn a_user_supplied_message_format_is_a_clear_error() {
    let fixture = Fixture::new("cargo-cli-message-format");
    for args in [
        &["check", "--message-format=json"][..],
        &["check", "--message-format", "short"][..],
    ] {
        let output = fixture.run(args, 0);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("--message-format"), "{stderr}");
        assert!(fixture.cargo_args().is_none());
    }
}

#[test]
fn arguments_after_the_double_dash_are_not_inspected() {
    let fixture = Fixture::new("cargo-cli-after-dashes");
    let output = fixture.run(&["test", "--", "--fix"], 0);
    assert!(output.status.success());
    assert!(fixture.cargo_args().is_some());
}

#[test]
fn a_plain_rust_file_path_is_normalised_and_backend_spans_are_dropped() {
    let fixture = Fixture::new("cargo-cli-plain");
    let button = fixture.read("src/components/button.rs");
    let generated = fixture.read("src/.generated/components/user.rs");
    // Plain `.rs`: Cargo reports it relative to the `#[path]` that
    // reached it, through `.generated/..`.
    let plain_name = "src/.generated/../components/button.rs";
    let needle = button
        .split_whitespace()
        .find(|word| word.len() > 3 && word.chars().all(char::is_alphanumeric))
        .unwrap()
        .to_string();
    let plain = span(plain_name, &button, &needle, true, Some("here"));

    // A secondary span whose expansion points into a registry crate.
    let mut backend = span(
        "src/.generated/components/user.rs",
        &generated,
        "::outou::__private::rsx! { div { class: \"user\", {::outou::__private::child(name)}, } }",
        false,
        Some("required by a bound introduced by this call"),
    );
    backend["expansion"] = json!({
        "span": backend.clone(),
        "macro_decl_name": "rsx!",
        "def_site_span": {
            "file_name": "/home/u/.cargo/registry/src/index.crates.io-1/dioxus-core-macro-0.7.10/src/lib.rs",
            "byte_start": 0, "byte_end": 1, "line_start": 1, "line_end": 1,
            "column_start": 1, "column_end": 2, "is_primary": false, "label": null,
            "suggested_replacement": null, "suggestion_applicability": null,
            "expansion": null, "text": [],
        },
    });
    let name = span(
        "src/.generated/components/user.rs",
        &generated,
        "{::outou::__private::child(name)}",
        true,
        None,
    );

    fixture.set_cargo_stdout(&[
        message(
            &fixture.manifest_path(),
            "warning",
            Some("dead_code"),
            "function is never used",
            vec![plain],
        ),
        message(
            &fixture.manifest_path(),
            "error",
            Some("E0277"),
            "something is wrong with this value",
            vec![name, backend],
        ),
    ]);

    let output = fixture.run(&["check"], 101);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("src/components/button.rs:"),
        "plain path is normalised:\n{stderr}"
    );
    let (user_line, _) = line_col(&fixture.read("src/components/user.rsx"), "{name}");
    assert!(
        stderr.contains(&format!("src/components/user.rsx:{user_line}:")),
        "{stderr}"
    );
    assert_no_backend_text(&output);
}

#[test]
fn a_backend_worded_message_is_translated() {
    let fixture = Fixture::new("cargo-cli-translate");
    let generated = fixture.read("src/.generated/components/user.rs");
    fixture.set_cargo_stdout(&[message(
        &fixture.manifest_path(),
        "error",
        Some("E0277"),
        "the trait bound `i32: IntoDynNode<_>` is not satisfied",
        vec![span(
            "src/.generated/components/user.rs",
            &generated,
            "{::outou::__private::child(name)}",
            true,
            Some("the trait `IntoDynNode` is not implemented"),
        )],
    )]);
    let output = fixture.run(&["check"], 101);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("this value cannot be used as element content here"),
        "{stderr}"
    );
    assert_no_backend_text(&output);
}

#[test]
fn help_distinguishes_outou_cargo_from_outou_check() {
    let top = Command::new(env!("CARGO_BIN_EXE_outou"))
        .arg("--help")
        .output()
        .unwrap();
    let top = String::from_utf8_lossy(&top.stdout);
    assert!(top.contains("cargo"), "{top}");

    let check = Command::new(env!("CARGO_BIN_EXE_outou"))
        .args(["check", "--help"])
        .output()
        .unwrap();
    let check = String::from_utf8_lossy(&check.stdout);
    assert!(check.contains("outou cargo check"), "{check}");

    let cargo = Command::new(env!("CARGO_BIN_EXE_outou"))
        .args(["cargo", "--help"])
        .output()
        .unwrap();
    let cargo = String::from_utf8_lossy(&cargo.stdout);
    assert!(cargo.contains("outou check"), "{cargo}");
    assert!(cargo.contains("--fix"), "{cargo}");
}

// ---------------------------------------------------------------------
// Review fixes (issue #25): paths, plain renderings, twins, output.
// ---------------------------------------------------------------------

/// Runs `outou cargo check` over the compiler messages of a recorded real
/// rustc run (`tests/fixtures/cargo-json/<case>.jsonl`). The recorded file
/// names point at a project that is not the fixture, so no source map is
/// found: the invariant under test is "nothing of the generated layout or
/// the backend reaches the user", whether or not a position can be given.
fn run_recorded(case: &str) -> (String, Output) {
    let fixture = Fixture::new(&format!("cargo-cli-rec-{case}"));
    let recorded = fs::read_to_string(
        support::repo_root()
            .join("tests/fixtures/cargo-json")
            .join(format!("{case}.jsonl")),
    )
    .unwrap();
    fixture.set_cargo_stdout(&recorded.lines().map(str::to_string).collect::<Vec<_>>());
    let output = fixture.run(&["check"], 101);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (text, output)
}

const RECORDED_CASES: &[&str] = &[
    "include-str-missing",
    "closure-long-arg",
    "closure-mismatch",
    "component-prop-types",
    "plain-module-bounds",
    "component-derive-hints",
    "deprecated-and-missing-props",
    "display-and-missing-prop",
];

/// Things the review found in real output.
const RECORDED_LEAKS: &[&str] = &[
    ".generated",
    "registry",
    "dioxus-",
    "long-type",
    "call_with_explicit_closure",
    "method defined here",
    "component_property",
    "the return type of this call",
    "the full name for the type",
    "required by a bound in `outou::prelude::use_state`",
];

#[test]
fn recorded_rustc_output_never_shows_the_generated_layout_or_dependencies() {
    for case in RECORDED_CASES {
        let (text, output) = run_recorded(case);
        assert!(!text.is_empty(), "{case}: no output");
        assert_no_backend_text(&output);
        for leak in RECORDED_LEAKS {
            assert!(
                !text.contains(leak),
                "{case}: output must not contain {leak:?}:\n{text}"
            );
        }
    }
}

#[test]
fn a_closure_type_in_a_message_is_mapped_to_the_rsx_position() {
    let fixture = Fixture::new("cargo-cli-closure");
    let generated = fixture.read("src/.generated/components/user.rs");
    // The expression `name` inside the generated `child(name)` call.
    let (gline, gcol) = line_col(&generated, "name)}");
    let (rline, rcol) = line_col(&fixture.read("src/components/user.rsx"), "{name}");
    let closure = format!(
        "{{closure@src/.generated/components/user.rs:{gline}:{gcol}: {gline}:{}}}",
        gcol + 4
    );
    let lost = "{closure@src/.generated/nowhere.rs:1:2: 1:5}";
    fixture.set_cargo_stdout(&[message_with_children(
        &fixture.manifest_path(),
        "error",
        Some("E0308"),
        &format!("mismatched types, found closure `{closure}`"),
        vec![span(
            "src/.generated/components/user.rs",
            &generated,
            "{::outou::__private::child(name)}",
            true,
            Some(&format!("found closure `{closure}`")),
        )],
        vec![child(
            "note",
            &format!("expected type `u32`\nfound closure `{closure}` and `{lost}`"),
            vec![],
        )],
    )]);
    let output = fixture.run(&["check"], 101);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The mapping of `{name}` is its expression, one column in.
    let mapped = format!("src/components/user.rsx:{rline}:{}", rcol + 1);
    assert!(
        stderr.matches(&format!("{{closure@{mapped}")).count() >= 3,
        "message, label and note all map the closure to {mapped}:\n{stderr}"
    );
    assert!(stderr.contains("{closure@<generated code>}"), "{stderr}");
    assert_no_backend_text(&output);
}

#[test]
fn an_include_str_path_is_hidden_and_the_resolution_rule_is_explained() {
    let fixture = Fixture::new("cargo-cli-include-str");
    let generated = fixture.read("src/.generated/components/user.rs");
    let mut primary = span(
        "src/.generated/components/user.rs",
        &generated,
        "{::outou::__private::child(name)}",
        true,
        None,
    );
    primary["expansion"] = json!({
        "span": primary.clone(),
        "macro_decl_name": "include_str!",
        "def_site_span": null,
    });
    fixture.set_cargo_stdout(&[message(
        &fixture.manifest_path(),
        "error",
        None,
        "couldn't read `src/.generated/components/missing.txt`: No such file or directory (os error 2)",
        vec![primary],
    )]);
    let output = fixture.run(&["check"], 101);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("missing.txt"), "{stderr}");
    assert!(
        stderr.contains("relative to the directory of the generated Rust"),
        "the note explains how include_str! paths resolve:\n{stderr}"
    );
    assert_no_backend_text(&output);
}

#[test]
fn a_dependency_note_and_a_macro_note_are_not_shown_for_plain_rust() {
    let fixture = Fixture::new("cargo-cli-plain-deps");
    let button = fixture.read("src/components/button.rs");
    let needle = button
        .split_whitespace()
        .find(|word| word.len() > 3 && word.chars().all(char::is_alphanumeric))
        .unwrap()
        .to_string();
    let plain_name = "src/.generated/../components/button.rs";
    let primary = span(plain_name, &button, &needle, true, Some("here"));
    let mut rendered_leak = message_with_children(
        &fixture.manifest_path(),
        "error",
        Some("E0277"),
        "expected an `FnOnce()` closure, found `{integer}`",
        vec![
            primary,
            external_span(Some("required by this bound in `use_state`")),
        ],
        vec![
            child("note", "a plain note", vec![]),
            child(
                "note",
                "required by a bound in `outou::prelude::use_state`",
                vec![external_span(Some("required by this bound"))],
            ),
        ],
    );
    // rustc's own rendering names the dependency and prints its source.
    let mut value: Value = serde_json::from_str(&rendered_leak).unwrap();
    value["message"]["rendered"] = json!(
        "error[E0277]: expected an `FnOnce()` closure\n --> src/components/button.rs:1:1\n  |\n  = note: required by a bound in `use_state`\n --> /home/u/.cargo/registry/src/index.crates.io-1/outou-0.0.1/src/state.rs:39:1\n   |\n39 | pub fn use_state<T: 'static>(init: impl FnOnce() -> T) -> State<T> {\n"
    );
    rendered_leak = value.to_string();
    fixture.set_cargo_stdout(&[rendered_leak]);
    let output = fixture.run(&["check"], 101);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("src/components/button.rs:"), "{stderr}");
    assert!(stderr.contains("a plain note"), "{stderr}");
    for leak in ["registry", "outou-0.0.1", "use_state"] {
        assert!(!stderr.contains(leak), "{leak:?} leaked:\n{stderr}");
    }
    assert_no_backend_text(&output);
}

/// The two diagnostics rustc reports for one missing required prop, at the
/// same generated span: the translated deprecation warning and the
/// untranslated E0061.
fn builder_pair(fixture: &Fixture, generated: &str, needle: &str) -> Vec<String> {
    let file = "src/.generated/components/user.rs";
    vec![
        message(
            &fixture.manifest_path(),
            "warning",
            Some("deprecated"),
            "use of deprecated method `UserCardPropsBuilder::<((), __other)>::build`: Missing required field name",
            vec![span(file, generated, needle, true, None)],
        ),
        message(
            &fixture.manifest_path(),
            "error",
            Some("E0061"),
            "this method takes 1 argument but 0 arguments were supplied",
            vec![span(
                file,
                generated,
                needle,
                true,
                Some("argument #1 of type `UserCardPropsBuilder_Error_Missing_required_field_name` is missing"),
            )],
        ),
    ]
}

#[test]
fn only_the_builder_twin_is_suppressed_not_a_users_own_diagnostic_at_the_same_position() {
    let fixture = Fixture::new("cargo-cli-twin");
    let generated = fixture.read("src/.generated/components/user.rs");
    let file = "src/.generated/components/user.rs";
    // The user's own deprecation warning maps to the same `.rsx` span as
    // the builder's (a different generated range inside one mapping), and
    // a second user error shares the very same generated range.
    let mut lines = builder_pair(&fixture, &generated, "UserCard");
    lines.push(message(
        &fixture.manifest_path(),
        "warning",
        Some("deprecated"),
        "use of deprecated function `UserCard`: use Card",
        vec![span(file, &generated, "UserCard", true, None)],
    ));
    lines.push(message(
        &fixture.manifest_path(),
        "error",
        Some("E0599"),
        "no field `nope` on type `String`",
        vec![span(file, &generated, "UserCard", true, None)],
    ));
    fixture.set_cargo_stdout(&lines);
    let output = fixture.run(&["check"], 101);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("use of deprecated function `UserCard`"),
        "{stderr}"
    );
    assert!(
        stderr.contains("no field `nope` on type `String`"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("this method takes 1 argument"),
        "the builder twin is dropped:\n{stderr}"
    );
    assert!(stderr.contains("missing a required property"), "{stderr}");
}

#[test]
fn a_builder_twin_without_its_translated_diagnostic_is_never_lost() {
    // rustc reports the translated diagnostic first. If it ever did not,
    // the untranslated one is shown: a duplicate costs less than a hard
    // error that vanishes.
    let fixture = Fixture::new("cargo-cli-twin-order");
    let generated = fixture.read("src/.generated/components/user.rs");
    let mut lines = builder_pair(&fixture, &generated, "UserCard");
    lines.remove(0);
    fixture.set_cargo_stdout(&lines);
    let output = fixture.run(&["check"], 101);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("this method takes 1 argument"), "{stderr}");
}

#[test]
fn a_long_span_that_narrows_exactly_keeps_its_caret_width() {
    let fixture = Fixture::new("cargo-cli-wide-span");
    let generated = fixture.read("src/.generated/crate-root.rs");
    let mut wide = span(
        "src/.generated/crate-root.rs",
        &generated,
        "user::UserCard",
        true,
        Some("here"),
    );
    // rustc says the span is 18 lines long (a closure body); the bytes are
    // inside one verbatim mapping, so they narrow exactly.
    wide["line_end"] = json!(wide["line_start"].as_u64().unwrap() + 17);
    fixture.set_cargo_stdout(&[message(
        &fixture.manifest_path(),
        "error",
        Some("E0308"),
        "mismatched types",
        vec![wide],
    )]);
    let output = fixture.run(&["check"], 101);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("{} here", "^".repeat("user::UserCard".len()))),
        "carets keep the width of the span:\n{stderr}"
    );
}

#[test]
fn a_cargo_shaped_line_in_test_output_survives_after_the_build_finished() {
    let fixture = Fixture::new("cargo-cli-build-finished");
    fixture.set_cargo_stdout(&[
        r#"{"reason":"compiler-artifact","filenames":["x"]}"#.to_string(),
        r#"{"reason":"build-script-executed","package_id":"x"}"#.to_string(),
        r#"{"reason":"build-finished","success":true}"#.to_string(),
        "running 1 test".to_string(),
        r#"{"reason":"build-finished","success":true,"from":"the test"}"#.to_string(),
        r#"{"reason":"compiler-message","from":"the test"}"#.to_string(),
        r#"{"reason":"compiler-artifact","from":"the test"}"#.to_string(),
    ]);
    let output = fixture.run(&["test"], 0);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("running 1 test"), "{stdout}");
    assert_eq!(
        stdout.matches(r#""from":"the test""#).count(),
        3,
        "everything after build-finished is the program's own output:\n{stdout}"
    );
    assert!(
        !stdout.contains(r#""success":true}"#) || stdout.matches("success").count() == 1,
        "cargo's own build-finished record is not user output:\n{stdout}"
    );
}

#[test]
fn an_unknown_reason_is_not_cargos_and_passes_through() {
    let fixture = Fixture::new("cargo-cli-unknown-reason");
    fixture.set_cargo_stdout(&[r#"{"reason":"my-own-event","n":1}"#.to_string()]);
    let output = fixture.run(&["test"], 0);
    assert!(String::from_utf8_lossy(&output.stdout).contains("my-own-event"));
}

#[test]
fn a_cargo_killed_by_a_signal_gives_128_plus_the_signal_number() {
    let fixture = Fixture::new("cargo-cli-signal");
    let killer = fixture.dir.join("kill-cargo.sh");
    fs::write(&killer, "#!/bin/sh\nkill -TERM $$\nsleep 5\n").unwrap();
    fs::set_permissions(&killer, fs::Permissions::from_mode(0o755)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_outou"))
        .args(["cargo", "check"])
        .current_dir(&fixture.dir)
        .env("CARGO", &killer)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(128 + 15));
}

#[test]
fn an_insertion_suggestion_says_insert_not_replace_nothing() {
    let fixture = Fixture::new("cargo-cli-insert");
    let generated = fixture.read("src/.generated/crate-root.rs");
    let primary = span(
        "src/.generated/crate-root.rs",
        &generated,
        "user::UserCard",
        true,
        None,
    );
    let start = generated.find("user::UserCard").unwrap();
    let mut insertion = span(
        "src/.generated/crate-root.rs",
        &generated,
        "user::UserCard",
        true,
        None,
    );
    insertion["byte_start"] = json!(start + 4);
    insertion["byte_end"] = json!(start + 4);
    insertion["suggested_replacement"] = json!("(/* u32 */)");
    fixture.set_cargo_stdout(&[message_with_children(
        &fixture.manifest_path(),
        "error",
        Some("E0308"),
        "mismatched types",
        vec![primary],
        vec![child(
            "help",
            "use parentheses to call this closure",
            vec![insertion],
        )],
    )]);
    let output = fixture.run(&["check"], 101);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("use parentheses to call this closure: insert `(/* u32 */)`"),
        "{stderr}"
    );
    assert!(!stderr.contains("replace `` with"), "{stderr}");
}

#[test]
fn explanation_notes_list_only_the_codes_that_were_shown() {
    let fixture = Fixture::new("cargo-cli-explain");
    let generated = fixture.read("src/.generated/components/user.rs");
    let mut lines = builder_pair(&fixture, &generated, "UserCard");
    lines.push(message(
        &fixture.manifest_path(),
        "error",
        Some("E0308"),
        "mismatched types",
        vec![span(
            "src/.generated/components/user.rs",
            &generated,
            "{::outou::__private::child(name)}",
            true,
            None,
        )],
    ));
    for text in [
        "Some errors have detailed explanations: E0061, E0308.",
        "For more information about an error, try `rustc --explain E0061`.",
    ] {
        lines.push(message(
            &fixture.manifest_path(),
            "failure-note",
            None,
            text,
            vec![],
        ));
    }
    fixture.set_cargo_stdout(&lines);
    let output = fixture.run(&["check"], 101);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("E0061"),
        "the suppressed code is not listed:\n{stderr}"
    );
    assert!(
        stderr.contains("For more information about this error, try `rustc --explain E0308`."),
        "{stderr}"
    );
    assert!(!stderr.contains("Some errors have"), "{stderr}");
}
