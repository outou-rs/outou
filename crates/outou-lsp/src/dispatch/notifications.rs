//! `.rsx` document lifecycle notifications from the editor:
//! `didOpen`/`didChange`/`didSave`/`didClose`, plus `$/cancelRequest`
//! (dispatched from here since it arrives as a notification, but
//! resolved against the pending-request map owned by
//! `crate::dispatch::requests`). Everything else is forwarded to
//! rust-analyzer untouched.

use std::collections::{HashMap, HashSet};

use lsp_server::{Connection, Notification};
use lsp_types::{
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
};
use serde_json::json;

use super::requests::handle_cancel_request;
use super::State;
use crate::documents;
use crate::{diagnostics, plan, uri};

pub(crate) fn dispatch_client_notification(
    state: &mut State,
    connection: &Connection,
    note: Notification,
) {
    match note.method.as_str() {
        "textDocument/didOpen" => {
            if let Ok(params) = serde_json::from_value::<DidOpenTextDocumentParams>(note.params) {
                if is_outou_rsx(&params.text_document.uri, &params.text_document.language_id) {
                    let rsx_uri = uri::to_outou(&params.text_document.uri)
                        .as_str()
                        .to_string();
                    handle_rsx_change(
                        state,
                        connection,
                        rsx_uri,
                        params.text_document.text,
                        params.text_document.version,
                    );
                } else if let Some(ra) = &state.ra {
                    ra.notify(
                        "textDocument/didOpen",
                        serde_json::to_value(params).unwrap(),
                    );
                }
            }
        }
        "textDocument/didChange" => {
            if let Ok(params) = serde_json::from_value::<DidChangeTextDocumentParams>(note.params) {
                let is_rsx = params.text_document.uri.as_str().ends_with(".rsx");
                if is_rsx {
                    let version = params.text_document.version;
                    let rsx_uri = uri::to_outou(&params.text_document.uri)
                        .as_str()
                        .to_string();
                    if let Some(change) = params.content_changes.into_iter().next_back() {
                        handle_rsx_change(state, connection, rsx_uri, change.text, version);
                    }
                } else if let Some(ra) = &state.ra {
                    ra.notify(
                        "textDocument/didChange",
                        serde_json::to_value(params).unwrap(),
                    );
                }
            }
        }
        "textDocument/didClose" => {
            // The generated overlay stays open on rust-analyzer for the
            // life of the session; only a full re-plan ever closes one.
        }
        "textDocument/didSave" => {
            if let Ok(params) = serde_json::from_value::<DidSaveTextDocumentParams>(note.params) {
                if params.text_document.uri.as_str().ends_with(".rsx") {
                    handle_rsx_save(state, &params.text_document.uri);
                } else if let Some(ra) = &state.ra {
                    ra.notify(
                        "textDocument/didSave",
                        serde_json::to_value(params).unwrap(),
                    );
                }
            }
        }
        "$/cancelRequest" => handle_cancel_request(state, note.params),
        _ => {
            if let Some(ra) = &state.ra {
                ra.notify(note.method, note.params);
            }
        }
    }
}

fn is_outou_rsx(uri: &lsp_types::Uri, language_id: &str) -> bool {
    language_id == "outou-rsx" || uri.as_str().ends_with(".rsx")
}

