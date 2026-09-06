//! Everything coming back from rust-analyzer: responses to requests this
//! server forwarded on the editor's behalf (mapped back through
//! `crate::mapping`/`crate::response`), rust-analyzer's own requests
//! *to* the editor, and its notifications (`publishDiagnostics`). Also
//! [`dispatch_client_response`], which routes an editor response back to
//! rust-analyzer for a request forwarded the other way
//! ([`forward_ra_request_to_client`]).

use lsp_server::{Connection, Message, Notification, Request, Response, ResponseError};
use lsp_types::PublishDiagnosticsParams;
use serde_json::Value;

use super::{PendingKind, State};
use crate::{diagnostics, response};

pub(crate) fn dispatch_ra_message(state: &mut State, connection: &Connection, message: Message) {
    match message {
        Message::Response(response) => handle_ra_response(state, connection, response),
        Message::Request(request) => handle_ra_request(state, connection, request),
        Message::Notification(notification) => {
            handle_ra_notification(state, connection, notification)
        }
    }
}

/// Handles a request rust-analyzer sent *to* this server (as opposed to a
/// response to one this server sent it). Per issue #9 Gate 3 review S2 —
/// and its own README's previous, inaccurate "forwarded transparently"
/// claim (S7) — most of these were answered locally with `null` and
/// never reached the editor at all. Three are now handled honestly:
///
/// - `workspace/configuration` is answered locally with an array of
///   `null`s the same length as the request (never `null` itself, which
///   is not a valid `workspace/configuration` result and can make a
///   rust-analyzer that actually asked for its own settings misbehave).
/// - `client/registerCapability` and `window/workDoneProgress/create`
///   are forwarded to the editor — with a freshly allocated id, tracked
///   in [`State::reverse_pending`] so the editor's response can be routed
///   back — when the editor's own `initialize` capabilities advertised
///   support for the relevant mechanism; otherwise (an editor that never
///   asked for dynamic registration, or `outou-lsp-client.mjs`'s own
///   minimal test capabilities) they are still answered locally with
///   `null`, exactly as before.
/// - Everything else is still answered locally with `null`: this server
///   has no case where rust-analyzer's other requests
///   (`workspace/workspaceFolders`, `workspace/semanticTokens/refresh`,
///   …) need a real editor answer to keep working.
fn handle_ra_request(state: &mut State, connection: &Connection, request: Request) {
    match request.method.as_str() {
        "workspace/configuration" => {
            let count = request
                .params
                .get("items")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            if let Some(ra) = &state.ra {
                ra.send(Message::Response(Response::new_ok(
                    request.id,
                    Value::Array(vec![Value::Null; count]),
                )));
            }
        }
        "client/registerCapability" if state.client_supports_dynamic_registration => {
            forward_ra_request_to_client(state, connection, request)
        }
        "window/workDoneProgress/create" if state.client_supports_work_done_progress => {
            forward_ra_request_to_client(state, connection, request)
        }
        _ => {
            if let Some(ra) = &state.ra {
                ra.send(Message::Response(Response::new_ok(request.id, Value::Null)));
            }
        }
    }
}

/// Relays one rust-analyzer -> server request on to the editor under a
/// freshly allocated id, remembering the mapping in
/// [`State::reverse_pending`] so [`dispatch_client_response`] can route
/// the eventual answer back to rust-analyzer under its own original id.
fn forward_ra_request_to_client(state: &mut State, connection: &Connection, request: Request) {
    let forwarded_id = state.next_forwarded_client_id();
    state
        .reverse_pending
        .insert(forwarded_id.clone(), request.id);
    let _ = connection.sender.send(Message::Request(Request {
        id: forwarded_id,
        method: request.method,
        params: request.params,
    }));
}

/// Handles a response the editor sent to a request *this server*
/// initiated on rust-analyzer's behalf (`forward_ra_request_to_client`):
/// routes it back to rust-analyzer under rust-analyzer's own original id.
/// A response whose id is not in [`State::reverse_pending`] is an
/// ordinary response to one of the editor's *own* requests to this
/// server and is not reachable here (`lsp_server::Connection`'s
/// `handle_shutdown`/request handling never produces one on this
/// channel); still ignored defensively rather than panicking.
pub(crate) fn dispatch_client_response(state: &mut State, response: Response) {
    let Some(ra_id) = state.reverse_pending.remove(&response.id) else {
        return;
    };
    if let Some(ra) = &state.ra {
        ra.send(Message::Response(Response {
            id: ra_id,
            response_result: response.response_result,
        }));
    }
}

