//! [`super::Workspace`] tests: loading, single-unit regeneration and
//! re-planning, against the real `examples/phase0-app` fixture (the same
//! target `tests/gate3.rs` drives through the full LSP protocol).

use std::path::Path;
use std::time::Instant;

use super::*;

/// Gate 3's fixed target program (`docs/phase0.md`, issue #9): a real,
/// multi-file `.rsx` crate, not a synthetic fixture, so these tests
/// exercise the same planner/codegen path the gate3 integration test
/// (`tests/gate3.rs`) drives through the real LSP protocol.
fn phase0_app_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/phase0-app")
        .canonicalize()
        .expect("examples/phase0-app exists")
}

#[test]
fn loads_every_unit_of_the_gate3_fixture() {
    let outcome = Workspace::load(&phase0_app_dir()).expect("plans and generates");
    let LoadOutcome::Planned(workspace) = outcome else {
        panic!("examples/phase0-app has a `.rsx` crate root");
    };
    // `main.rsx` (crate root) and `components.rsx`.
    assert_eq!(workspace.generated.len(), 2);
    assert_eq!(workspace.rsx.len(), 2);
}

/// A single-file edit regenerates only that unit — issue #9's
/// performance budget item ("a single-file edit never regenerates the
/// whole crate") and the architecture note's own requirement.
/// `components.rsx`'s generated text (and declared-module set) must be
/// byte-identical before and after editing `main.rsx`.
#[test]
fn regenerating_one_unit_does_not_touch_another() {
    let outcome = Workspace::load(&phase0_app_dir()).expect("plans and generates");
    let LoadOutcome::Planned(mut workspace) = outcome else {
        panic!("examples/phase0-app has a `.rsx` crate root");
    };
    let main_rsx_uri = workspace
        .rsx
        .keys()
        .find(|uri| uri.ends_with("main.rsx"))
        .expect("main.rsx is a known unit")
        .clone();
    let components_generated_uri = workspace
        .generated
        .keys()
        .find(|uri| uri.ends_with("components.rs"))
        .expect("components.rs is a known unit")
        .clone();
    let components_text_before = workspace.generated[&components_generated_uri].text.clone();

    let new_main_text =
        workspace.rsx[&main_rsx_uri].line_index.text().to_string() + "\n// a trailing comment\n";
    workspace
        .regenerate(&main_rsx_uri, &new_main_text, 2)
        .expect("regenerating a syntactically valid edit succeeds");

    assert_eq!(
        workspace.generated[&components_generated_uri].text, components_text_before,
        "editing main.rsx must not change components.rs's generated text"
    );
}

/// M6 (issue #9 Gate 3 review, HIGH-4): a stale rust-analyzer
/// diagnostic for the unit's *previous* generated text must not
/// survive a regeneration and be re-merged into the next
/// `publishDiagnostics`, or a fixed type error keeps reappearing at
/// its old (now wrong) position.
#[test]
fn regenerate_clears_previously_cached_rust_analyzer_diagnostics() {
    let outcome = Workspace::load(&phase0_app_dir()).expect("plans and generates");
    let LoadOutcome::Planned(mut workspace) = outcome else {
        panic!("examples/phase0-app has a `.rsx` crate root");
    };
    let main_rsx_uri = workspace
        .rsx
        .keys()
        .find(|uri| uri.ends_with("main.rsx"))
        .expect("main.rsx is a known unit")
        .clone();
    let generated_uri = workspace.rsx_to_generated[&main_rsx_uri].clone();
    workspace
        .generated
        .get_mut(&generated_uri)
        .unwrap()
        .last_ra_diagnostics = vec![lsp_types::Diagnostic {
        message: "mismatched types".to_string(),
        ..Default::default()
    }];

    let text = workspace.rsx[&main_rsx_uri].line_index.text().to_string();
    workspace
        .regenerate(&main_rsx_uri, &text, 2)
        .expect("regenerating succeeds");

    assert!(
        workspace.generated[&generated_uri]
            .last_ra_diagnostics
            .is_empty(),
        "regenerate must clear the previous rust-analyzer diagnostics"
    );
}

