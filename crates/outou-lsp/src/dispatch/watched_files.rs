//! `workspace/didChangeWatchedFiles`: edits made outside the editor
//! (`git checkout`, a formatter run in a terminal) to files the editor does
//! not have open (#31, F17 (d)).
//!
//! Once the `initialize` handshake is done (`initialized` is consumed by
//! `Connection::initialize_finish`, so it never reaches the dispatcher), a client that supports dynamic registration is
//! asked to watch `**/*.rsx` and `**/Cargo.toml` ([`register_watchers`]);
//! its events arrive in [`handle_did_change_watched_files`]. `.generated`
//! is deliberately not watched: those files are written by this server and
//! `outou build`, and the server's own view of them is the in-memory
//! generated text it sends to rust-analyzer.
//!
//! An open `.rsx` document is authoritative: the editor's buffer wins over
//! the disk, so its events are ignored. For a closed one, a changed file is
//! regenerated from disk like an edit; a created or deleted file, or a
//! changed crate `Cargo.toml`, changes what the module plan contains and
//! triggers the same full re-plan a `mod` edit does.

use std::path::Path;

use lsp_server::{Connection, Message, Request};
use lsp_types::{DidChangeWatchedFilesParams, FileChangeType, FileEvent};
use serde_json::{json, Value};

use super::notifications::{handle_rsx_change, last_sent_ra_versions, resync_after_replan};
use super::State;
use crate::{diagnostics, uri};

const RSX_GLOB: &str = "**/*.rsx";
const MANIFEST_GLOB: &str = "**/Cargo.toml";
const MANIFEST_FILE: &str = "Cargo.toml";
const REGISTRATION_ID: &str = "outou-watched-files";

/// Asks the editor to watch `.rsx` files and manifests, when it can and
/// there is a workspace to keep up to date.
pub(crate) fn register_watchers(state: &mut State, connection: &Connection) {
    if !state.client_supports_watched_files_registration || state.workspace.is_none() {
        return;
    }
    let id = state.next_forwarded_client_id();
    let _ = connection.sender.send(Message::Request(Request {
        id,
        method: "client/registerCapability".to_string(),
        params: json!({
            "registrations": [{
                "id": REGISTRATION_ID,
                "method": "workspace/didChangeWatchedFiles",
                "registerOptions": {
                    "watchers": [
                        { "globPattern": RSX_GLOB },
                        { "globPattern": MANIFEST_GLOB },
                    ],
                },
            }],
        }),
    }));
}

/// What a batch of watched-file events asks of the workspace.
#[derive(Default)]
struct Plan {
    /// A created or deleted `.rsx` file, or a changed manifest.
    replan: bool,
    /// Known, closed `.rsx` files deleted from disk.
    deleted: Vec<String>,
    /// Known, closed `.rsx` files whose disk text now differs, with it.
    changed: Vec<(String, String)>,
}

pub(super) fn handle_did_change_watched_files(
    state: &mut State,
    connection: &Connection,
    params: Value,
) {
    // Events for everything but `.rsx` still go to rust-analyzer, as every
    // notification this server does not handle itself does.
    forward_non_rsx_to_rust_analyzer(state, &params);
    let Ok(params) = serde_json::from_value::<DidChangeWatchedFilesParams>(params) else {
        return;
    };
    let plan = classify(state, &params.changes);

    if plan.replan {
        replan_from_disk(state, connection);
    } else {
        for (rsx_uri, text) in plan.changed {
            // Version 0: the file is not open, read from disk.
            handle_rsx_change(state, connection, rsx_uri, text, 0);
        }
    }
    for rsx_uri in plan.deleted {
        // A file deleted and created again in one batch exists now, and the
        // re-plan above has already reloaded it.
        let exists_again = uri::outou_uri_str_to_path(&rsx_uri).is_some_and(|path| path.exists());
        if !exists_again {
            forget_deleted(state, connection, &rsx_uri);
        }
    }
}

