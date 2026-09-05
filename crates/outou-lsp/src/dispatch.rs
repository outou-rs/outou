//! Request/notification dispatch: everything that happens once
//! `crate::server`'s handshake is done. Requests this server intercepts
//! (`textDocument/{hover,completion,definition}`) are rewritten to the
//! generated file and position before being forwarded to rust-analyzer,
//! and the response mapped back before being relayed to the editor
//! (`crate::mapping`, `crate::response`); everything else is forwarded
//! transparently.

use std::collections::{HashMap, HashSet};

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response, ResponseError};
use lsp_types::{
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    PublishDiagnosticsParams,
};
use serde_json::{json, Value};

use crate::documents::{self, Workspace};
use crate::mapping;
use crate::ra::RaClient;
use crate::{diagnostics, response, uri};

/// One rust-analyzer request this server is waiting on a response for.
pub(crate) struct Pending {
    client_id: RequestId,
    kind: PendingKind,
}

enum PendingKind {
    Hover {
        generated_uri: lsp_types::Uri,
    },
    Definition,
    Completion {
        generated_uri: lsp_types::Uri,
    },
    /// Forwarded verbatim; the response is relayed with no mapping.
    Forward,
}

/// Everything the dispatch loop needs, for one `initialize`d root.
pub(crate) struct State {
    pub(crate) workspace: Option<Workspace>,
    /// `.rsx` documents kept only in degraded mode (no crate root), for
    /// which there is no [`Workspace`] to hold them.
    pub(crate) degraded_docs: HashMap<String, documents::RsxDocument>,
    pub(crate) ra: Option<RaClient>,
    pending: HashMap<RequestId, Pending>,
}

impl State {
    pub(crate) fn new() -> Self {
        Self {
            workspace: None,
            degraded_docs: HashMap::new(),
            ra: None,
            pending: HashMap::new(),
        }
    }
}

pub(crate) fn dispatch_client_request(state: &mut State, connection: &Connection, req: Request) {
    match req.method.as_str() {
        "textDocument/hover" => forward_position_request(state, connection, req, |uri| {
            PendingKind::Hover { generated_uri: uri }
        }),
        "textDocument/completion" => forward_position_request(state, connection, req, |uri| {
            PendingKind::Completion { generated_uri: uri }
        }),
        "textDocument/definition" => {
            forward_position_request(state, connection, req, |_| PendingKind::Definition)
        }
        _ => forward_transparent_request(state, connection, req),
    }
}

fn extract_position_params(params: &Value) -> Option<(lsp_types::Uri, lsp_types::Position)> {
    let uri_str = params.get("textDocument")?.get("uri")?.as_str()?;
    let uri: lsp_types::Uri = uri_str.parse().ok()?;
    let position: lsp_types::Position =
        serde_json::from_value(params.get("position")?.clone()).ok()?;
    Some((uri, position))
}

fn forward_position_request(
    state: &mut State,
    connection: &Connection,
    req: Request,
    make_kind: impl FnOnce(lsp_types::Uri) -> PendingKind,
) {
    let client_id = req.id.clone();
    let Some(workspace) = &state.workspace else {
        respond_null(connection, client_id);
        return;
    };
    let Some((rsx_uri, position)) = extract_position_params(&req.params) else {
        respond_null(connection, client_id);
        return;
    };
    let Some(mapped) = mapping::rsx_position_to_generated(workspace, &rsx_uri, position) else {
        respond_null(connection, client_id);
        return;
    };
    let Some(ra) = &state.ra else {
        respond_null(connection, client_id);
        return;
    };

    let mut params = req.params;
    params["textDocument"]["uri"] = serde_json::to_value(&mapped.generated_uri).unwrap();
    params["position"] = serde_json::to_value(mapped.position).unwrap();

    let ra_id = ra.next_id();
    state.pending.insert(
        ra_id.clone(),
        Pending {
            client_id,
            kind: make_kind(mapped.generated_uri),
        },
    );
    ra.send(Message::Request(Request {
        id: ra_id,
        method: req.method,
        params,
    }));
}

fn forward_transparent_request(state: &mut State, connection: &Connection, req: Request) {
    let Some(ra) = &state.ra else {
        respond_null(connection, req.id);
        return;
    };
    let ra_id = ra.next_id();
    state.pending.insert(
        ra_id.clone(),
        Pending {
            client_id: req.id.clone(),
            kind: PendingKind::Forward,
        },
    );
    ra.send(Message::Request(Request {
        id: ra_id,
        method: req.method,
        params: req.params,
    }));
}

fn respond_null(connection: &Connection, id: RequestId) {
    let _ = connection
        .sender
        .send(Message::Response(Response::new_ok(id, Value::Null)));
}

