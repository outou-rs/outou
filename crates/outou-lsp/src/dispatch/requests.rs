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
use crate::documents::{RsxDocument, Workspace};
use crate::mapping;
use crate::uri;

pub(crate) fn dispatch_client_request(state: &mut State, connection: &Connection, req: Request) {
    match req.method.as_str() {
        "textDocument/hover" => dispatch_hover_request(state, connection, req),
        "textDocument/completion" => dispatch_completion_request(state, connection, req),
        "textDocument/definition" => {
            forward_position_request(state, connection, req, |_, _, _| PendingKind::Definition)
        }
        "textDocument/prepareRename" => dispatch_prepare_rename_request(state, connection, req),
        "textDocument/rename" => dispatch_rename_request(state, connection, req),
        "textDocument/references" => {
            forward_position_request(state, connection, req, |_, _, _| PendingKind::References)
        }
        "textDocument/formatting" => dispatch_formatting_request(state, connection, req),
        "textDocument/semanticTokens/full" => {
            dispatch_semantic_tokens_request(state, connection, req)
        }
        _ => forward_transparent_request(state, connection, req),
    }
}

/// `textDocument/formatting`: answered entirely locally, never forwarded
/// to rust-analyzer — formatting is Outou-syntax-driven
/// (`outou_fmt::format_source`), not a question about the *expanded*
/// Rust rust-analyzer sees, and it is the exact same pipeline `outou fmt`
/// uses (`docs/phase0/issues/13-formatter.md`: "one pipeline, not two").
///
/// `TODO(phase0)`: this runs synchronously, inline in the single
/// dispatch loop every other request also goes through, with no
/// wall-clock budget — one `rustfmt` process per file plus one more per
/// expression island (`crate::snippet`'s recursive placeholder passes),
/// so cost scales with island count, not file size. Measured directly
/// (this machine, release build): ~0.18s for the whole of
/// `examples/phase0-app/src/components.rsx` (a handful of islands); an
/// independent review reproduction reported low-single-digit seconds for
/// a small file engineered to contain dozens of islands. A future fix
/// should move this to a worker thread and answer `null` past some
/// deadline rather than block the whole server on a pathological file.
/// This also ignores the request's `options` (`tabSize`, `insertSpaces`)
/// entirely: `outou_fmt::FormatOptions` has no equivalent knobs yet, and
/// `rustfmt`'s own indent width is a fixed constant
/// (`outou_fmt::width::INDENT_UNIT`), not sourced from the editor.
///
/// Responds with `null` (no edits) rather than an LSP error response for
/// every case the issue's spec calls out as "never edit": an unknown
/// document, a file with a syntax error, or a file `outou-fmt` otherwise
/// refuses (`rustfmt` missing or failing) — an editor's format-on-save
/// must never be surprised by an error popup for a file that simply
/// cannot be formatted right now. An *operational* failure (`rustfmt`
/// missing or failing, as opposed to the file's own syntax error) is
/// still logged to this server's stderr rather than silently dropped, so
/// a misconfigured environment is diagnosable instead of just quietly
/// never formatting anything.
fn dispatch_formatting_request(state: &State, connection: &Connection, req: Request) {
    let client_id = req.id.clone();
    let Some(rsx_uri) = extract_document_uri(&req.params) else {
        respond_null(connection, client_id);
        return;
    };
    let rsx_uri_string = uri::to_outou(&rsx_uri).as_str().to_string();
    let Some(document) = find_rsx_document(state, &rsx_uri_string) else {
        respond_null(connection, client_id);
        return;
    };

    let source = document.line_index.text();
    let edits = match formatting_edits(source, &outou_fmt::FormatOptions::default()) {
        Ok(edits) => edits,
        Err(outou_fmt::FormatError::SyntaxErrors { .. }) => {
            respond_null(connection, client_id);
            return;
        }
        Err(err) => {
            eprintln!("outou-lsp: cannot format {rsx_uri_string}: {err}");
            respond_null(connection, client_id);
            return;
        }
    };

    let _ = connection.sender.send(Message::Response(Response::new_ok(
        client_id,
        serde_json::to_value(edits).unwrap_or(Value::Null),
    )));
}

