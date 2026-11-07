//! Dispatch-level tests for a standing plan error (#31): what the editor
//! and rust-analyzer are told while the module graph does not resolve, and
//! what clears it. rust-analyzer is the logging mock of `progress_tests`.

use std::fs;
use std::path::{Path, PathBuf};

use lsp_server::{Connection, Message, Notification};
use serde_json::{json, Value};

use super::notifications::{dispatch_client_notification, handle_rsx_change};
use super::progress_tests::{mock_ra, ra_received};
use super::State;
use crate::documents::{LoadOutcome, Workspace, INITIAL_RA_VERSION};
use crate::uri;

const WATCH_CHANGED: u32 = 2;
const WATCH_DELETED: u32 = 3;
const MANIFEST: &str = "[package]\nname = \"t\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";

pub(super) fn crate_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("outou-plan-error-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).expect("creating src dir");
    fs::write(dir.join("Cargo.toml"), MANIFEST).expect("writing the manifest");
    dir
}

/// A state over the crate at `dir`, as the server has it after startup: every
/// unit already opened on rust-analyzer at the initial version.
pub(super) fn load(dir: &Path) -> State {
    let LoadOutcome::Planned(mut workspace) = Workspace::load(dir).expect("plans") else {
        panic!("the temp crate has a `.rsx` crate root");
    };
    for unit in workspace.generated.values_mut() {
        unit.ra_version = INITIAL_RA_VERSION;
    }
    let mut state = State::new();
    state.workspace = Some(*workspace);
    state
}

pub(super) fn with_mock_ra(state: &mut State, tag: &str) -> PathBuf {
    let (ra, log) = mock_ra(tag);
    state.ra = Some(ra);
    log
}

pub(super) fn key(state: &State, suffix: &str) -> String {
    let workspace = state.workspace.as_ref().unwrap();
    workspace
        .rsx
        .keys()
        .find(|uri| uri.ends_with(suffix))
        .unwrap_or_else(|| panic!("{suffix} is a known document"))
        .clone()
}

pub(super) fn generated_text(state: &State, rsx_uri: &str) -> String {
    let workspace = state.workspace.as_ref().unwrap();
    workspace.generated[&workspace.rsx_to_generated[rsx_uri]]
        .text
        .clone()
}

fn ra_version(state: &State, rsx_uri: &str) -> i32 {
    let workspace = state.workspace.as_ref().unwrap();
    workspace.generated[&workspace.rsx_to_generated[rsx_uri]].ra_version
}

fn plan_error_file(state: &State) -> Option<String> {
    let uri = state.workspace.as_ref().unwrap().plan_error_uri()?;
    Some(uri.rsplit("/src/").next().unwrap().to_string())
}

/// The messages of the last publish for a URI ending `suffix`, if any.
fn last_published(client: &Connection, suffix: &str) -> Option<Vec<String>> {
    let mut last = None;
    while let Ok(msg) = client.receiver.try_recv() {
        if let Message::Notification(note) = msg {
            if note.method != "textDocument/publishDiagnostics" {
                continue;
            }
            let params: lsp_types::PublishDiagnosticsParams =
                serde_json::from_value(note.params).unwrap();
            if params.uri.as_str().ends_with(suffix) {
                last = Some(params.diagnostics.into_iter().map(|d| d.message).collect());
            }
        }
    }
    last
}

fn notify(state: &mut State, connection: &Connection, method: &str, params: Value) {
    dispatch_client_notification(
        state,
        connection,
        Notification::new(method.to_string(), params),
    );
}

fn watched(state: &mut State, connection: &Connection, events: &[(&Path, u32)]) {
    let changes: Vec<Value> = events
        .iter()
        .map(|(path, kind)| {
            let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            json!({ "uri": outou_sourcemap::file_uri(&path).as_str(), "type": kind })
        })
        .collect();
    notify(
        state,
        connection,
        "workspace/didChangeWatchedFiles",
        json!({ "changes": changes }),
    );
}

fn did_close(state: &mut State, connection: &Connection, rsx_uri: &str) {
    let uri = uri::to_lsp(&outou_sourcemap::Uri::new(rsx_uri));
    notify(
        state,
        connection,
        "textDocument/didClose",
        json!({ "textDocument": { "uri": uri.as_str() } }),
    );
}