pub(crate) fn dispatch_ra_message(state: &mut State, connection: &Connection, message: Message) {
    match message {
        Message::Response(response) => handle_ra_response(state, connection, response),
        Message::Request(request) => {
            if let Some(ra) = &state.ra {
                ra.send(Message::Response(Response::new_ok(request.id, Value::Null)));
            }
        }
        Message::Notification(notification) => {
            handle_ra_notification(state, connection, notification)
        }
    }
}

fn handle_ra_response(state: &mut State, connection: &Connection, response: Response) {
    let Some(pending) = state.pending.remove(&response.id) else {
        return;
    };
    let result = map_pending_result(state, pending.kind, response.response_result);
    let _ = connection.sender.send(Message::Response(Response {
        id: pending.client_id,
        response_result: result,
    }));
}

fn map_pending_result(
    state: &State,
    kind: PendingKind,
    result: Result<Value, ResponseError>,
) -> Result<Value, ResponseError> {
    let mut value = result?;
    let Some(workspace) = &state.workspace else {
        return Ok(value);
    };
    match kind {
        PendingKind::Forward => {}
        PendingKind::Hover { generated_uri } => {
            response::map_hover_range(workspace, &generated_uri, &mut value)
        }
        PendingKind::Definition => response::map_definition_response(workspace, &mut value),
        PendingKind::Completion { generated_uri } => {
            response::map_completion_response(workspace, &generated_uri, &mut value)
        }
    }
    Ok(value)
}

fn handle_ra_notification(state: &mut State, connection: &Connection, notification: Notification) {
    if notification.method == "textDocument/publishDiagnostics" {
        if let Ok(params) = serde_json::from_value::<PublishDiagnosticsParams>(notification.params)
        {
            if let Some(workspace) = &mut state.workspace {
                diagnostics::handle_ra_publish(connection, workspace, params);
            }
        }
    }
}

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

/// Handles a `.rsx` file being saved: writes that unit's current
/// (Recovery-mode) generated text to disk and forwards `didSave` to
/// rust-analyzer for the *generated* file, so its own `checkOnSave`
/// flycheck — the only mechanism that surfaces semantic (type-mismatch)
/// errors, per the Week 1 spike's known blockers — runs against the
/// freshest translation rather than whatever was last on disk (possibly
/// nothing, if `outou build` was never run).
fn handle_rsx_save(state: &mut State, rsx_uri: &lsp_types::Uri) {
    let Some(workspace) = &state.workspace else {
        return;
    };
    let rsx_uri_string = uri::to_outou(rsx_uri).as_str().to_string();
    let Some(generated_uri_string) = workspace.rsx_to_generated.get(&rsx_uri_string) else {
        return;
    };
    let Some(unit) = workspace.generated.get(generated_uri_string) else {
        return;
    };

    if let Some(parent) = unit.planned.generated_file.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!(
                "outou-lsp: creating {}: {e}",
                unit.planned.generated_file.display()
            );
            return;
        }
    }
    if let Err(e) = std::fs::write(&unit.planned.generated_file, &unit.text) {
        eprintln!(
            "outou-lsp: writing {}: {e}",
            unit.planned.generated_file.display()
        );
        return;
    }

    if let Some(ra) = &state.ra {
        ra.notify(
            "textDocument/didSave",
            json!({
                "textDocument": { "uri": generated_uri_string },
                "text": unit.text,
            }),
        );
    }
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
        replan_and_resync(state, connection, rsx_uri_string, new_text);
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
) {
    let Some(workspace) = state.workspace.as_mut() else {
        return;
    };
    let previously_open: HashSet<String> = workspace.generated.keys().cloned().collect();

    if let Err(e) = workspace.replan(&rsx_uri_string, &new_text) {
        eprintln!("outou-lsp: re-planning after a module declaration change: {e}");
        return;
    }

    if let Some(ra) = &state.ra {
        let now_open: Vec<String> = workspace.generated.keys().cloned().collect();
        for generated_uri in &now_open {
            let Some(unit) = workspace.generated.get_mut(generated_uri) else {
                continue;
            };
            unit.ra_version = 1;
            if previously_open.contains(generated_uri) {
                ra.notify(
                    "textDocument/didChange",
                    json!({
                        "textDocument": { "uri": generated_uri, "version": 1 },
                        "contentChanges": [{ "text": unit.text }],
                    }),
                );
            } else {
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

    let rsx_uris: Vec<String> = workspace.rsx.keys().cloned().collect();
    for uri in rsx_uris {
        diagnostics::publish_for_rsx(connection, workspace, &uri);
    }
}