/// The `lsp_types::TextEdit`-shaped JSON edits for formatting `source`, a
/// pure function of `source` and `options` (rebuilding its own
/// [`outou_sourcemap::LineIndex`] rather than requiring an
/// [`RsxDocument`]) so it can be unit-tested directly, including the
/// "operational failure, not a syntax error" case a real `.rsx` document
/// is not needed to exercise: `Ok(vec![])` when already formatted,
/// `Ok(vec![edit])` when not, and `Err` — never silently downgraded to
/// an empty edit list — for anything [`outou_fmt::format_source`] itself
/// returns `Err` for, including [`outou_fmt::FormatError::RustfmtUnavailable`]
/// and [`outou_fmt::FormatError::RustfmtFailed`].
fn formatting_edits(
    source: &str,
    options: &outou_fmt::FormatOptions,
) -> Result<Vec<Value>, outou_fmt::FormatError> {
    let formatted = outou_fmt::format_source(source, options)?;
    if formatted == source {
        return Ok(Vec::new());
    }
    let line_index = outou_sourcemap::LineIndex::new(source);
    Ok(vec![full_document_edit(&line_index, source, formatted)])
}

fn extract_document_uri(params: &Value) -> Option<lsp_types::Uri> {
    params
        .get("textDocument")?
        .get("uri")?
        .as_str()?
        .parse()
        .ok()
}

/// Finds `rsx_uri_string`'s document regardless of whether this server is
/// running against a planned [`crate::documents::Workspace`] or, in
/// degraded mode (no `.rsx` crate root), only tracking documents in
/// [`State::degraded_docs`] — formatting needs only the `.rsx` text
/// itself, never the plan or a generated unit, so both modes answer it
/// the same way.
fn find_rsx_document<'a>(state: &'a State, rsx_uri_string: &str) -> Option<&'a RsxDocument> {
    if let Some(workspace) = &state.workspace {
        if let Some(document) = workspace.rsx.get(rsx_uri_string) {
            return Some(document);
        }
    }
    state.degraded_docs.get(rsx_uri_string)
}

/// One `lsp_types::TextEdit`-shaped JSON value replacing the whole
/// document, from `(0, 0)` to the end of `source` as `line_index` (built
/// from `source`, before formatting) converts it — the position
/// convention every other response in this crate already uses
/// (UTF-16 code units, `outou_sourcemap::LineIndex`).
fn full_document_edit(
    line_index: &outou_sourcemap::LineIndex,
    source: &str,
    formatted: String,
) -> Value {
    let whole_document = outou_sourcemap::Span::new(0, source.len() as u32);
    let range = mapping::to_lsp_range(line_index.span_to_range(whole_document));
    json!({
        "range": range,
        "newText": formatted,
    })
}

/// `textDocument/hover` needs the same local/forward split H1 gave
/// completion (issue #9 Gate 3 review, H2): a tag-name position (element
/// or component, opening or closing) is answered locally with `null`,
/// never forwarded — see `crate::complete::is_tag_name_position`'s own
/// doc comment for why.
fn dispatch_hover_request(state: &mut State, connection: &Connection, req: Request) {
    if let Some(workspace) = &state.workspace {
        if let Some((rsx_uri, position)) = extract_position_params(&req.params) {
            let rsx_uri_string = uri::to_outou(&rsx_uri).as_str().to_string();
            if complete::is_tag_name_position(workspace, &rsx_uri_string, position) {
                let _ = connection
                    .sender
                    .send(Message::Response(Response::new_ok(req.id, Value::Null)));
                return;
            }
        }
    }
    forward_position_request(state, connection, req, |uri, _pos, _rsx_pos| {
        PendingKind::Hover { generated_uri: uri }
    });
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
    forward_position_request(state, connection, req, |uri, cursor, rsx_cursor| {
        PendingKind::Completion {
            generated_uri: uri,
            cursor,
            rsx_cursor,
        }
    });
}

