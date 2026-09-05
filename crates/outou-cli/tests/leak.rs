//! Backend-vocabulary leak test (issue #11): scans every rendered Outou
//! diagnostic this repository ships for the exact markers `AGENTS.md`
//! names as a Phase 0 failure. Kept deliberately cheap — no `cargo
//! check`/`dioxus` build here — so it runs on every `cargo test`, unlike
//! `tests/ui.rs`'s `#[ignore]`d build-case test:
//!
//! - Every `tests/ui/<case>/`: a fast (syntax) case is rendered fresh,
//!   in-process, exactly like `tests/ui.rs`'s own
//!   `ui_syntax_cases_match_expected_stderr`. A build case
//!   (`NEEDS_CARGO_CHECK`) is checked against its already-generated,
//!   checked-in `expected.stderr` instead of being rebuilt — rebuilding
//!   needs the full `dioxus` dependency tree, which is exactly what
//!   `tests/ui.rs`'s `ui_build_cases_match_expected_stderr` does (and
//!   blesses that file from); this test only needs to confirm the
//!   *persisted* rendering is leak-free, which needs no compiler
//!   invocation at all. Run `cargo test -p outou-cli --test ui --
//!   --ignored` after touching a semantic/backend case to keep that file
//!   honest.
//! - Every `tests/fixtures/{diagnostics,incomplete}/*.rsx` fixture,
//!   rendered fresh with `outou_syntax::parse`/`render_diagnostics` —
//!   these are pure syntax fixtures, never generated code, so no build
//!   is needed for them either.
//! - The shared translation table itself
//!   (`outou_syntax::vocabulary::translate_message`): every marker in
//!   `BACKEND_MARKERS` is swept through a synthetic message and the
//!   result asserted marker-free, so a marker added to the table without
//!   a rewrite that actually clears it (or without the generic fallback
//!   still applying) is caught here directly, independent of any
//!   fixture happening to reproduce it.

use std::fs;
use std::path::PathBuf;

use outou_syntax::vocabulary;

/// Substrings that must never appear in a user-facing Outou diagnostic
/// (`AGENTS.md`, `docs/phase0/issues/11-diagnostics-ui-tests.md`'s
/// checklist). A superset of `outou_syntax::vocabulary::BACKEND_MARKERS`:
/// that table is what `translate_message` looks for to decide *whether*
/// to rewrite a message; this list is the harder, independent check on
/// the *rendered output* itself, and also covers two markers the
/// translation table has no need to know about (`rsx! macro`, the exact
/// phrase `AGENTS.md` uses, and `.generated/`, a path fragment rather
/// than a message substring).
const FORBIDDEN: &[&str] = &[
    "rsx!",
    "rsx! macro",
    "PropsBuilder",
    "dioxus",
    "dioxus_rsx",
    "GeneratedNode",
    "IntoDynNode",
    "VNode",
    "RenderError",
    "__private",
    ".generated/",
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root exists")
}

/// Checks `text` for every [`FORBIDDEN`] substring, pushing one message
/// per hit onto `failures` rather than panicking immediately — so one
/// run reports every leak found, not just the first.
fn scan(label: &str, text: &str, failures: &mut Vec<String>) {
    for marker in FORBIDDEN {
        if text.contains(marker) {
            failures.push(format!(
                "{label}: rendered output contains forbidden backend vocabulary {marker:?}:\n{text}"
            ));
        }
    }
}

/// Every `tests/ui/<case>` directory, sorted.
fn ui_case_dirs() -> Vec<PathBuf> {
    let dir = repo_root().join("tests/ui");
    let mut dirs: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.join("input.rsx").is_file())
        .collect();
    dirs.sort();
    dirs
}