/// LSP's own cancellation error code (`ErrorCodes.RequestCancelled`).
const REQUEST_CANCELLED: i32 = -32800;

fn handle_ra_response(state: &mut State, connection: &Connection, response: Response) {
    let Some(pending) = state.pending.remove(&response.id) else {
        return;
    };
    let result = if pending.epoch != state.epoch {
        // S3 (issue #9 Gate 3 review): the workspace has moved on to a
        // later epoch (a crate-wide re-plan happened) since this request
        // was sent — the generated file/position it was mapped against
        // may no longer exist or may mean something different now.
        // Reported as an ordinary LSP cancellation rather than mapped
        // against stale state.
        Err(ResponseError {
            code: REQUEST_CANCELLED,
            message: "outou-lsp: the workspace was re-planned while this request was in flight"
                .to_string(),
            data: None,
        })
    } else {
        map_pending_result(state, pending.kind, response.response_result)
    };
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
            response::sanitize_hover(workspace, &generated_uri, &mut value)
        }
        PendingKind::Definition => response::map_definition_response(workspace, &mut value),
        PendingKind::Completion {
            generated_uri,
            cursor,
            rsx_cursor,
        } => response::map_completion_response(
            workspace,
            &generated_uri,
            cursor,
            rsx_cursor,
            &mut value,
        ),
    }
    Ok(value)
}