/// `textDocument/semanticTokens/full`: forwarded to rust-analyzer for the
/// generated file whenever one is attached and known (mapped back by
/// [`crate::semantic_tokens::rewrite_response`]); otherwise — degraded
/// mode, or a workspace whose rust-analyzer failed to start, or a document
/// this server has no generated unit for — answered immediately with
/// Outou's own AST-derived tokens alone
/// (`crate::semantic_tokens::local_only_response`), never `null`: JSX
/// vocabulary (component vs. element, event attributes, …) does not
/// depend on rust-analyzer at all.
fn dispatch_semantic_tokens_request(state: &mut State, connection: &Connection, req: Request) {
    let client_id = req.id.clone();
    let Some(rsx_uri) = extract_document_uri(&req.params) else {
        respond_null(connection, client_id);
        return;
    };
    let rsx_uri_string = uri::to_outou(&rsx_uri).as_str().to_string();

    if forward_semantic_tokens_request(state, &rsx_uri_string, &req, client_id.clone()) {
        return;
    }

    let Some(document) = find_rsx_document(state, &rsx_uri_string) else {
        respond_null(connection, client_id);
        return;
    };
    let value = crate::semantic_tokens::local_only_response(
        document.line_index.text(),
        &state.semantic_legend,
    );
    let _ = connection
        .sender
        .send(Message::Response(Response::new_ok(client_id, value)));
}

/// Whether a `textDocument/semanticTokens/full` request should be
/// forwarded to rust-analyzer at all (issue #14 review, SHOULD-LAND-7): a
/// live rust-analyzer is necessary but not sufficient — one that never
/// advertised `semanticTokensProvider` in its own `initialize` response
/// (an older or differently-configured version) would only ever answer
/// `MethodNotFound`, which used to reach the editor as a raw RA error
/// instead of this server falling back to `local_only_response` the way
/// every other unsupported-rust-analyzer case already does. A free
/// function of two plain `bool`s (rather than `&State` directly) so it is
/// unit-testable without a real [`crate::ra::RaClient`], which this crate
/// has no test double for.
fn ra_can_serve_semantic_tokens(has_ra: bool, ra_supports_semantic_tokens: bool) -> bool {
    has_ra && ra_supports_semantic_tokens
}

/// Forwards `req` to rust-analyzer for `rsx_uri_string`'s generated unit,
/// returning `true` if it did (the caller must not answer again). Returns
/// `false` when there is no workspace, no rust-analyzer, rust-analyzer
/// never advertised semantic tokens support, or there is no known
/// generated unit for this document — the caller falls back to answering
/// locally.
fn forward_semantic_tokens_request(
    state: &mut State,
    rsx_uri_string: &str,
    req: &Request,
    client_id: RequestId,
) -> bool {
    if !ra_can_serve_semantic_tokens(state.ra.is_some(), state.ra_supports_semantic_tokens) {
        return false;
    }
    let Some(workspace) = &state.workspace else {
        return false;
    };
    let Some(generated_uri_string) = workspace.rsx_to_generated.get(rsx_uri_string) else {
        return false;
    };
    let Some(unit) = workspace.generated.get(generated_uri_string) else {
        return false;
    };
    let generated_uri_lsp = uri::to_lsp(&unit.generated_uri);
    let rsx_version = workspace.rsx.get(rsx_uri_string).map(|doc| doc.version);
    let Some(ra) = &state.ra else {
        return false;
    };

    let mut params = req.params.clone();
    params["textDocument"]["uri"] = serde_json::to_value(&generated_uri_lsp).unwrap();

    let ra_id = ra.next_id();
    state.pending.insert(
        ra_id.clone(),
        Pending {
            client_id,
            kind: PendingKind::SemanticTokens {
                generated_uri: generated_uri_lsp,
                rsx_uri: rsx_uri_string.to_string(),
            },
            epoch: state.epoch,
            rsx_document: rsx_version.map(|version| (rsx_uri_string.to_string(), version)),
        },
    );
    ra.send(Message::Request(Request {
        id: ra_id,
        method: req.method.clone(),
        params,
    }));
    true
}