/// Handles a `.rsx` file being saved: re-plans the crate from the
/// now-on-disk sources and writes every unit's generated Rust through the
/// exact same transactional Strict path `outou build` uses
/// (`outou_cli::build::emit::emit`), then forwards `didSave` to
/// rust-analyzer for the saved file's *generated* unit, so its own
/// `checkOnSave` flycheck — the only mechanism that surfaces semantic
/// (type-mismatch) errors, per the Week 1 spike's known blockers — runs
/// against a build that actually compiles.
///
/// Never writes Recovery-mode placeholder text (issue #9 Gate 3 review,
/// M1/CRITICAL-1, confirmed live: saving a half-typed `<UserCard us`
/// replaced `examples/phase0-app`'s real `[[bin]]` target,
/// `src/.generated/crate-root.rs`, with a placeholder call, and the
/// sibling `.rs.map.json` was never rewritten at all, leaving on-disk
/// code and map silently diverged). If Strict generation fails anywhere
/// in the plan, nothing is written, `didSave` is not forwarded to
/// rust-analyzer, and the in-memory workspace (already updated by the
/// preceding `didChange`, still in Recovery mode for the editor overlay)
/// is left exactly as it was — Outou's own syntax diagnostic, published
/// by the ordinary `didChange` path, already tells the user why nothing
/// was written.
fn handle_rsx_save(state: &mut State, rsx_uri: &lsp_types::Uri) {
    let Some(workspace) = &state.workspace else {
        return;
    };
    let rsx_uri_string = uri::to_outou(rsx_uri).as_str().to_string();

    let plan = match plan::resolve(&workspace.manifest_dir) {
        Ok(plan::Resolved::Planned { plan, .. }) => plan,
        Ok(plan::Resolved::Degraded { .. }) => {
            eprintln!(
                "outou-lsp: no `.rsx` crate root after saving {rsx_uri_string}; not writing generated Rust"
            );
            return;
        }
        Err(e) => {
            eprintln!("outou-lsp: re-planning on save failed: {e}; not writing generated Rust");
            return;
        }
    };

    if let Err(e) = outou_cli::build::emit::emit(&plan) {
        eprintln!(
            "outou-lsp: not writing generated Rust for {rsx_uri_string} (syntax errors prevent generation): {e}"
        );
        return;
    }

    let Some(saved_unit) = plan
        .units
        .iter()
        .find(|unit| outou_sourcemap::file_uri(&unit.source_file).as_str() == rsx_uri_string)
    else {
        return;
    };
    let generated_uri = outou_sourcemap::file_uri(&saved_unit.generated_file);
    let Some(ra) = &state.ra else {
        return;
    };
    let text = workspace
        .generated
        .get(generated_uri.as_str())
        .map(|unit| unit.text.clone());
    ra.notify(
        "textDocument/didSave",
        json!({
            "textDocument": { "uri": generated_uri.as_str() },
            "text": text,
        }),
    );
}

/// Regenerates one `.rsx` unit after an edit (or, in degraded mode,
/// reparses it stand-alone) and republishes its diagnostics. Shared by
/// `didOpen` and `didChange`: both are, from the compiler's point of
/// view, "here is this file's current text".
fn handle_rsx_change(
    state: &mut State,
    connection: &Connection,
    rsx_uri_string: String,
    new_text: String,
    version: i32,
) {
    let Some(workspace) = state.workspace.as_mut() else {
        state.degraded_docs.insert(
            rsx_uri_string.clone(),
            documents::RsxDocument::new(new_text, version),
        );
        diagnostics::publish_degraded(connection, &state.degraded_docs, &rsx_uri_string);
        return;
    };

    if workspace.module_shape_changed(&rsx_uri_string, &new_text) {
        replan_and_resync(state, connection, rsx_uri_string, new_text, version);
        return;
    }

    match workspace.regenerate(&rsx_uri_string, &new_text, version) {
        Ok(Some(generated_uri_string)) => {
            if let Some(ra) = &state.ra {
                if let Some(unit) = workspace.generated.get_mut(&generated_uri_string) {
                    unit.ra_version += 1;
                    ra.notify(
                        "textDocument/didChange",
                        json!({
                            "textDocument": { "uri": generated_uri_string, "version": unit.ra_version },
                            "contentChanges": [{ "text": unit.text }],
                        }),
                    );
                }
            }
        }
        Ok(None) => {
            workspace.rsx.insert(
                rsx_uri_string.clone(),
                documents::RsxDocument::new(new_text, version),
            );
        }
        Err(e) => eprintln!("outou-lsp: regenerating {rsx_uri_string}: {e}"),
    }
    diagnostics::publish_for_rsx(connection, workspace, &rsx_uri_string);
}