/// H1: an edit that cannot be re-planned still regenerates the unit and
/// reaches rust-analyzer, now and for the ordinary edits that follow.
#[test]
fn edits_while_a_missing_module_stands_reach_the_generated_unit_and_rust_analyzer() {
    let dir = crate_dir("t1");
    fs::write(dir.join("src/main.rsx"), "fn main() {}\n").unwrap();
    let mut state = load(&dir);
    let log = with_mock_ra(&mut state, "t1");
    let main = key(&state, "main.rsx");
    let (connection, _client) = Connection::memory();

    handle_rsx_change(
        &mut state,
        &connection,
        main.clone(),
        "fn main() { let marker_first = 1; }\nmod missing;\n".to_string(),
        2,
    );
    assert!(generated_text(&state, &main).contains("marker_first"));
    assert_eq!(ra_version(&state, &main), INITIAL_RA_VERSION + 1);

    handle_rsx_change(
        &mut state,
        &connection,
        main.clone(),
        "fn main() { let marker_second = 1; }\nmod missing;\n".to_string(),
        3,
    );
    assert!(generated_text(&state, &main).contains("marker_second"));
    assert_eq!(ra_version(&state, &main), INITIAL_RA_VERSION + 2);
    let sent = ra_received(&log, "marker_second");
    assert!(
        sent.iter().any(|m| m["params"]["contentChanges"][0]["text"]
            .as_str()
            .is_some_and(|t| t.contains("marker_second"))),
        "rust-analyzer must receive the edit: {sent:?}"
    );

    // Removing the declaration re-plans and clears the error.
    handle_rsx_change(
        &mut state,
        &connection,
        main.clone(),
        "fn main() {}\n".to_string(),
        4,
    );
    assert!(state.workspace.as_ref().unwrap().plan_error.is_none());
    fs::remove_dir_all(&dir).ok();
}

/// T2: a module file deleted on disk; the declaring file's text is
/// unchanged, and its next body edit must not be frozen.
#[test]
fn a_body_edit_after_a_module_file_was_deleted_on_disk_reaches_the_generated_unit() {
    let dir = crate_dir("t2");
    fs::write(dir.join("src/main.rsx"), "mod a;\nfn main() {}\n").unwrap();
    fs::write(dir.join("src/a.rsx"), "pub fn a() {}\n").unwrap();
    let mut state = load(&dir);
    let main = key(&state, "main.rsx");
    let (connection, client) = Connection::memory();
    handle_rsx_change(
        &mut state,
        &connection,
        main.clone(),
        "mod a;\nfn main() {}\n".to_string(),
        1,
    );
    let a_file = dir.join("src/a.rsx").canonicalize().unwrap();
    fs::remove_file(&a_file).unwrap();
    watched(&mut state, &connection, &[(&a_file, WATCH_DELETED)]);
    assert_eq!(plan_error_file(&state).as_deref(), Some("main.rsx"));
    let _ = client.receiver.try_iter().count();
    let version = ra_version(&state, &main);

    handle_rsx_change(
        &mut state,
        &connection,
        main.clone(),
        "mod a;\nfn main() { let marker_t2 = 1; }\n".to_string(),
        2,
    );

    assert!(generated_text(&state, &main).contains("marker_t2"));
    assert!(ra_version(&state, &main) >= version);
    assert_eq!(
        state.workspace.as_ref().unwrap().rsx[&main].version,
        2,
        "the edit's own version"
    );
    let shown = last_published(&client, "main.rsx").expect("republished");
    assert!(shown.iter().any(|m| m.contains("`a`")), "{shown:?}");
    fs::remove_dir_all(&dir).ok();
}

/// T3: a crate-wide error on the root (two crate roots); every root edit
/// that follows must still reach the generated unit.
#[test]
fn root_edits_after_a_crate_wide_plan_error_reach_the_generated_unit() {
    let dir = crate_dir("t3");
    fs::write(dir.join("src/main.rsx"), "fn main() {}\n").unwrap();
    let mut state = load(&dir);
    let main = key(&state, "main.rsx");
    let (connection, _client) = Connection::memory();
    fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
    watched(
        &mut state,
        &connection,
        &[(&dir.join("Cargo.toml"), WATCH_CHANGED)],
    );
    assert_eq!(plan_error_file(&state).as_deref(), Some("main.rsx"));

    handle_rsx_change(
        &mut state,
        &connection,
        main.clone(),
        "fn main() { let marker_t3 = 1; }\n".to_string(),
        2,
    );

    assert!(generated_text(&state, &main).contains("marker_t3"));
    fs::remove_dir_all(&dir).ok();
}