/// Whether `position` inside `rsx_uri_string` sits on an *intrinsic* HTML
/// element's tag name (opening or closing), as opposed to a component's
/// (issue #14 review, SHOULD-LAND-6): reproduced live, forwarding a
/// rename/prepareRename request for `<div>` to rust-analyzer depends on
/// it happening to refuse a backend macro token on its own — this server
/// never guaranteed that itself, and a rust-analyzer that *did* return an
/// edit for it would rename backend-internal HTML vocabulary a user never
/// wrote. `crate::complete::tag_name_at_position` reuses the same AST
/// walk `crate::complete`'s own tag-name completion/hover classification
/// already relies on; grammar §5's own rule (uppercase-initial =
/// component) decides the rest.
fn is_intrinsic_tag_rename(
    workspace: &Workspace,
    rsx_uri_string: &str,
    position: lsp_types::Position,
) -> bool {
    complete::tag_name_at_position(workspace, rsx_uri_string, position)
        .is_some_and(|name| name.chars().next().is_some_and(|c| !c.is_ascii_uppercase()))
}

/// `textDocument/prepareRename` on an intrinsic tag name answers `null`
/// (not renameable) locally, never forwarded — see
/// [`is_intrinsic_tag_rename`]. Every other position forwards as usual.
fn dispatch_prepare_rename_request(state: &mut State, connection: &Connection, req: Request) {
    if let Some(workspace) = &state.workspace {
        if let Some((rsx_uri, position)) = extract_position_params(&req.params) {
            let rsx_uri_string = uri::to_outou(&rsx_uri).as_str().to_string();
            if is_intrinsic_tag_rename(workspace, &rsx_uri_string, position) {
                respond_null(connection, req.id);
                return;
            }
        }
    }
    forward_position_request(state, connection, req, |uri, _, _| {
        PendingKind::PrepareRename { generated_uri: uri }
    });
}

/// `textDocument/rename` on an intrinsic tag name is refused locally with
/// an Outou-vocabulary error, never forwarded — see
/// [`is_intrinsic_tag_rename`]. Every other position forwards as usual.
fn dispatch_rename_request(state: &mut State, connection: &Connection, req: Request) {
    if let Some(workspace) = &state.workspace {
        if let Some((rsx_uri, position)) = extract_position_params(&req.params) {
            let rsx_uri_string = uri::to_outou(&rsx_uri).as_str().to_string();
            if is_intrinsic_tag_rename(workspace, &rsx_uri_string, position) {
                let _ = connection.sender.send(Message::Response(Response {
                    id: req.id,
                    response_result: Err(ResponseError {
                        code: lsp_server::ErrorCode::InvalidRequest as i32,
                        message: "Outou cannot rename an HTML element name; only components can \
                                  be renamed"
                            .to_string(),
                        data: None,
                    }),
                }));
                return;
            }
        }
    }
    forward_position_request(state, connection, req, |_, _, _| PendingKind::Rename);
}

