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

use super::{Pending, PendingKind, State};
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

/// LSP's own `ErrorCodes.ContentModified` (3.16.0): "the content of a
/// document got modified outside normal conditions." Used when a
/// rename/references/semantic-tokens response arrives after its
/// originating `.rsx` *document* changed version — a finer-grained check
/// than [`REQUEST_CANCELLED`]'s crate-wide epoch, needed because an
/// ordinary `didChange` regeneration does not bump the epoch at all (issue
/// #14 review, BLOCKING-3).
const CONTENT_MODIFIED: i32 = -32801;

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
    } else if let Some(stale) = stale_rsx_document(state, &pending) {
        stale
    } else {
        map_pending_result(state, pending.kind, response.response_result)
    };
    let _ = connection.sender.send(Message::Response(Response {
        id: pending.client_id,
        response_result: result,
    }));
}

/// Whether `pending`'s captured `.rsx` document version (`Pending::rsx_document`,
/// issue #14 review, BLOCKING-3) no longer matches the document's current
/// version, and — if so — the response this server must send instead of
/// translating rust-analyzer's answer against state it was never computed
/// for. `None` for a request this crate does not gate on staleness at all
/// (`Pending::rsx_document` was never captured for it), or when the
/// version still matches.
///
/// `PrepareRename` answers `null` (not renameable *right now*, rather than
/// a hard failure, for what is usually just a losing race with a fast
/// typist); every other staleness-checked kind (`Rename`, `References`,
/// `SemanticTokens`) answers `ContentModified` — silently applying a
/// rename computed against a buffer that no longer exists is exactly the
/// data-loss risk this check exists to prevent.
fn stale_rsx_document(state: &State, pending: &Pending) -> Option<Result<Value, ResponseError>> {
    let (rsx_uri, captured_version) = pending.rsx_document.as_ref()?;
    let workspace = state.workspace.as_ref()?;
    let current_version = workspace.rsx.get(rsx_uri).map(|doc| doc.version)?;
    if current_version == *captured_version {
        return None;
    }
    Some(match &pending.kind {
        PendingKind::PrepareRename { .. } => Ok(Value::Null),
        _ => Err(ResponseError {
            code: CONTENT_MODIFIED,
            message: format!(
                "Outou cannot safely answer here: `{rsx_uri}` changed while this request was in \
                 flight"
            ),
            data: None,
        }),
    })
}