/// D1: an error shown on another file than the one edited goes away when
/// the edit that caused it is undone, although the undone text equals the
/// plan in force.
#[test]
fn undoing_the_edit_that_broke_another_file_clears_the_error() {
    let dir = crate_dir("t9");
    fs::create_dir_all(dir.join("src/sub")).unwrap();
    fs::write(dir.join("src/main.rsx"), "mod a;\nfn main() {}\n").unwrap();
    fs::write(dir.join("src/a.rsx"), "pub fn a() {}\n").unwrap();
    fs::write(dir.join("src/sub/a.rsx"), "mod b;\npub fn a() {}\n").unwrap();
    let mut state = load(&dir);
    let main = key(&state, "main.rsx");
    let (connection, client) = Connection::memory();

    handle_rsx_change(
        &mut state,
        &connection,
        main.clone(),
        "#[path = \"sub/a.rsx\"]\nmod a;\nfn main() {}\n".to_string(),
        2,
    );
    assert_eq!(plan_error_file(&state).as_deref(), Some("sub/a.rsx"));
    let shown = last_published(&client, "sub/a.rsx").expect("the declaring file is published");
    assert!(shown.iter().any(|m| m.contains("`b`")), "{shown:?}");

    handle_rsx_change(
        &mut state,
        &connection,
        main,
        "mod a;\nfn main() {}\n".to_string(),
        3,
    );
    assert!(state.workspace.as_ref().unwrap().plan_error.is_none());
    assert_eq!(last_published(&client, "sub/a.rsx"), Some(Vec::new()));
    fs::remove_dir_all(&dir).ok();
}

/// M1 (T4): closing a buffer with an unsaved `mod missing;` re-plans from
/// disk, so the error goes and rust-analyzer gets the on-disk text back.
#[test]
fn closing_a_buffer_with_an_unsaved_bad_module_clears_the_error_and_reverts_the_unit() {
    let dir = crate_dir("t4");
    fs::write(dir.join("src/main.rsx"), "fn main() {}\n").unwrap();
    let mut state = load(&dir);
    let log = with_mock_ra(&mut state, "t4");
    let main = key(&state, "main.rsx");
    let (connection, client) = Connection::memory();
    handle_rsx_change(
        &mut state,
        &connection,
        main.clone(),
        "fn main() { let unsaved_t4 = 1; }\nmod missing;\n".to_string(),
        2,
    );
    assert!(plan_error_file(&state).is_some());
    let _ = client.receiver.try_iter().count();

    did_close(&mut state, &connection, &main);

    assert!(state.workspace.as_ref().unwrap().plan_error.is_none());
    assert!(!generated_text(&state, &main).contains("unsaved_t4"));
    assert_eq!(state.workspace.as_ref().unwrap().rsx[&main].version, 0);
    assert_eq!(last_published(&client, "main.rsx"), Some(Vec::new()));
    let sent = ra_received(&log, "\"fn main() {}");
    assert!(
        sent.iter().any(|m| m["params"]["contentChanges"][0]["text"]
            .as_str()
            .is_some_and(|t| !t.contains("unsaved_t4"))),
        "rust-analyzer must get the on-disk text back: {sent:?}"
    );
    fs::remove_dir_all(&dir).ok();
}

/// T4b: the same without any plan error: a discarded unsaved body edit must
/// not stay in rust-analyzer's copy.
#[test]
fn closing_a_buffer_with_an_unsaved_body_edit_reverts_the_unit() {
    let dir = crate_dir("t4b");
    fs::write(dir.join("src/main.rsx"), "fn main() {}\n").unwrap();
    let mut state = load(&dir);
    let log = with_mock_ra(&mut state, "t4b");
    let main = key(&state, "main.rsx");
    let (connection, _client) = Connection::memory();
    handle_rsx_change(
        &mut state,
        &connection,
        main.clone(),
        "fn main() { let unsaved_t4b = 1; }\n".to_string(),
        2,
    );
    assert!(generated_text(&state, &main).contains("unsaved_t4b"));

    did_close(&mut state, &connection, &main);

    assert!(!generated_text(&state, &main).contains("unsaved_t4b"));
    assert_eq!(ra_version(&state, &main), INITIAL_RA_VERSION + 2);
    let sent = ra_received(&log, "unsaved_t4b");
    assert!(
        sent.iter()
            .any(|m| m["params"]["textDocument"]["version"] == json!(INITIAL_RA_VERSION + 2)),
        "a second didChange must carry the on-disk text: {sent:?}"
    );
    fs::remove_dir_all(&dir).ok();
}