/// Handles a notification from rust-analyzer: `publishDiagnostics` is
/// merged and republished for the owning `.rsx` file
/// (`diagnostics::handle_ra_publish`); `$/progress` is relayed to the
/// editor verbatim, under its own token, so a real editor gets a
/// readiness signal for rust-analyzer's indexing (issue #9 Gate 3 review,
/// S6) — `outou-lsp-client.mjs` used to work around the lack of one with
/// bounded request retries (see its own module doc comment; it now
/// listens for `$/progress` `end` instead).
///
/// No token rewriting is needed: every token this server ever sees in a
/// `$/progress` notification was itself allocated by the *editor*, in
/// its response to the `window/workDoneProgress/create` request this
/// server forwarded on rust-analyzer's behalf (`handle_ra_request` below)
/// — never one this server or rust-analyzer invented independently — so
/// there is nothing to collide with.
///
/// Forwarding is gated on the exact same condition as forwarding
/// `create` itself (`state.client_supports_work_done_progress`, issue #9
/// Gate 3 review, L12): a client that never advertised
/// `window.workDoneProgress` never saw a `create` request either, so it
/// has no token to match a forwarded `$/progress` against — sending one
/// anyway would be a notification for a progress report the editor never
/// agreed to track. Keeping both behind the same flag means the two can
/// never drift out of sync the way L12 found them.
fn handle_ra_notification(state: &mut State, connection: &Connection, notification: Notification) {
    match notification.method.as_str() {
        "textDocument/publishDiagnostics" => {
            if let Ok(params) =
                serde_json::from_value::<PublishDiagnosticsParams>(notification.params)
            {
                if let Some(workspace) = &mut state.workspace {
                    diagnostics::handle_ra_publish(connection, workspace, params);
                }
            }
        }
        "$/progress" if state.client_supports_work_done_progress => {
            let _ = connection.sender.send(Message::Notification(Notification {
                method: notification.method,
                params: notification.params,
            }));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_server::RequestId;
    use serde_json::json;

    use super::super::Pending;

    /// S2: a client response to a rust-analyzer -> client request this
    /// server forwarded must be consumed from `reverse_pending` (and, with
    /// a live rust-analyzer attached — exercised by the gate3 integration
    /// test, not reachable in this unit test without spawning one — routed
    /// back under rust-analyzer's own original id).
    #[test]
    fn dispatch_client_response_consumes_the_reverse_pending_entry() {
        let mut state = State::new();
        let forwarded = state.next_forwarded_client_id();
        state
            .reverse_pending
            .insert(forwarded.clone(), RequestId::from(5));

        dispatch_client_response(&mut state, Response::new_ok(forwarded, Value::Null));

        assert!(state.reverse_pending.is_empty());
    }

    #[test]
    fn dispatch_client_response_ignores_an_unknown_id() {
        let mut state = State::new();
        // Must not panic.
        dispatch_client_response(
            &mut state,
            Response::new_ok(RequestId::from(1), Value::Null),
        );
    }

    /// S3 (issue #9 Gate 3 review): a rust-analyzer response that arrives
    /// after the workspace has moved on to a later epoch (a crate-wide
    /// re-plan happened while the request was in flight) must be answered
    /// `RequestCancelled`, not mapped against state the response was
    /// never computed against.
    #[test]
    fn handle_ra_response_cancels_a_response_from_a_stale_epoch() {
        let mut state = State::new();
        let ra_id = RequestId::from(1);
        state.pending.insert(
            ra_id.clone(),
            Pending {
                client_id: RequestId::from(1),
                kind: PendingKind::Forward,
                epoch: 0,
            },
        );
        state.epoch = 1;
        let (connection, client) = Connection::memory();

        handle_ra_response(
            &mut state,
            &connection,
            Response::new_ok(ra_id, json!({ "some": "result" })),
        );

        let msg = client
            .receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("a response was sent to the editor");
        match msg {
            Message::Response(response) => {
                let error = response
                    .response_result
                    .expect_err("a stale-epoch response must be an error");
                assert_eq!(error.code, REQUEST_CANCELLED);
            }
            other => panic!("expected a response, got {other:?}"),
        }
    }

    /// S6/L12: `$/progress` is forwarded to the editor verbatim when it
    /// advertised `window.workDoneProgress` support (the same flag that
    /// gates forwarding `window/workDoneProgress/create` in the first
    /// place).
    #[test]
    fn handle_ra_notification_forwards_progress_when_the_client_supports_it() {
        let mut state = State::new();
        state.client_supports_work_done_progress = true;
        let (connection, client) = Connection::memory();

        handle_ra_notification(
            &mut state,
            &connection,
            Notification::new(
                "$/progress".to_string(),
                json!({ "token": "rustAnalyzer/Indexing", "value": { "kind": "end" } }),
            ),
        );

        let msg = client
            .receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("the progress notification was forwarded");
        match msg {
            Message::Notification(note) => {
                assert_eq!(note.method, "$/progress");
                assert_eq!(note.params["token"], "rustAnalyzer/Indexing");
            }
            other => panic!("expected a notification, got {other:?}"),
        }
    }

    /// L12: kept consistent with `create`'s own gating — a client that
    /// never advertised `window.workDoneProgress` never gets `$/progress`
    /// either, since it was never asked to create the token in the first
    /// place.
    #[test]
    fn handle_ra_notification_drops_progress_when_the_client_does_not_support_it() {
        let mut state = State::new();
        assert!(!state.client_supports_work_done_progress);
        let (connection, client) = Connection::memory();

        handle_ra_notification(
            &mut state,
            &connection,
            Notification::new(
                "$/progress".to_string(),
                json!({ "token": "rustAnalyzer/Indexing", "value": { "kind": "end" } }),
            ),
        );

        assert!(
            client.receiver.try_recv().is_err(),
            "no $/progress should reach a client that never advertised support for it"
        );
    }

    #[test]
    fn handle_ra_response_maps_a_response_from_the_current_epoch() {
        let mut state = State::new();
        let ra_id = RequestId::from(1);
        state.pending.insert(
            ra_id.clone(),
            Pending {
                client_id: RequestId::from(1),
                kind: PendingKind::Forward,
                epoch: 0,
            },
        );
        let (connection, client) = Connection::memory();

        handle_ra_response(
            &mut state,
            &connection,
            Response::new_ok(ra_id, json!({ "some": "result" })),
        );

        let msg = client
            .receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("a response was sent to the editor");
        match msg {
            Message::Response(response) => {
                assert_eq!(
                    response.response_result.expect("not an error"),
                    json!({ "some": "result" })
                );
            }
            other => panic!("expected a response, got {other:?}"),
        }
    }
}