fn map_pending_result(
    state: &State,
    kind: PendingKind,
    result: Result<Value, ResponseError>,
) -> Result<Value, ResponseError> {
    let mut value = result.map_err(sanitize_ra_error)?;
    let Some(workspace) = &state.workspace else {
        return Ok(value);
    };
    match kind {
        PendingKind::Forward => {}
        PendingKind::Hover { generated_uri } => {
            response::sanitize_hover(workspace, &generated_uri, &mut value)
        }
        PendingKind::Definition => response::map_definition_response(workspace, &mut value),
        PendingKind::PrepareRename { generated_uri } => {
            crate::rename::translate_prepare_rename_response(workspace, &generated_uri, &mut value)
        }
        PendingKind::Rename => {
            if let Err(refusal) = crate::rename::translate_workspace_edit(workspace, &mut value) {
                return Err(rename_refusal_error(refusal));
            }
        }
        PendingKind::References => {
            crate::references::translate_references_response(workspace, &mut value)
        }
        PendingKind::SemanticTokens {
            generated_uri,
            rsx_uri,
        } => crate::semantic_tokens::rewrite_response(
            workspace,
            &state.semantic_legend,
            &generated_uri,
            &rsx_uri,
            &mut value,
        ),
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

/// Builds the LSP error response for a rename this server refuses to
/// perform (`crate::rename::UnmappableRename`): part of what
/// rust-analyzer wants to rewrite lands in generated code with no `.rsx`
/// source, and applying the rest while silently dropping that part would
/// leave the rename half-done. `lsp_server::ErrorCode` (as vendored here)
/// has no closer standard code than `InternalError` for "the server
/// refuses this request for a reason specific to it", so the message
/// carries the actual explanation, in Outou vocabulary — never a backend
/// term (`AGENTS.md`).
///
/// The generated URI is logged (`eprintln!`) rather than put in the
/// message sent to the editor (issue #14 review, SHOULD-LAND-5): a
/// `src/.generated/…` path is exactly the kind of backend-internal detail
/// `AGENTS.md` forbids in user-facing output, even though it is Outou's
/// own error rather than rust-analyzer's.
fn rename_refusal_error(refusal: crate::rename::UnmappableRename) -> ResponseError {
    eprintln!(
        "outou-lsp: refusing a rename: no `.rsx` source for {}",
        refusal.generated_uri
    );
    ResponseError {
        code: lsp_server::ErrorCode::InternalError as i32,
        message: "Outou cannot safely rename here: part of the result has no corresponding \
                   `.rsx` location, so the rename was not applied at all"
            .to_string(),
        data: None,
    }
}

/// Sanitizes a rust-analyzer error response before it ever reaches the
/// editor (issue #14 review, SHOULD-LAND-5): reproduced live, an
/// unsanitized rust-analyzer error can name backend vocabulary (`rsx!`,
/// `dioxus_*`, …) or a `src/.generated/…` path, both forbidden in
/// user-facing output by `AGENTS.md` — every other payload this server
/// answers already goes through `crate::translate`/`crate::response` for
/// exactly this reason, but an error response used to skip that step
/// entirely (`result?` propagated it verbatim). The original message is
/// still logged (`eprintln!`) for diagnosability; only the message the
/// editor sees is replaced.
fn sanitize_ra_error(mut error: ResponseError) -> ResponseError {
    let needs_sanitizing = crate::translate::contains_backend_marker(&error.message)
        || error.message.contains(".generated");
    if !needs_sanitizing {
        return error;
    }
    eprintln!("outou-lsp: sanitizing a rust-analyzer error before forwarding it: {error:?}");
    error.message = "Outou cannot answer this request: the underlying error names internal, \
                      generated implementation detail rather than anything in your `.rsx` file"
        .to_string();
    error.data = None;
    error
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

    /// Issue #14 review (SHOULD-LAND-5): reproduced live, a rust-analyzer
    /// error mentioning backend vocabulary must never reach the editor
    /// verbatim.
    #[test]
    fn sanitize_ra_error_replaces_a_message_naming_backend_vocabulary() {
        let error = ResponseError {
            code: -32600,
            message: "cannot expand `rsx!` here: PropsBuilder mismatch".to_string(),
            data: None,
        };
        let sanitized = sanitize_ra_error(error);
        assert!(!sanitized.message.contains("rsx!"));
        assert!(!sanitized.message.contains("PropsBuilder"));
    }

    /// A `src/.generated/…` path leak is caught even without a
    /// `contains_backend_marker` hit.
    #[test]
    fn sanitize_ra_error_replaces_a_message_naming_a_generated_path() {
        let error = ResponseError {
            code: -32600,
            message: "error at src/.generated/main.rs:12:4".to_string(),
            data: None,
        };
        let sanitized = sanitize_ra_error(error);
        assert!(!sanitized.message.contains(".generated"));
    }

    /// An ordinary rust-analyzer error with no backend vocabulary at all
    /// passes through unchanged.
    #[test]
    fn sanitize_ra_error_leaves_an_ordinary_message_untouched() {
        let error = ResponseError {
            code: -32600,
            message: "Cannot rename a non-local definition".to_string(),
            data: None,
        };
        let sanitized = sanitize_ra_error(error.clone());
        assert_eq!(sanitized.message, error.message);
    }

    /// The rename refusal message itself must not leak the generated URI
    /// either (issue #14 review, SHOULD-LAND-5).
    #[test]
    fn rename_refusal_error_does_not_leak_the_generated_uri() {
        let error = rename_refusal_error(crate::rename::UnmappableRename {
            generated_uri: "file:///app/src/.generated/main.rs".to_string(),
        });
        assert!(!error.message.contains(".generated"));
        assert!(!error.message.contains("file:///"));
    }

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
                rsx_document: None,
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

    /// A workspace with one component `<Greeting>hi</Greeting>` whose
    /// identifier's generated occurrence maps back to both tags, shared by
    /// the dispatch-level rename/references/semantic-tokens tests below —
    /// the same fixture shape `crate::rename`, `crate::references` and
    /// `crate::semantic_tokens`'s own unit tests build independently, but
    /// exercised here through the *dispatch* layer (`handle_ra_response`)
    /// with a fake rust-analyzer response, per the issue's own requirement.
    fn dispatch_sample_workspace() -> (crate::documents::Workspace, lsp_types::Uri, lsp_types::Uri)
    {
        use outou_sourcemap::{
            file_uri, Mapping, MappingKind, Registry, SourceId, SourceMap, SourceSpan, Span,
        };
        use std::path::Path;

        let rsx_path = Path::new("/app/src/main.rsx");
        let generated_path = Path::new("/app/src/.generated/main.rs");
        let rsx_uri = file_uri(rsx_path);
        let generated_uri = file_uri(generated_path);

        let rsx_source = "fn f() { <Greeting>hi</Greeting> }";
        let generated_text = "let _ = Greeting;";
        let open_start = rsx_source.find("Greeting").unwrap() as u32;
        let close_start = rsx_source.rfind("Greeting").unwrap() as u32;

        let map = SourceMap::new(generated_uri.clone(), vec![rsx_uri.clone()]).with_mapping(
            Mapping::new(
                Span::new(8, 16),
                vec![
                    SourceSpan::new(SourceId(0), Span::new(open_start, open_start + 8)),
                    SourceSpan::new(SourceId(0), Span::new(close_start, close_start + 8)),
                ],
                MappingKind::Identifier,
            ),
        );
        let registry = Registry::new().with_map(map);

        let mut workspace = crate::documents::Workspace {
            manifest_dir: std::path::PathBuf::from("/app"),
            plan: None,
            registry,
            rsx: std::collections::HashMap::new(),
            generated: std::collections::HashMap::new(),
            rsx_to_generated: std::collections::HashMap::new(),
        };
        workspace.rsx.insert(
            rsx_uri.as_str().to_string(),
            crate::documents::RsxDocument::new(rsx_source.to_string(), 1),
        );
        workspace.generated.insert(
            generated_uri.as_str().to_string(),
            crate::documents::test_generated_unit(&generated_uri, &rsx_uri, generated_text),
        );
        workspace.rsx_to_generated.insert(
            rsx_uri.as_str().to_string(),
            generated_uri.as_str().to_string(),
        );

        (
            workspace,
            crate::uri::to_lsp(&rsx_uri),
            crate::uri::to_lsp(&generated_uri),
        )
    }

    /// Dispatch-level: `PendingKind::References`'s fake rust-analyzer
    /// response (one `Location` in the generated file) reaches the editor
    /// as two `.rsx` locations (opening + closing tag).
    #[test]
    fn handle_ra_response_translates_a_references_response_end_to_end() {
        let (workspace, rsx_uri, generated_uri) = dispatch_sample_workspace();
        let mut state = State::new();
        state.workspace = Some(workspace);
        let ra_id = RequestId::from(1);
        state.pending.insert(
            ra_id.clone(),
            Pending {
                client_id: RequestId::from(1),
                kind: PendingKind::References,
                epoch: 0,
                rsx_document: None,
            },
        );
        let (connection, client) = Connection::memory();

        handle_ra_response(
            &mut state,
            &connection,
            Response::new_ok(
                ra_id,
                json!([{ "uri": generated_uri.as_str(), "range": {"start": {"line":0,"character":8}, "end": {"line":0,"character":16}}}]),
            ),
        );

        let Message::Response(response) = client
            .receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap()
        else {
            panic!("expected a response");
        };
        let locations = response.response_result.unwrap();
        let items = locations.as_array().unwrap();
        assert_eq!(items.len(), 2, "{items:#?}");
        for item in items {
            assert_eq!(item["uri"], rsx_uri.as_str());
        }
    }

    /// Issue #14 review (BLOCKING-3): a rename response whose `.rsx`
    /// document changed version since the request was forwarded (an
    /// ordinary `didChange` regeneration, which does **not** bump
    /// `State::epoch`) must be refused as `ContentModified`, never
    /// translated and stamped with whatever version the document happens
    /// to be at *now* — the request was computed against a buffer that no
    /// longer exists.
    #[test]
    fn handle_ra_response_refuses_a_rename_whose_rsx_document_changed() {
        let (workspace, rsx_uri, generated_uri) = dispatch_sample_workspace();
        let rsx_uri_string = crate::uri::to_outou(&rsx_uri).as_str().to_string();
        let mut state = State::new();
        state.workspace = Some(workspace);
        let ra_id = RequestId::from(1);
        state.pending.insert(
            ra_id.clone(),
            Pending {
                client_id: RequestId::from(1),
                kind: PendingKind::Rename,
                epoch: 0,
                // The document is at version 1 (`dispatch_sample_workspace`);
                // this request was forwarded when it was still version 0.
                rsx_document: Some((rsx_uri_string, 0)),
            },
        );
        let (connection, client) = Connection::memory();

        handle_ra_response(
            &mut state,
            &connection,
            Response::new_ok(
                ra_id,
                json!({ "changes": { generated_uri.as_str(): [
                    { "range": {"start": {"line":0,"character":8}, "end": {"line":0,"character":16}}, "newText": "Welcome" }
                ]}}),
            ),
        );

        let Message::Response(response) = client
            .receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap()
        else {
            panic!("expected a response");
        };
        let error = response
            .response_result
            .expect_err("a stale-document rename must be refused, not applied");
        assert_eq!(error.code, CONTENT_MODIFIED);
    }

    /// Same staleness case, but for `PrepareRename`: per the review, this
    /// answers `null` (not renameable *right now*) rather than an error —
    /// `prepareRename` failing loudly for a transient race is worse than
    /// just not offering a rename this keystroke.
    #[test]
    fn handle_ra_response_nulls_a_prepare_rename_whose_rsx_document_changed() {
        let (workspace, rsx_uri, generated_uri) = dispatch_sample_workspace();
        let rsx_uri_string = crate::uri::to_outou(&rsx_uri).as_str().to_string();
        let mut state = State::new();
        state.workspace = Some(workspace);
        let ra_id = RequestId::from(1);
        state.pending.insert(
            ra_id.clone(),
            Pending {
                client_id: RequestId::from(1),
                kind: PendingKind::PrepareRename {
                    generated_uri: generated_uri.clone(),
                },
                epoch: 0,
                rsx_document: Some((rsx_uri_string, 0)),
            },
        );
        let (connection, client) = Connection::memory();

        handle_ra_response(
            &mut state,
            &connection,
            Response::new_ok(
                ra_id,
                json!({"start": {"line":0,"character":8}, "end": {"line":0,"character":16}}),
            ),
        );

        let Message::Response(response) = client
            .receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap()
        else {
            panic!("expected a response");
        };
        assert_eq!(response.response_result.unwrap(), Value::Null);
    }

    /// The non-stale case still works exactly as before: a matching
    /// captured version does not spuriously refuse anything.
    #[test]
    fn handle_ra_response_still_translates_a_rename_with_a_matching_rsx_version() {
        let (workspace, rsx_uri, generated_uri) = dispatch_sample_workspace();
        let rsx_uri_string = crate::uri::to_outou(&rsx_uri).as_str().to_string();
        let mut state = State::new();
        state.workspace = Some(workspace);
        let ra_id = RequestId::from(1);
        state.pending.insert(
            ra_id.clone(),
            Pending {
                client_id: RequestId::from(1),
                kind: PendingKind::Rename,
                epoch: 0,
                rsx_document: Some((rsx_uri_string, 1)),
            },
        );
        let (connection, client) = Connection::memory();

        handle_ra_response(
            &mut state,
            &connection,
            Response::new_ok(
                ra_id,
                json!({ "changes": { generated_uri.as_str(): [
                    { "range": {"start": {"line":0,"character":8}, "end": {"line":0,"character":16}}, "newText": "Welcome" }
                ]}}),
            ),
        );

        let Message::Response(response) = client
            .receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap()
        else {
            panic!("expected a response");
        };
        assert!(response.response_result.is_ok(), "{response:#?}");
        assert_eq!(
            response.response_result.unwrap()["changes"][rsx_uri.as_str()]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    /// Dispatch-level: `PendingKind::Rename`'s fake rust-analyzer
    /// `WorkspaceEdit` that touches synthesized (unmappable) generated
    /// code is refused end to end, as an LSP error response, not applied
    /// partially.
    #[test]
    fn handle_ra_response_refuses_an_unmappable_rename_end_to_end() {
        let (workspace, _rsx_uri, generated_uri) = dispatch_sample_workspace();
        let mut state = State::new();
        state.workspace = Some(workspace);
        let ra_id = RequestId::from(1);
        state.pending.insert(
            ra_id.clone(),
            Pending {
                client_id: RequestId::from(1),
                kind: PendingKind::Rename,
                epoch: 0,
                rsx_document: None,
            },
        );
        let (connection, client) = Connection::memory();

        handle_ra_response(
            &mut state,
            &connection,
            Response::new_ok(
                ra_id,
                json!({ "changes": { generated_uri.as_str(): [
                    { "range": {"start": {"line":0,"character":0}, "end": {"line":0,"character":3}}, "newText": "xxx" }
                ]}}),
            ),
        );

        let Message::Response(response) = client
            .receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap()
        else {
            panic!("expected a response");
        };
        assert!(response.response_result.is_err());
    }

    /// Dispatch-level: `PendingKind::SemanticTokens`'s fake rust-analyzer
    /// response is merged with Outou's own overlay end to end.
    #[test]
    fn handle_ra_response_translates_a_semantic_tokens_response_end_to_end() {
        let (workspace, rsx_uri, generated_uri) = dispatch_sample_workspace();
        let mut state = State::new();
        state.workspace = Some(workspace);
        let rsx_uri_string = crate::uri::to_outou(&rsx_uri).as_str().to_string();
        let ra_id = RequestId::from(1);
        state.pending.insert(
            ra_id.clone(),
            Pending {
                client_id: RequestId::from(1),
                kind: PendingKind::SemanticTokens {
                    generated_uri: generated_uri.clone(),
                    rsx_uri: rsx_uri_string,
                },
                epoch: 0,
                rsx_document: None,
            },
        );
        let (connection, client) = Connection::memory();

        handle_ra_response(
            &mut state,
            &connection,
            Response::new_ok(ra_id, json!({ "data": [0, 8, 8, 12, 0] })),
        );

        let Message::Response(response) = client
            .receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap()
        else {
            panic!("expected a response");
        };
        let tokens: lsp_types::SemanticTokens =
            serde_json::from_value(response.response_result.unwrap()).unwrap();
        assert!(!tokens.data.is_empty());
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
                rsx_document: None,
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