/// Handles a module-declaration change (issue #9 architecture note: "if
/// changed, re-plan and regenerate all units"): re-resolves the crate,
/// resyncs rust-analyzer's overlays for every unit (new ones get
/// `didOpen`, previously known ones get a full-text `didChange`), and
/// republishes diagnostics for every known `.rsx` file.
fn replan_and_resync(
    state: &mut State,
    connection: &Connection,
    rsx_uri_string: String,
    new_text: String,
    version: i32,
) {
    let Some(workspace) = state.workspace.as_mut() else {
        return;
    };
    // Captured before `replan` rebuilds `workspace.generated` from
    // scratch (every unit comes back with `ra_version: 0`, regardless of
    // what rust-analyzer was last told): S3 (issue #9 Gate 3 review)
    // needs each previously-open unit's *last-sent* version to keep
    // counting up rather than resetting to 1, which rust-analyzer is
    // free to treat as a stale/duplicate update after N edits.
    let previous_ra_versions: HashMap<String, i32> = workspace
        .generated
        .iter()
        .map(|(uri, unit)| (uri.clone(), unit.ra_version))
        .collect();

    if let Err(e) = workspace.replan(&rsx_uri_string, &new_text) {
        eprintln!("outou-lsp: re-planning after a module declaration change: {e}");
        // The crate-wide re-plan failed (issue #9 Gate 3 review, M2), but
        // the editor's own buffer still changed: keep serving *this*
        // document from the new text rather than leaving stale syntax
        // diagnostics up for content the user no longer has on screen.
        // The rest of `workspace` (other units, the registry) is left
        // exactly as it was — `replan` never partially mutates it before
        // this point.
        workspace.rsx.insert(
            rsx_uri_string.clone(),
            documents::RsxDocument::new(new_text, version),
        );
        diagnostics::publish_for_rsx(connection, workspace, &rsx_uri_string);
        return;
    }
    // A successful re-plan invalidates every generated-file position any
    // in-flight rust-analyzer request was computed against (S3): bump the
    // epoch so `handle_ra_response` answers those `RequestCancelled`
    // instead of mapping a stale response against the new state.
    state.epoch += 1;
    let Some(workspace) = state.workspace.as_mut() else {
        return;
    };

    if let Some(ra) = &state.ra {
        let now_open: HashSet<String> = workspace.generated.keys().cloned().collect();
        for generated_uri in previous_ra_versions.keys() {
            if !now_open.contains(generated_uri) {
                // This unit no longer exists after the re-plan (a module
                // was removed, or renamed to a different generated path);
                // tell rust-analyzer its overlay is gone rather than
                // leaving a phantom open document (S3).
                ra.notify(
                    "textDocument/didClose",
                    json!({ "textDocument": { "uri": generated_uri } }),
                );
            }
        }
        for generated_uri in &now_open {
            let Some(unit) = workspace.generated.get_mut(generated_uri) else {
                continue;
            };
            match previous_ra_versions.get(generated_uri) {
                Some(&previous_version) => {
                    unit.ra_version = previous_version + 1;
                    ra.notify(
                        "textDocument/didChange",
                        json!({
                            "textDocument": { "uri": generated_uri, "version": unit.ra_version },
                            "contentChanges": [{ "text": unit.text }],
                        }),
                    );
                }
                None => {
                    unit.ra_version = 1;
                    ra.notify(
                        "textDocument/didOpen",
                        json!({
                            "textDocument": {
                                "uri": generated_uri,
                                "languageId": "rust",
                                "version": 1,
                                "text": unit.text,
                            }
                        }),
                    );
                }
            }
        }
    }

    let rsx_uris: Vec<String> = workspace.rsx.keys().cloned().collect();
    for uri in rsx_uris {
        diagnostics::publish_for_rsx(connection, workspace, &uri);
    }
}