/// M2 (issue #9 Gate 3 review, HIGH-2), at the `Workspace` API level
/// rather than through the full LSP protocol (`tests/gate3.rs` covers
/// the end-to-end shape): a `mod` declaration that exists only in the
/// edited buffer — never saved to disk — must still enter the
/// re-planned module graph, with a matching `#[path]` in the
/// generated root.
#[test]
fn replan_sees_a_module_declared_only_in_the_open_buffer() {
    let tmp = std::env::temp_dir().join(format!(
        "outou-lsp-test-replan-overlay-{}-{}",
        std::process::id(),
        line!()
    ));
    let src = tmp.join("src");
    fs::create_dir_all(&src).expect("creating src dir");
    fs::write(src.join("main.rsx"), "fn main() {}\n").expect("writing crate root");
    fs::write(src.join("newmod.rsx"), "pub fn f() {}\n").expect("writing the new module file");

    let outcome = Workspace::load(&tmp).expect("plans and generates");
    let LoadOutcome::Planned(mut workspace) = outcome else {
        panic!("the temp crate has a `.rsx` crate root");
    };
    let main_rsx_uri = workspace
        .rsx
        .keys()
        .find(|uri| uri.ends_with("main.rsx"))
        .expect("main.rsx is a known unit")
        .clone();
    // Mark the document "open" (as `didOpen` would), matching
    // `build_overlay`'s own condition for including a buffer.
    workspace.rsx.get_mut(&main_rsx_uri).unwrap().version = 2;

    // The edit is never written to disk: only the in-memory buffer
    // declares `mod newmod;`.
    let edited_text = "mod newmod;\nfn main() {}\n";
    workspace
        .replan(&main_rsx_uri, edited_text)
        .expect("replanning succeeds");

    assert!(
        workspace
            .generated
            .keys()
            .any(|uri| uri.ends_with("newmod.rs")),
        "newmod.rs must now be a planned/generated unit: {:?}",
        workspace.generated.keys().collect::<Vec<_>>()
    );
    let root_generated_uri = workspace.rsx_to_generated[&main_rsx_uri].clone();
    assert_eq!(
        workspace.generated[&root_generated_uri]
            .planned
            .module_paths
            .len(),
        1,
        "the root unit must have exactly one `#[path]` rewrite, for `newmod`"
    );

    fs::remove_dir_all(&tmp).ok();
}

/// Perf smoke test for issue #9's budget ("incremental `.rsx` ->
/// generated Rust: perceived as instantaneous"): regenerating one
/// small-to-medium real unit is a single `outou_syntax::parse` +
/// `DioxusBackend::generate` call (`outou_cli::build::emit::generate_unit`),
/// the same primitive `outou build` uses per file — no LSP-specific
/// overhead beyond that. 50ms is a generous bound (typical runs are
/// well under 1ms for a file this size); this exists to catch a
/// catastrophic regression, not to pin an exact number.
#[test]
fn regenerating_one_unit_is_fast() {
    let outcome = Workspace::load(&phase0_app_dir()).expect("plans and generates");
    let LoadOutcome::Planned(mut workspace) = outcome else {
        panic!("examples/phase0-app has a `.rsx` crate root");
    };
    let main_rsx_uri = workspace
        .rsx
        .keys()
        .find(|uri| uri.ends_with("main.rsx"))
        .expect("main.rsx is a known unit")
        .clone();
    let text = workspace.rsx[&main_rsx_uri].line_index.text().to_string();

    let start = Instant::now();
    workspace
        .regenerate(&main_rsx_uri, &text, 2)
        .expect("regenerating succeeds");
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 50,
        "regenerating one unit took {elapsed:?}, expected well under 50ms"
    );
}