fn forward_position_request(
    state: &mut State,
    connection: &Connection,
    req: Request,
    make_kind: impl FnOnce(lsp_types::Uri, lsp_types::Position, lsp_types::Position) -> PendingKind,
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
    let rsx_uri_string = uri::to_outou(&rsx_uri).as_str().to_string();
    let Some(mapped) = mapping::rsx_position_to_generated(workspace, &rsx_uri, position) else {
        respond_null(connection, client_id);
        return;
    };
    // issue #14 review (BLOCKING-3): captured so `dispatch::responses`
    // can refuse (rename) or drop (references/semantic tokens) a response
    // that arrives after this exact `.rsx` document changed underneath
    // it — an ordinary `didChange` regeneration does not bump `epoch`.
    let rsx_version = workspace.rsx.get(&rsx_uri_string).map(|doc| doc.version);
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
            kind: make_kind(mapped.generated_uri, mapped.position, position),
            epoch: state.epoch,
            rsx_document: rsx_version.map(|version| (rsx_uri_string, version)),
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
            rsx_document: None,
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

    /// Issue #14 review (SHOULD-LAND-7): forwarding
    /// `textDocument/semanticTokens/full` needs both a live rust-analyzer
    /// *and* its own advertised support for the request — either alone is
    /// not enough.
    #[test]
    fn ra_can_serve_semantic_tokens_requires_both_a_live_ra_and_its_advertised_support() {
        assert!(ra_can_serve_semantic_tokens(true, true));
        assert!(!ra_can_serve_semantic_tokens(true, false));
        assert!(!ra_can_serve_semantic_tokens(false, true));
        assert!(!ra_can_serve_semantic_tokens(false, false));
    }

    fn workspace_with_rsx(source: &str) -> (State, lsp_types::Uri) {
        let rsx_uri: lsp_types::Uri = "file:///app/src/main.rsx".parse().unwrap();
        let mut state = State::new();
        let mut workspace = Workspace {
            manifest_dir: std::path::PathBuf::from("/app"),
            plan: None,
            registry: outou_sourcemap::Registry::new(),
            rsx: std::collections::HashMap::new(),
            generated: std::collections::HashMap::new(),
            rsx_to_generated: std::collections::HashMap::new(),
        };
        workspace.rsx.insert(
            uri::to_outou(&rsx_uri).as_str().to_string(),
            RsxDocument::new(source.to_string(), 1),
        );
        state.workspace = Some(workspace);
        (state, rsx_uri)
    }

    fn position_of(
        state: &State,
        rsx_uri: &lsp_types::Uri,
        needle_offset: usize,
    ) -> lsp_types::Position {
        let workspace = state.workspace.as_ref().unwrap();
        let doc = workspace.rsx.get(uri::to_outou(rsx_uri).as_str()).unwrap();
        mapping::to_lsp_range(doc.line_index.span_to_range(outou_sourcemap::Span::new(
            needle_offset as u32,
            needle_offset as u32,
        )))
        .start
    }

    /// Issue #14 review (SHOULD-LAND-6): a rename request whose cursor
    /// sits on an *intrinsic* HTML element's tag name is refused locally
    /// — never sent to rust-analyzer at all (`state.pending` stays
    /// empty), which also means it needs no live rust-analyzer to test.
    #[test]
    fn dispatch_rename_request_refuses_an_intrinsic_tag_without_forwarding() {
        let source = "#[component]\nfn App() -> Element {\n    <div>hi</div>\n}\n";
        let (mut state, rsx_uri) = workspace_with_rsx(source);
        let offset = source.find("<div>").unwrap() + 2;
        let position = position_of(&state, &rsx_uri, offset);
        let (connection, client) = Connection::memory();

        dispatch_rename_request(
            &mut state,
            &connection,
            Request {
                id: RequestId::from(1),
                method: "textDocument/rename".to_string(),
                params: json!({
                    "textDocument": { "uri": rsx_uri.as_str() },
                    "position": { "line": position.line, "character": position.character },
                    "newName": "Section",
                }),
            },
        );

        let Message::Response(response) = client
            .receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap()
        else {
            panic!("expected a response");
        };
        assert!(response.response_result.is_err(), "{response:#?}");
        assert!(
            state.pending.is_empty(),
            "must never be forwarded to rust-analyzer"
        );
    }

    /// Same case for `prepareRename`: answered `null`, not an error, and
    /// also never forwarded.
    #[test]
    fn dispatch_prepare_rename_request_nulls_an_intrinsic_tag_without_forwarding() {
        let source = "#[component]\nfn App() -> Element {\n    <div>hi</div>\n}\n";
        let (mut state, rsx_uri) = workspace_with_rsx(source);
        let offset = source.find("<div>").unwrap() + 2;
        let position = position_of(&state, &rsx_uri, offset);
        let (connection, client) = Connection::memory();

        dispatch_prepare_rename_request(
            &mut state,
            &connection,
            Request {
                id: RequestId::from(1),
                method: "textDocument/prepareRename".to_string(),
                params: json!({
                    "textDocument": { "uri": rsx_uri.as_str() },
                    "position": { "line": position.line, "character": position.character },
                }),
            },
        );

        let Message::Response(response) = client
            .receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap()
        else {
            panic!("expected a response");
        };
        assert_eq!(response.response_result.unwrap(), Value::Null);
        assert!(
            state.pending.is_empty(),
            "must never be forwarded to rust-analyzer"
        );
    }

    /// A rename on a *component* tag name is not refused locally: with no
    /// rust-analyzer attached (`state.ra` is `None` in this unit test),
    /// it falls through to `forward_position_request`'s own `None` ->
    /// `null` handling — the point of this test is only that it does
    /// *not* take the intrinsic-refusal path.
    #[test]
    fn dispatch_rename_request_does_not_refuse_a_component_tag() {
        let source = "#[component]\nfn App() -> Element {\n    <Greeting>hi</Greeting>\n}\n";
        let (mut state, rsx_uri) = workspace_with_rsx(source);
        let offset = source.find("<Greeting>").unwrap() + 2;
        let position = position_of(&state, &rsx_uri, offset);
        let (connection, client) = Connection::memory();

        dispatch_rename_request(
            &mut state,
            &connection,
            Request {
                id: RequestId::from(1),
                method: "textDocument/rename".to_string(),
                params: json!({
                    "textDocument": { "uri": rsx_uri.as_str() },
                    "position": { "line": position.line, "character": position.character },
                    "newName": "Welcome",
                }),
            },
        );

        let Message::Response(response) = client
            .receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap()
        else {
            panic!("expected a response");
        };
        // No rust-analyzer attached, so `forward_position_request` itself
        // answers `null` (its own documented "no ra" behavior) — the
        // important assertion is that this is `Ok(Null)`, not the
        // `InvalidRequest` error the intrinsic-refusal path would send.
        assert_eq!(response.response_result.unwrap(), Value::Null);
    }

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

                rsx_document: None,
            },
        );

        assert_eq!(
            resolve_cancel_target(&state, &json!({ "id": 7 })),
            Some(ra_id)
        );
    }

    /// The bug this guards against: an operational failure (`rustfmt`
    /// missing) must be an `Err` all the way out of [`formatting_edits`],
    /// never silently turned into `Ok(vec![])` — which would look
    /// identical to "already formatted" to every caller, including
    /// [`dispatch_formatting_request`], hiding a misconfigured
    /// environment behind a formatter that quietly never does anything.
    #[test]
    fn formatting_edits_surfaces_a_rustfmt_unavailable_error_rather_than_an_empty_list() {
        let options = outou_fmt::FormatOptions {
            rustfmt_program: "outou-lsp-test-nonexistent-program-xyz".to_string(),
            ..outou_fmt::FormatOptions::default()
        };
        let result = formatting_edits("fn f() { <div /> }", &options);
        assert!(matches!(
            result,
            Err(outou_fmt::FormatError::RustfmtUnavailable { .. })
        ));
    }

    /// `textDocument/formatting` for a known, unformatted degraded-mode
    /// document (no crate root, so no [`crate::documents::Workspace`] at
    /// all) must be answered locally, with one full-document edit — never
    /// forwarded (there is nothing to forward to; `state.ra` is `None`
    /// here and the response still arrives).
    #[test]
    fn formatting_request_returns_a_full_document_edit_for_an_unformatted_document() {
        let mut state = State::new();
        let uri: lsp_types::Uri = "file:///app.rsx".parse().unwrap();
        let rsx_uri_string = uri::to_outou(&uri).as_str().to_string();
        state.degraded_docs.insert(
            rsx_uri_string,
            RsxDocument::new("fn f() { <div  /> }".to_string(), 1),
        );
        let (connection, client) = Connection::memory();

        dispatch_formatting_request(
            &state,
            &connection,
            Request {
                id: RequestId::from(1),
                method: "textDocument/formatting".to_string(),
                params: json!({
                    "textDocument": { "uri": "file:///app.rsx" },
                    "options": { "tabSize": 4, "insertSpaces": true },
                }),
            },
        );

        let message = client
            .receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("a response");
        let Message::Response(response) = message else {
            panic!("expected a response, got {message:?}");
        };
        let edits = response.response_result.expect("no error");
        let edits = edits.as_array().expect("an array of edits");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0]["newText"], json!("fn f() {\n    <div />\n}\n"));
    }

    /// Already-formatted input returns an empty edit list, not `null` —
    /// distinct from the "cannot format this" cases below, but equally a
    /// no-op for the editor.
    #[test]
    fn formatting_request_returns_no_edits_for_already_formatted_input() {
        let mut state = State::new();
        let uri: lsp_types::Uri = "file:///app.rsx".parse().unwrap();
        let rsx_uri_string = uri::to_outou(&uri).as_str().to_string();
        state.degraded_docs.insert(
            rsx_uri_string,
            RsxDocument::new("fn f() {\n    <div />\n}\n".to_string(), 1),
        );
        let (connection, client) = Connection::memory();

        dispatch_formatting_request(
            &state,
            &connection,
            Request {
                id: RequestId::from(1),
                method: "textDocument/formatting".to_string(),
                params: json!({ "textDocument": { "uri": "file:///app.rsx" } }),
            },
        );

        let Message::Response(response) = client
            .receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap()
        else {
            panic!("expected a response");
        };
        assert_eq!(response.response_result.unwrap(), json!([]));
    }

    /// A file with a syntax error is never edited: `null`, not an error
    /// response — an editor's format-on-save must not surface an error
    /// popup for a file that simply cannot be formatted right now.
    #[test]
    fn formatting_request_returns_null_for_a_file_with_a_syntax_error() {
        let mut state = State::new();
        let uri: lsp_types::Uri = "file:///broken.rsx".parse().unwrap();
        let rsx_uri_string = uri::to_outou(&uri).as_str().to_string();
        state.degraded_docs.insert(
            rsx_uri_string,
            RsxDocument::new("fn f() { <div cl".to_string(), 1),
        );
        let (connection, client) = Connection::memory();

        dispatch_formatting_request(
            &state,
            &connection,
            Request {
                id: RequestId::from(1),
                method: "textDocument/formatting".to_string(),
                params: json!({ "textDocument": { "uri": "file:///broken.rsx" } }),
            },
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

    /// An unknown document (never opened) is also `null`, never an error.
    #[test]
    fn formatting_request_returns_null_for_an_unknown_document() {
        let state = State::new();
        let (connection, client) = Connection::memory();

        dispatch_formatting_request(
            &state,
            &connection,
            Request {
                id: RequestId::from(1),
                method: "textDocument/formatting".to_string(),
                params: json!({ "textDocument": { "uri": "file:///never-opened.rsx" } }),
            },
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

                rsx_document: None,
            },
        );
        state.pending.insert(
            RequestId::from(2),
            Pending {
                client_id: RequestId::from(11),
                kind: PendingKind::Definition,
                epoch: 0,

                rsx_document: None,
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