#[test]
fn ui_cases_never_leak_backend_vocabulary() {
    let mut failures = Vec::new();
    let mut checked = 0usize;
    for case_dir in ui_case_dirs() {
        let name = case_dir.file_name().unwrap().to_string_lossy().to_string();
        if case_dir.join("NEEDS_CARGO_CHECK").is_file() {
            // Checked against the checked-in, already-rendered file
            // rather than rebuilt: see this file's module doc.
            let expected_path = case_dir.join("expected.stderr");
            let text = fs::read_to_string(&expected_path).unwrap_or_else(|e| {
                panic!(
                    "reading {} (run `cargo test -p outou-cli --test ui -- --ignored` first): {e}",
                    expected_path.display()
                )
            });
            scan(
                &format!("tests/ui/{name} (expected.stderr)"),
                &text,
                &mut failures,
            );
        } else {
            let input_path = case_dir.join("input.rsx");
            let source = fs::read_to_string(&input_path)
                .unwrap_or_else(|e| panic!("reading {}: {e}", input_path.display()));
            let report = outou_cli::check::check_source("input.rsx", &source);
            scan(&format!("tests/ui/{name}"), &report.rendered, &mut failures);
        }
        checked += 1;
    }
    assert!(checked > 0, "expected at least one tests/ui case");
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn fixture_diagnostics_never_leak_backend_vocabulary() {
    let mut failures = Vec::new();
    let mut checked = 0usize;
    for sub in ["diagnostics", "incomplete"] {
        let dir = repo_root().join("tests/fixtures").join(sub);
        let mut rsx_files: Vec<PathBuf> = fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("rsx"))
            .collect();
        rsx_files.sort();
        for path in rsx_files {
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
            let file_name = path.file_name().unwrap().to_string_lossy().to_string();
            let parsed = outou_syntax::parse(&source);
            let rendered = parsed.render_diagnostics(&file_name);
            scan(
                &format!("tests/fixtures/{sub}/{file_name}"),
                &rendered,
                &mut failures,
            );
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "expected at least one diagnostics/incomplete fixture"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// Deliverable 3's "unit test on the translation table itself": every
/// substring in `outou_syntax::vocabulary::BACKEND_MARKERS` is swept
/// through a synthetic message, and the translated result must contain
/// none of them — whether it got a specific rewrite or fell back to the
/// generic message. This is the comprehensive form of the guarantee;
/// [`translation_table_handles_every_message_actually_observed_from_rustc`]
/// below additionally pins the exact messages this repository has seen
/// `rustc` produce for generated code (docs/backend-leakage.md rows 12,
/// 19, 20).
#[test]
fn every_backend_marker_translates_to_a_marker_free_message() {
    for marker in vocabulary::BACKEND_MARKERS {
        let synthetic = format!("a rustc diagnostic mentioning {marker} somewhere in its text");
        let translated = vocabulary::translate_message(&synthetic);
        assert!(
            !vocabulary::contains_backend_marker(&translated),
            "translating a message containing marker {marker:?} left backend vocabulary behind: {translated:?}"
        );
    }
}

/// Pins the exact rustc messages this repository has actually observed
/// for generated code (captured while building `tests/ui/backend-*` and
/// `tests/ui/semantic-type-mismatch`, issue #11) against the translation
/// table, so a future change to `KNOWN_REWRITES` or `BACKEND_MARKERS`
/// cannot silently regress one of them.
#[test]
fn translation_table_handles_every_message_actually_observed_from_rustc() {
    let cases = [
        // docs/backend-leakage.md row 12: an unknown element attribute.
        (
            "cannot find value `frobnicate` in module `dioxus_elements::div`",
            "the backend rejected this element; see the generated code",
        ),
        // row 19: a missing required prop's deprecated-`build` warning.
        (
            "use of deprecated method `UserCardPropsBuilder::<((),)>::build`: Missing required field name",
            "this component is missing a required property",
        ),
        // row 19/20: a non-`IntoDynNode` island child.
        (
            "the trait bound `i32: IntoDynNode<outou::prelude::dioxus_core::nodes::FromNodeIterator>` is not satisfied",
            "this value cannot be used as element content here",
        ),
        // A plain rustc message with no backend marker at all — most
        // diagnostics for generated code, per `docs/gate3-results.md` —
        // must pass through completely unchanged.
        ("mismatched types", "mismatched types"),
    ];
    for (observed, expected_translation) in cases {
        assert_eq!(
            vocabulary::translate_message(observed),
            expected_translation
        );
    }
}
