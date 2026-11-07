//! Completion reads the component names and parameters that every
//! `RsxDocument` derived when it was built (#31 L15), never re-parsing a
//! file per keystroke. These tests pin that this cached view follows every
//! path that replaces a document: an edit in another open buffer, a
//! watched-file event, a re-plan and a close that reverts to disk.

use std::fs;
use std::path::Path;

use lsp_server::Connection;
use serde_json::{json, Value};

use super::notifications::{dispatch_client_notification, forward_regenerated, handle_rsx_change};
use super::plan_error_tests::{crate_dir, key, load};
use super::State;
use crate::complete::local_completion;
use crate::documents::PARSE_COUNT;
use crate::uri;

const WATCH_CHANGED: u32 = 2;

const MAIN_TAG: &str = "mod widgets;\n#[component]\nfn App() -> Element {\n    <div><Wi</div>\n}\n";
const MAIN_ATTR: &str =
    "mod widgets;\n#[component]\nfn App() -> Element {\n    <div><Widget  /></div>\n}\n";
const WIDGETS: &str = "#[component]\npub fn Widget(label: String) -> Element {\n    <p />\n}\n";

fn parses() -> usize {
    PARSE_COUNT.with(|count| count.get())
}

/// Labels offered at the cursor `needle_end` bytes after `needle` in `main`.
fn labels_after(state: &State, main: &str, needle: &str) -> Vec<String> {
    let mut labels = raw_labels_after(state, main, needle);
    labels.sort();
    labels
}

/// [`labels_after`] in the order the server answered, unsorted.
fn raw_labels_after(state: &State, main: &str, needle: &str) -> Vec<String> {
    let workspace = state.workspace.as_ref().unwrap();
    let doc = &workspace.rsx[main];
    let offset = (doc.line_index.text().find(needle).unwrap() + needle.len()) as u32;
    let range = doc
        .line_index
        .span_to_range(outou_sourcemap::Span::new(offset, offset));
    let position = lsp_types::Position::new(range.start.line, range.start.character);
    let items = local_completion(workspace, main, position).expect("answered locally");
    items
        .iter()
        .map(|item| item["label"].as_str().unwrap().to_string())
        .collect()
}

/// The component names offered for `<Wi`, with `main` showing a tag name.
fn tag_labels(state: &mut State, connection: &Connection, main: &str) -> Vec<String> {
    handle_rsx_change(
        state,
        connection,
        main.to_string(),
        MAIN_TAG.to_string(),
        90,
    );
    labels_after(state, main, "<Wi")
        .into_iter()
        .filter(|label| label.starts_with("Wi"))
        .collect()
}

/// The prop names offered inside `<Widget  />`, with `main` showing it.
fn attr_labels(state: &mut State, connection: &Connection, main: &str) -> Vec<String> {
    handle_rsx_change(
        state,
        connection,
        main.to_string(),
        MAIN_ATTR.to_string(),
        91,
    );
    labels_after(state, main, "<Widget ")
}

fn notify(state: &mut State, connection: &Connection, method: &str, params: Value) {
    dispatch_client_notification(
        state,
        connection,
        lsp_server::Notification::new(method.to_string(), params),
    );
}

fn watch(state: &mut State, connection: &Connection, path: &Path, kind: u32) {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    notify(
        state,
        connection,
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [{ "uri": outou_sourcemap::file_uri(&path).as_str(), "type": kind }] }),
    );
}

fn app_state(tag: &str) -> (State, std::path::PathBuf) {
    let dir = crate_dir(tag);
    fs::write(dir.join("src/main.rsx"), MAIN_TAG).unwrap();
    fs::write(dir.join("src/widgets.rsx"), WIDGETS).unwrap();
    (load(&dir), dir)
}

fn wiggle() -> String {
    format!("{WIDGETS}#[component]\npub fn Wiggle(size: u32) -> Element {{\n    <p />\n}}\n")
}