fn classify(state: &State, events: &[FileEvent]) -> Plan {
    let mut plan = Plan::default();
    let Some(workspace) = state.workspace.as_ref() else {
        return plan;
    };
    let manifest = uri::canonicalize_lenient(&workspace.manifest_dir.join(MANIFEST_FILE));
    for event in events {
        let rsx_uri = uri::normalize_file_uri(event.uri.as_str());
        if !rsx_uri.ends_with(".rsx") {
            plan.replan |= uri::outou_uri_str_to_path(&rsx_uri)
                .is_some_and(|path| uri::canonicalize_lenient(&path) == manifest);
            continue;
        }
        let known = workspace.rsx.get(&rsx_uri);
        if known.is_some_and(|doc| doc.version != 0) {
            continue;
        }
        if event.typ == FileChangeType::CREATED {
            // A file outside the crate cannot be reached by its `mod` tree.
            plan.replan |= is_inside_crate(&workspace.manifest_dir, &rsx_uri);
        } else if event.typ == FileChangeType::DELETED {
            mark_deleted(&mut plan, &rsx_uri, known.is_some());
        } else if let Some(doc) = known {
            read_changed(&mut plan, &rsx_uri, doc.line_index.text());
        }
    }
    plan
}

fn is_inside_crate(manifest_dir: &Path, rsx_uri: &str) -> bool {
    let crate_dir = uri::canonicalize_lenient(manifest_dir);
    uri::outou_uri_str_to_path(rsx_uri)
        .is_some_and(|path| uri::canonicalize_lenient(&path).starts_with(crate_dir))
}

fn mark_deleted(plan: &mut Plan, rsx_uri: &str, is_known: bool) {
    if is_known {
        plan.replan = true;
        plan.deleted.push(rsx_uri.to_string());
    }
}

fn read_changed(plan: &mut Plan, rsx_uri: &str, current_text: &str) {
    let Some(path) = uri::outou_uri_str_to_path(rsx_uri) else {
        return;
    };
    match std::fs::read_to_string(&path) {
        Ok(text) if text == current_text => {}
        Ok(text) => plan.changed.push((rsx_uri.to_string(), text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => mark_deleted(plan, rsx_uri, true),
        Err(e) => eprintln!("outou-lsp: reading {}: {e}", path.display()),
    }
}

/// A `.rsx` file the editor had open was deleted from disk and then closed:
/// forgets it, then re-plans, since its buffer was part of the overlay.
pub(super) fn handle_deleted_after_close(
    state: &mut State,
    connection: &Connection,
    rsx_uri: &str,
) {
    forget_deleted(state, connection, rsx_uri);
    replan_from_disk(state, connection);
}

/// The same crate-wide re-plan a `mod` edit triggers, minus the edit.
fn replan_from_disk(state: &mut State, connection: &Connection) {
    let Some(workspace) = state.workspace.as_mut() else {
        return;
    };
    let previous_ra_versions = last_sent_ra_versions(workspace);
    let result = workspace.replan_from_disk();
    // A failed re-plan leaves the previous state intact (see
    // `Workspace::replan`), so requests in flight stay valid; only a
    // successful one invalidates their positions.
    match result {
        Ok(()) => {
            state.epoch += 1;
            resync_after_replan(state, connection, &previous_ra_versions)
        }
        Err(e) => eprintln!("outou-lsp: re-planning after a change on disk: {e}"),
    }
}

/// Drops a deleted file the re-plan did not already drop (it keeps the old
/// plan when the plan no longer resolves, e.g. `mod x;` still declared),
/// and clears its diagnostics.
pub(super) fn forget_deleted(state: &mut State, connection: &Connection, rsx_uri: &str) {
    if let Some(workspace) = state.workspace.as_mut() {
        if let Some(generated_uri) = workspace.remove_deleted(rsx_uri) {
            if let Some(ra) = &state.ra {
                ra.notify(
                    "textDocument/didClose",
                    json!({ "textDocument": { "uri": generated_uri } }),
                );
            }
        }
    }
    diagnostics::clear_for_rsx(connection, rsx_uri);
}

fn forward_non_rsx_to_rust_analyzer(state: &State, params: &Value) {
    let Some(ra) = &state.ra else {
        return;
    };
    let Some(changes) = params.get("changes").and_then(Value::as_array) else {
        return;
    };
    let others: Vec<&Value> = changes
        .iter()
        .filter(|change| {
            !change
                .get("uri")
                .and_then(Value::as_str)
                .is_some_and(|uri| uri.ends_with(".rsx"))
        })
        .collect();
    if !others.is_empty() {
        ra.notify(
            "workspace/didChangeWatchedFiles",
            json!({ "changes": others }),
        );
    }
}