/// T4c: the error is on another file than the one whose buffer is closed.
#[test]
fn closing_the_buffer_that_caused_an_error_in_another_file_clears_it() {
    let dir = crate_dir("t4c");
    fs::create_dir_all(dir.join("src/sub")).unwrap();
    fs::write(dir.join("src/main.rsx"), "mod a;\nfn main() {}\n").unwrap();
    fs::write(dir.join("src/a.rsx"), "pub fn a() {}\n").unwrap();
    fs::write(dir.join("src/sub/a.rsx"), "mod b;\npub fn a() {}\n").unwrap();
    let mut state = load(&dir);
    let main = key(&state, "main.rsx");
    let (connection, client) = Connection::memory();
    handle_rsx_change(
        &mut state,
        &connection,
        main.clone(),
        "#[path = \"sub/a.rsx\"]\nmod a;\nfn main() {}\n".to_string(),
        2,
    );
    assert_eq!(plan_error_file(&state).as_deref(), Some("sub/a.rsx"));
    let _ = client.receiver.try_iter().count();

    did_close(&mut state, &connection, &main);

    assert!(state.workspace.as_ref().unwrap().plan_error.is_none());
    assert_eq!(last_published(&client, "sub/a.rsx"), Some(Vec::new()));
    fs::remove_dir_all(&dir).ok();
}

/// M2: the declaring file is a plain `.rs` file. The error stays on it, and
/// saving the fixed file re-plans (`**/*.rs` is not watched).
#[test]
fn saving_a_plain_rust_file_that_declared_the_bad_module_clears_the_error() {
    let dir = crate_dir("t5");
    fs::write(dir.join("src/main.rsx"), "mod util;\nfn main() {}\n").unwrap();
    fs::write(dir.join("src/util.rs"), "pub fn u() {}\n").unwrap();
    let mut state = load(&dir);
    let (connection, client) = Connection::memory();
    fs::write(dir.join("src/util.rs"), "mod nope;\npub fn u() {}\n").unwrap();
    watched(
        &mut state,
        &connection,
        &[(&dir.join("Cargo.toml"), WATCH_CHANGED)],
    );
    assert_eq!(plan_error_file(&state).as_deref(), Some("util.rs"));
    let shown = last_published(&client, "util.rs").expect("the .rs file carries the error");
    assert!(shown.iter().any(|m| m.contains("`nope`")), "{shown:?}");

    fs::write(dir.join("src/util.rs"), "pub fn u() {}\n").unwrap();
    let util = outou_sourcemap::file_uri(&dir.join("src/util.rs").canonicalize().unwrap());
    notify(
        &mut state,
        &connection,
        "textDocument/didSave",
        json!({ "textDocument": { "uri": util.as_str() } }),
    );

    assert!(state.workspace.as_ref().unwrap().plan_error.is_none());
    assert_eq!(last_published(&client, "util.rs"), Some(Vec::new()));
    fs::remove_dir_all(&dir).ok();
}

/// L3: the error moves between files as the user edits, and each move
/// shows it on the new file and clears it from the old one.
#[test]
fn the_error_moves_between_files_and_ends_cleared() {
    let dir = crate_dir("t7");
    fs::write(dir.join("src/main.rsx"), "mod a;\nfn main() {}\n").unwrap();
    fs::write(dir.join("src/a.rsx"), "pub fn a() {}\n").unwrap();
    let mut state = load(&dir);
    let main = key(&state, "main.rsx");
    let a = key(&state, "a.rsx");
    let (connection, client) = Connection::memory();

    handle_rsx_change(
        &mut state,
        &connection,
        a.clone(),
        "mod b;\npub fn a() {}\n".to_string(),
        2,
    );
    assert_eq!(plan_error_file(&state).as_deref(), Some("a.rsx"));
    assert!(last_published(&client, "a.rsx").is_some_and(|m| m.iter().any(|m| m.contains("`b`"))));

    handle_rsx_change(
        &mut state,
        &connection,
        main.clone(),
        "mod c;\nmod a;\nfn main() {}\n".to_string(),
        2,
    );
    assert_eq!(plan_error_file(&state).as_deref(), Some("main.rsx"));
    assert_eq!(last_published(&client, "a.rsx"), Some(Vec::new()));

    handle_rsx_change(
        &mut state,
        &connection,
        main,
        "mod a;\nfn main() {}\n".to_string(),
        3,
    );
    assert_eq!(plan_error_file(&state).as_deref(), Some("a.rsx"));
    assert_eq!(last_published(&client, "main.rsx"), Some(Vec::new()));

    handle_rsx_change(&mut state, &connection, a, "pub fn a() {}\n".to_string(), 3);
    assert!(plan_error_file(&state).is_none());
    assert_eq!(last_published(&client, "a.rsx"), Some(Vec::new()));
    fs::remove_dir_all(&dir).ok();
}