#[test]
fn completion_reflects_an_unsaved_edit_in_another_open_file() {
    let (mut state, dir) = app_state("cc-unsaved");
    let (main, widgets) = (key(&state, "main.rsx"), key(&state, "widgets.rsx"));
    let (connection, _client) = Connection::memory();
    assert_eq!(tag_labels(&mut state, &connection, &main), ["Widget"]);
    assert_eq!(attr_labels(&mut state, &connection, &main), ["label"]);

    handle_rsx_change(&mut state, &connection, widgets.clone(), wiggle(), 2);
    assert_eq!(
        tag_labels(&mut state, &connection, &main),
        ["Widget", "Wiggle"]
    );

    let changed =
        "#[component]\npub fn Widget(size: u32, r#type: String) -> Element {\n    <p />\n}\n";
    handle_rsx_change(&mut state, &connection, widgets, changed.to_string(), 3);
    assert_eq!(
        attr_labels(&mut state, &connection, &main),
        ["size", "type"]
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn completion_reflects_a_disk_change_picked_up_by_the_watcher() {
    let (mut state, dir) = app_state("cc-watched");
    let main = key(&state, "main.rsx");
    let (connection, _client) = Connection::memory();
    assert_eq!(tag_labels(&mut state, &connection, &main), ["Widget"]);

    let widgets_path = dir.join("src/widgets.rsx");
    fs::write(&widgets_path, wiggle()).unwrap();
    watch(&mut state, &connection, &widgets_path, WATCH_CHANGED);
    assert_eq!(
        tag_labels(&mut state, &connection, &main),
        ["Widget", "Wiggle"]
    );
    assert_eq!(attr_labels(&mut state, &connection, &main), ["label"]);

    fs::write(&widgets_path, "pub fn helper() {}\n").unwrap();
    watch(&mut state, &connection, &widgets_path, WATCH_CHANGED);
    assert!(tag_labels(&mut state, &connection, &main).is_empty());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn completion_reflects_a_replan_that_adds_and_removes_a_file() {
    let (mut state, dir) = app_state("cc-replan");
    let main = key(&state, "main.rsx");
    let (connection, _client) = Connection::memory();

    // A new module file, reached by a `mod` line typed into the open buffer.
    let extra_path = dir.join("src/extra.rsx");
    fs::write(
        &extra_path,
        "#[component]\npub fn Wick(lit: bool) -> Element {\n    <p />\n}\n",
    )
    .unwrap();
    // The triggering buffer also declares a component, which the re-plan
    // must take from the typed text rather than from the previous document.
    let with_mod = MAIN_TAG.replacen("mod widgets;", "mod widgets;\nmod extra;", 1)
        + "#[component]\npub fn Wizard() -> Element {\n    <p />\n}\n";
    handle_rsx_change(&mut state, &connection, main.clone(), with_mod, 5);
    assert!(state.workspace.as_ref().unwrap().plan_error.is_none());
    assert_eq!(
        labels_after(&state, &main, "<Wi"),
        ["Wick", "Widget", "Wizard"]
    );
    assert!(state
        .workspace
        .as_ref()
        .unwrap()
        .rsx
        .keys()
        .any(|u| u.ends_with("extra.rsx")));

    // Dropping the `mod` line re-plans the file away.
    handle_rsx_change(
        &mut state,
        &connection,
        main.clone(),
        MAIN_TAG.to_string(),
        6,
    );
    assert!(!state
        .workspace
        .as_ref()
        .unwrap()
        .rsx
        .keys()
        .any(|u| u.ends_with("extra.rsx")));
    assert_eq!(labels_after(&state, &main, "<Wi"), ["Widget"]);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn completion_forgets_an_unsaved_edit_once_the_file_is_closed() {
    let (mut state, dir) = app_state("cc-close");
    let (main, widgets) = (key(&state, "main.rsx"), key(&state, "widgets.rsx"));
    let (connection, _client) = Connection::memory();

    handle_rsx_change(&mut state, &connection, widgets.clone(), wiggle(), 2);
    assert_eq!(
        tag_labels(&mut state, &connection, &main),
        ["Widget", "Wiggle"]
    );

    let lsp_uri = uri::to_lsp(&outou_sourcemap::Uri::new(&widgets));
    notify(
        &mut state,
        &connection,
        "textDocument/didClose",
        json!({ "textDocument": { "uri": lsp_uri.as_str() } }),
    );
    assert_eq!(tag_labels(&mut state, &connection, &main), ["Widget"]);
    fs::remove_dir_all(&dir).ok();
}

/// Counts the parses made by this crate itself (every one goes through
/// `parse_rsx`, enforced by `clippy.toml`); the codegen's own parse inside
/// `generate_unit` lives in another crate and is not counted. Completion
/// parses only the document being completed in, however many documents
/// there are. An edit parses only the edited text, three times: the
/// module-shape check, `regenerate`, and the new `RsxDocument`.
#[test]
fn completion_makes_one_lsp_side_parse_however_many_documents_and_an_edit_three() {
    let (mut state, dir) = app_state("cc-count");
    let (main, widgets) = (key(&state, "main.rsx"), key(&state, "widgets.rsx"));
    let (connection, _client) = Connection::memory();
    handle_rsx_change(
        &mut state,
        &connection,
        main.clone(),
        MAIN_TAG.to_string(),
        2,
    );

    let before = parses();
    for _ in 0..25 {
        assert_eq!(labels_after(&state, &main, "<Wi"), ["Widget"]);
    }
    assert_eq!(
        parses() - before,
        25,
        "one parse per request, of the current document"
    );

    // Many more documents must not add a parse per request.
    let workspace = state.workspace.as_mut().unwrap();
    for index in 0..40 {
        let text = format!("#[component]\npub fn Extra{index}() -> Element {{ <p /> }}\n");
        workspace.rsx.insert(
            format!("file:///elsewhere/e{index}.rsx"),
            crate::documents::RsxDocument::new(text, 0),
        );
    }
    let before = parses();
    assert_eq!(labels_after(&state, &main, "<Wi"), ["Widget"]);
    assert_eq!(parses() - before, 1);

    let before = parses();
    handle_rsx_change(&mut state, &connection, widgets, wiggle(), 3);
    assert_eq!(
        parses() - before,
        3,
        "edited text only, three LSP-side parses"
    );
    fs::remove_dir_all(&dir).ok();
}

/// Component labels come out in a fixed order, and the first component of a
/// duplicated name (by URI order) supplies the props, whatever order the
/// `HashMap` iterates in.
#[test]
fn cross_file_completion_order_and_first_component_wins_are_deterministic() {
    let (mut state, dir) = app_state("cc-order");
    let main = key(&state, "main.rsx");
    let (connection, _client) = Connection::memory();
    handle_rsx_change(
        &mut state,
        &connection,
        main.clone(),
        MAIN_ATTR.to_string(),
        2,
    );
    let workspace = state.workspace.as_mut().unwrap();
    workspace.rsx.remove(&key_of(workspace, "widgets.rsx"));
    for index in (0..10).rev() {
        let text = format!(
            "#[component]\npub fn Widget(p{index}: u32) -> Element {{ <p /> }}\n\
             #[component]\npub fn Wi{index}() -> Element {{ <p /> }}\n"
        );
        workspace.rsx.insert(
            format!("file:///elsewhere/d{index}.rsx"),
            crate::documents::RsxDocument::new(text, 0),
        );
    }
    assert_eq!(labels_after(&state, &main, "<Widget "), ["p0"]);

    handle_rsx_change(
        &mut state,
        &connection,
        main.clone(),
        MAIN_TAG.to_string(),
        3,
    );
    let components: Vec<String> = raw_labels_after(&state, &main, "<Wi")
        .into_iter()
        .filter(|label| label.starts_with("Wi"))
        .collect();
    let mut sorted = components.clone();
    sorted.sort();
    assert_eq!(components, sorted, "component labels are sorted");
    assert_eq!(components.len(), 11);
    fs::remove_dir_all(&dir).ok();
}

fn key_of(workspace: &crate::documents::Workspace, suffix: &str) -> String {
    workspace
        .rsx
        .keys()
        .find(|uri| uri.ends_with(suffix))
        .unwrap()
        .clone()
}

/// The crate-wide re-plan fails (`mod missing;`), the unit is regenerated
/// against the plan in force; the new text is still what completion sees.
#[test]
fn completion_reflects_an_edit_whose_replan_failed() {
    let (mut state, dir) = app_state("cc-failed-replan");
    let main = key(&state, "main.rsx");
    let (connection, _client) = Connection::memory();
    let broken = format!(
        "mod missing;\n{MAIN_TAG}#[component]\npub fn Wisp(ghost: bool) -> Element {{ <p /> }}\n"
    );
    handle_rsx_change(&mut state, &connection, main.clone(), broken, 2);
    assert!(state.workspace.as_ref().unwrap().plan_error.is_some());
    assert_eq!(labels_after(&state, &main, "<Wi"), ["Widget", "Wisp"]);
    fs::remove_dir_all(&dir).ok();
}

/// Generation failing for a known unit still replaces the document.
#[test]
fn completion_reflects_an_edit_whose_regeneration_errored() {
    let (mut state, dir) = app_state("cc-regen-err");
    let (main, widgets) = (key(&state, "main.rsx"), key(&state, "widgets.rsx"));
    let error = crate::documents::LoadError::Emit(outou_cli::build::EmitError::Unsupported {
        path: dir.join("src/widgets.rsx"),
        backend: "test",
        message: "refused".to_string(),
    });
    forward_regenerated(&mut state, &widgets, wiggle(), 2, Err(error));
    assert_eq!(labels_after(&state, &main, "<Wi"), ["Widget", "Wiggle"]);
    fs::remove_dir_all(&dir).ok();
}

/// A document the plan does not reach is replaced too.
#[test]
fn completion_reflects_an_edit_to_a_file_outside_the_plan() {
    let (mut state, dir) = app_state("cc-unplanned");
    let main = key(&state, "main.rsx");
    let (connection, _client) = Connection::memory();
    let outside = "file:///elsewhere/stray.rsx".to_string();
    let text = "#[component]\npub fn Wild() -> Element { <p /> }\n";
    handle_rsx_change(
        &mut state,
        &connection,
        outside.clone(),
        text.to_string(),
        1,
    );
    assert_eq!(labels_after(&state, &main, "<Wi"), ["Widget", "Wild"]);
    handle_rsx_change(
        &mut state,
        &connection,
        outside,
        "fn nothing() {}\n".to_string(),
        2,
    );
    assert_eq!(labels_after(&state, &main, "<Wi"), ["Widget"]);
    fs::remove_dir_all(&dir).ok();
}
