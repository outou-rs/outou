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
/// checklist), a *literal* list rather than `BACKEND_MARKERS` itself
/// (see [`forbidden`]): two entries here have no message-substring
/// equivalent at all (`rsx! macro`, the exact phrase `AGENTS.md` uses, and
/// `.generated/`, a path fragment rather than a message substring).
const EXTRA_FORBIDDEN: &[&str] = &["rsx! macro", "dioxus", ".generated/"];

/// The full forbidden-substring list this test scans rendered output
/// for: every entry in [`EXTRA_FORBIDDEN`] plus every entry in
/// `outou_syntax::vocabulary::BACKEND_MARKERS`.
///
/// This module doc used to claim a hand-maintained `FORBIDDEN` constant
/// was "a superset of `BACKEND_MARKERS`" without actually deriving it from
/// that table — six markers (`Properties`, `__template`, `_completions`,
/// `typed_builder`, `Usage in rsx`, `ChildComponent`) were added to
/// `BACKEND_MARKERS` over time and never mirrored here, so a rendered
/// diagnostic leaking one of them would have passed this scan undetected
/// (F5, issue #12 corpus review). Deriving the list directly, rather than
/// re-asserting the superset property as a separate check, makes that
/// drift structurally impossible.
fn forbidden() -> Vec<&'static str> {
    let mut markers: Vec<&'static str> = EXTRA_FORBIDDEN.to_vec();
    markers.extend(vocabulary::BACKEND_MARKERS.iter().copied());
    markers
}

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
    for marker in forbidden() {
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
/// *reliable* substring in `outou_syntax::vocabulary::BACKEND_MARKERS`
/// (every entry except `vocabulary::AMBIGUOUS_MARKERS`, F12) is swept
/// through a synthetic message, and the translated result must contain
/// none of them — whether it got a specific rewrite or fell back to the
/// generic message. This is the comprehensive form of the guarantee;
/// [`translation_table_handles_every_message_actually_observed_from_rustc`]
/// below additionally pins the exact messages this repository has seen
/// `rustc` produce for generated code (docs/backend-leakage.md rows 12,
/// 19, 20).
#[test]
fn every_reliable_backend_marker_translates_to_a_marker_free_message() {
    for marker in vocabulary::BACKEND_MARKERS {
        if vocabulary::AMBIGUOUS_MARKERS.contains(marker) {
            continue;
        }
        let synthetic = format!("a rustc diagnostic mentioning {marker} somewhere in its text");
        let translated = vocabulary::translate_message(&synthetic);
        assert!(
            !vocabulary::contains_backend_marker(&translated),
            "translating a message containing marker {marker:?} left backend vocabulary behind: {translated:?}"
        );
    }
}

/// F12's other half: a message whose *only* marker is one of
/// `vocabulary::AMBIGUOUS_MARKERS` must be left completely unchanged by
/// `translate_message` rather than replaced (see
/// `outou_syntax::vocabulary`'s own tests for the same guarantee at the
/// unit level; this pins it from `outou-cli`'s side of the shared table
/// too).
#[test]
fn every_ambiguous_marker_alone_is_left_untouched_by_translate_message() {
    for marker in vocabulary::AMBIGUOUS_MARKERS {
        let synthetic = format!("a rustc diagnostic mentioning {marker} somewhere in its text");
        assert_eq!(vocabulary::translate_message(&synthetic), synthetic);
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
