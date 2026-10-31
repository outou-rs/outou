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

    let text =
        workspace.rsx[&main_rsx_uri].line_index.text().to_string() + "\nfn extra_item() {}\n";
    let regenerated = workspace
        .regenerate(&main_rsx_uri, &text, 2)
        .expect("regenerating succeeds")
        .expect("main.rsx is a known unit");

    assert!(regenerated.generated_changed);
    assert!(
        workspace.generated[&generated_uri]
            .last_ra_diagnostics
            .is_empty(),
        "regenerate must clear the previous rust-analyzer diagnostics"
    );
}

/// F17 (c), #31: regenerating from the text the unit already has (a
/// `didOpen` of the file as it is on disk) leaves the generated text, and
/// so the rust-analyzer diagnostics that describe it, as they were.
#[test]
fn regenerate_with_unchanged_text_keeps_rust_analyzer_diagnostics() {
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
    let regenerated = workspace
        .regenerate(&main_rsx_uri, &text, 2)
        .expect("regenerating succeeds")
        .expect("main.rsx is a known unit");

    assert!(!regenerated.generated_changed);
    assert_eq!(
        workspace.generated[&generated_uri]
            .last_ra_diagnostics
            .len(),
        1
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
/// generated Rust: perceived as instantaneous"). One `Workspace::regenerate`
/// call parses and generates the unit (`outou_cli::build::emit::generate_unit`,
/// the per-file step `outou build` also uses), and additionally re-parses
/// for `declared_modules`, builds a `LineIndex` and updates the registry.
/// 10ms is the fixed A1 threshold (`docs/alpha-results.md`, "A1:
/// measurement", user-confirmed 2026-09-27). For scale: the clean-machine
/// A1 pass measured the `generate_unit` step alone at 0.46ms worst case
/// (N=40 per unit, both fixed workspaces); the whole `regenerate` call was
/// not timed separately. The test takes the median of a few calls so a
/// single scheduler hiccup under concurrent compile load cannot fail it;
/// it exists to catch a catastrophic regression, not to pin a number.
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

    const CALLS: usize = 5;
    let mut timings = Vec::with_capacity(CALLS);
    for version in 2..2 + CALLS as i32 {
        let start = Instant::now();
        workspace
            .regenerate(&main_rsx_uri, &text, version)
            .expect("regenerating succeeds");
        timings.push(start.elapsed());
    }
    timings.sort();
    let median = timings[CALLS / 2];

    assert!(
        median < std::time::Duration::from_millis(10),
        "regenerating one unit took a median of {median:?} over {CALLS} calls ({timings:?}), expected well under 10ms"
    );
}

/// The editor opened the workspace through a symbolic link: its URIs name
/// the link, the workspace is keyed by real paths. Before URI
/// normalization (`crate::uri::normalize_file_uri`) the link-side URI
/// matched nothing and every forwarded request answered `null`.
#[cfg(unix)]
#[test]
fn a_symlinked_editor_uri_maps_once_normalized() {
    let root = std::env::temp_dir().join(format!(
        "outou-lsp-test-symlink-{}-{}",
        std::process::id(),
        line!()
    ));
    let _ = fs::remove_dir_all(&root);
    let real = root.join("real");
    fs::create_dir_all(real.join("src")).expect("creating src dir");
    fs::write(real.join("src/main.rsx"), "fn main() {}\n").expect("writing crate root");
    std::os::unix::fs::symlink(&real, root.join("link")).expect("creating the symlink");

    let LoadOutcome::Planned(workspace) = Workspace::load(&real).expect("plans and generates")
    else {
        panic!("the temp crate has a `.rsx` crate root");
    };

    let link_uri = outou_sourcemap::file_uri(&root.join("link/src/main.rsx"));
    let position = lsp_types::Position::new(0, 3);

    let raw: lsp_types::Uri = link_uri.as_str().parse().unwrap();
    assert!(
        crate::mapping::rsx_position_to_generated(&workspace, &raw, position).is_none(),
        "the lexical link-side URI is not a key of the workspace"
    );

    let normalized: lsp_types::Uri = crate::uri::normalize_file_uri(link_uri.as_str())
        .parse()
        .unwrap();
    assert!(crate::mapping::rsx_position_to_generated(&workspace, &normalized, position).is_some());
    let _ = fs::remove_dir_all(&root);
}

/// #31: a re-plan that fails part-way (here: the second unit's source
/// cannot be read after the first one was already generated) leaves the
/// previous plan, documents, generated units and mappings queryable.
#[test]
fn a_replan_that_fails_part_way_keeps_the_previous_state() {
    let tmp = std::env::temp_dir().join(format!(
        "outou-lsp-test-replan-atomic-{}-{}",
        std::process::id(),
        line!()
    ));
    let src = tmp.join("src");
    fs::create_dir_all(&src).expect("creating src dir");
    fs::write(src.join("main.rsx"), "mod a;\nmod b;\nfn main() {}\n").expect("writing root");
    fs::write(src.join("a.rsx"), "pub fn a() {}\n").expect("writing a");
    fs::write(src.join("b.rsx"), "pub fn b() {}\n").expect("writing b");

    let LoadOutcome::Planned(mut workspace) = Workspace::load(&tmp).expect("plans and generates")
    else {
        panic!("the temp crate has a `.rsx` crate root");
    };
    let mut rsx_before: Vec<(String, String, i32)> = workspace
        .rsx
        .iter()
        .map(|(uri, doc)| (uri.clone(), doc.line_index.text().to_string(), doc.version))
        .collect();
    rsx_before.sort();
    let mut generated_before: Vec<(String, String)> = workspace
        .generated
        .iter()
        .map(|(uri, unit)| (uri.clone(), unit.text.clone()))
        .collect();
    generated_before.sort();
    let map_before = workspace.rsx_to_generated.clone();
    let units_before = workspace.plan.as_ref().unwrap().units.len();
    assert_eq!(units_before, 3);

    let failing = |path: &Path| -> std::io::Result<String> {
        if path.ends_with("b.rsx") {
            Err(std::io::Error::other("injected read failure"))
        } else {
            fs::read_to_string(path)
        }
    };
    let result = workspace.replan_with_reader(&HashMap::new(), &failing);
    assert!(matches!(result, Err(LoadError::ReadSource { .. })));

    let mut rsx_after: Vec<(String, String, i32)> = workspace
        .rsx
        .iter()
        .map(|(uri, doc)| (uri.clone(), doc.line_index.text().to_string(), doc.version))
        .collect();
    rsx_after.sort();
    let mut generated_after: Vec<(String, String)> = workspace
        .generated
        .iter()
        .map(|(uri, unit)| (uri.clone(), unit.text.clone()))
        .collect();
    generated_after.sort();
    assert_eq!(rsx_after, rsx_before, "documents must be unchanged");
    assert_eq!(generated_after, generated_before, "units must be unchanged");
    assert_eq!(workspace.rsx_to_generated, map_before);
    assert_eq!(workspace.plan.as_ref().unwrap().units.len(), units_before);
    let a_generated = workspace
        .generated
        .keys()
        .find(|uri| uri.ends_with("a.rs"))
        .expect("a.rs is still a known unit")
        .clone();
    let a_uri = crate::uri::to_lsp(&outou_sourcemap::Uri::new(&a_generated));
    assert!(
        !matches!(
            crate::mapping::generated_location_to_source(
                &workspace,
                &a_uri,
                lsp_types::Range::default()
            ),
            crate::mapping::MappedLocation::Unchanged
        ),
        "the registry must still know the previous generated files"
    );

    fs::remove_dir_all(&tmp).ok();
}
