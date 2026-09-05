//! Requests the editor sends this server: `textDocument/{hover,
//! completion,definition}` are rewritten to the generated file and
//! position before being forwarded to rust-analyzer (mapped back by
//! `crate::dispatch::responses`); everything else is forwarded
//! transparently. Also `$/cancelRequest` (which needs the same
//! editor-id -> rust-analyzer-id lookup as the pending map below) and
//! [`fail_all_pending`], used when rust-analyzer exits.

use lsp_server::{Connection, Message, Request, RequestId, Response, ResponseError};
use serde_json::{json, Value};

use super::{Pending, PendingKind, State};
use crate::complete;
use crate::mapping;
use crate::uri;

pub(crate) fn dispatch_client_request(state: &mut State, connection: &Connection, req: Request) {
    match req.method.as_str() {
        "textDocument/hover" => forward_position_request(state, connection, req, |uri, _pos| {
            PendingKind::Hover { generated_uri: uri }
        }),
        "textDocument/completion" => dispatch_completion_request(state, connection, req),
        "textDocument/definition" => {
            forward_position_request(state, connection, req, |_, _| PendingKind::Definition)
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

/// `textDocument/completion` needs one extra step before the generic
/// [`forward_position_request`] path: a tag-name or attribute-name
/// position is answered locally (`crate::complete`), never forwarded to
/// rust-analyzer at all, per Gate 3's review (issue #9 fix list M3) —
/// rust-analyzer only ever sees the *expanded* Rust, where that
/// distinction has already been lost.
fn dispatch_completion_request(state: &mut State, connection: &Connection, req: Request) {
    if let Some(workspace) = &state.workspace {
        if let Some((rsx_uri, position)) = extract_position_params(&req.params) {
            let rsx_uri_string = uri::to_outou(&rsx_uri).as_str().to_string();
            if let Some(items) = complete::local_completion(workspace, &rsx_uri_string, position) {
                let _ = connection.sender.send(Message::Response(Response::new_ok(
                    req.id,
                    json!({ "isIncomplete": false, "items": items }),
                )));
                return;
            }
        }
    }
    forward_position_request(state, connection, req, |uri, cursor| {
        PendingKind::Completion {
            generated_uri: uri,
            cursor,
        }
    });
}

fn forward_position_request(
    state: &mut State,
    connection: &Connection,
    req: Request,
    make_kind: impl FnOnce(lsp_types::Uri, lsp_types::Position) -> PendingKind,
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
            kind: make_kind(mapped.generated_uri, mapped.position),
            epoch: state.epoch,
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
            epoch: state.epoch,
        },
    );
    ra.send(Message::Request(Request {
        id: ra_id,
        method: req.method,
        params: req.params,
    }));
}

/// Fails every request this server is still waiting on a rust-analyzer
/// response for, with an ordinary `InternalError` rather than leaving the
/// editor waiting forever (issue #9 Gate 3 review, S5/MEDIUM-11):
/// `crate::server`'s main loop calls this the moment rust-analyzer's
/// message channel closes (the child exited or crashed), since none of
/// those in-flight requests will ever get an answer otherwise.
pub(crate) fn fail_all_pending(state: &mut State, connection: &Connection) {
    for (_, pending) in state.pending.drain() {
        let _ = connection.sender.send(Message::Response(Response {
            id: pending.client_id,
            response_result: Err(ResponseError {
                code: lsp_server::ErrorCode::InternalError as i32,
                message: "outou-lsp: rust-analyzer exited before answering this request"
                    .to_string(),
                data: None,
            }),
        }));
    }
    state.reverse_pending.clear();
}

fn respond_null(connection: &Connection, id: RequestId) {
    let _ = connection
        .sender
        .send(Message::Response(Response::new_ok(id, Value::Null)));
}

/// Rewrites a client-issued `$/cancelRequest`'s `id` through
/// [`State`]'s pending map before forwarding it to rust-analyzer (issue
/// #9 Gate 3 review, M9/S2/HIGH-5): the id in `params.id` is the
/// *editor's* id for the request it wants cancelled, but rust-analyzer
/// was sent that request under this server's own, independent id
/// (`RaClient::next_id`, a separate counter from the editor's), so
/// forwarding `params.id` verbatim either cancels the wrong rust-analyzer
/// request (if the two id spaces happen to collide) or nothing at all
/// (confirmed live: a divergent-id cancel delivered the full completion
/// result instead of `-32800 canceled by client`). A cancel for a
/// request this server already answered locally (`crate::complete`) or
/// is not currently waiting on is silently dropped, matching ordinary
/// LSP cancel semantics (cancelling an already-finished request is a
/// no-op).
pub(super) fn handle_cancel_request(state: &State, params: Value) {
    let Some(ra_id) = resolve_cancel_target(state, &params) else {
        return;
    };
    if let Some(ra) = &state.ra {
        ra.notify(
            "$/cancelRequest",
            json!({ "id": serde_json::to_value(ra_id).unwrap() }),
        );
    }
}

/// The rust-analyzer-side request id that a client `$/cancelRequest`'s
/// `params.id` (an *editor* id) corresponds to, or `None` if it names a
/// request this server does not currently have pending (already
/// answered, answered locally, or unknown).
fn resolve_cancel_target(state: &State, params: &Value) -> Option<RequestId> {
    let client_id: RequestId = serde_json::from_value(params.get("id")?.clone()).ok()?;
    state
        .pending
        .iter()
        .find(|(_, pending)| pending.client_id == client_id)
        .map(|(ra_id, _)| ra_id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// S2 (issue #9 Gate 3 review, HIGH-5): a `$/cancelRequest` names the
    /// request in the *editor's* id space; this must resolve to the
    /// independent id rust-analyzer was actually sent it under.
    #[test]
    fn resolve_cancel_target_rewrites_the_editor_id_to_the_ra_id() {
        let mut state = State::new();
        let ra_id = RequestId::from(42);
        state.pending.insert(
            ra_id.clone(),
            Pending {
                client_id: RequestId::from(7),
                kind: PendingKind::Forward,
                epoch: 0,
            },
        );

        assert_eq!(
            resolve_cancel_target(&state, &json!({ "id": 7 })),
            Some(ra_id)
        );
    }

    #[test]
    fn resolve_cancel_target_is_none_for_an_unknown_editor_id() {
        let state = State::new();
        assert_eq!(resolve_cancel_target(&state, &json!({ "id": 999 })), None);
    }

    /// S5 (issue #9 Gate 3 review, MEDIUM-11): every request waiting on a
    /// response must get an answer, not hang forever, once rust-analyzer
    /// is known to be gone.
    #[test]
    fn fail_all_pending_answers_every_in_flight_request_with_an_error() {
        let mut state = State::new();
        state.pending.insert(
            RequestId::from(1),
            Pending {
                client_id: RequestId::from(10),
                kind: PendingKind::Forward,
                epoch: 0,
            },
        );
        state.pending.insert(
            RequestId::from(2),
            Pending {
                client_id: RequestId::from(11),
                kind: PendingKind::Definition,
                epoch: 0,
            },
        );
        let (connection, client) = Connection::memory();

        fail_all_pending(&mut state, &connection);

        assert!(state.pending.is_empty());
        let mut seen_client_ids = std::collections::HashSet::new();
        for _ in 0..2 {
            let msg = client
                .receiver
                .recv_timeout(std::time::Duration::from_secs(1))
                .expect("a response for each pending request");
            match msg {
                Message::Response(response) => {
                    assert!(response.response_result.is_err());
                    seen_client_ids.insert(response.id);
                }
                other => panic!("expected a response, got {other:?}"),
            }
        }
        assert_eq!(
            seen_client_ids,
            [RequestId::from(10), RequestId::from(11)]
                .into_iter()
                .collect()
        );
    }
}
