//! Dispatch-level tests for work-done progress token handling, the
//! forwarded `partialResultToken`, and `experimental/serverStatus` notices
//! (issue #21 review). rust-analyzer is a shell script that logs its stdin,
//! so what actually reaches it can be asserted.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use serde_json::{json, Value};

use super::notifications::dispatch_client_notification;
use super::requests::dispatch_client_request;
use super::responses::{dispatch_client_response, dispatch_ra_message};
use super::State;
use crate::ra::RaClient;

const MOCK_WAIT: Duration = Duration::from_secs(3);

/// A stand-in rust-analyzer that appends everything it is sent to a log.
fn mock_ra(name: &str) -> (RaClient, PathBuf) {
    let dir = std::env::temp_dir().join(format!("outou-mock-ra-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("stdin.log");
    let _ = std::fs::remove_file(&log);
    let script = dir.join("mock-ra.sh");
    std::fs::write(
        &script,
        format!("#!/bin/sh\nexec cat > '{}'\n", log.display()),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    (
        RaClient::spawn(script.to_str().unwrap(), &dir).unwrap(),
        log,
    )
}

/// Every message the mock received, once `needle` shows up in its log.
fn ra_received(log: &PathBuf, needle: &str) -> Vec<Value> {
    let deadline = Instant::now() + MOCK_WAIT;
    loop {
        let text = std::fs::read_to_string(log).unwrap_or_default();
        if text.contains(needle) || Instant::now() > deadline {
            return text
                .split("Content-Length:")
                .filter_map(|chunk| chunk.split_once("\r\n\r\n"))
                .filter_map(|(_, body)| serde_json::from_str(body.trim()).ok())
                .collect();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn drain(client: &Connection) -> Vec<Message> {
    client.receiver.try_iter().collect()
}

fn progress(token: &str, value: Value) -> Message {
    Message::Notification(Notification::new(
        "$/progress".to_string(),
        json!({ "token": token, "value": value }),
    ))
}

fn forwarded_progress(client: &Connection) -> Vec<Value> {
    drain(client)
        .into_iter()
        .filter_map(|m| match m {
            Message::Notification(n) if n.method == "$/progress" => Some(n.params),
            _ => None,
        })
        .collect()
}

/// Has rust-analyzer create `token` and returns the token the editor saw.
fn create_token(
    state: &mut State,
    connection: &Connection,
    client: &Connection,
    id: i32,
    token: &str,
) -> Value {
    dispatch_ra_message(
        state,
        connection,
        Message::Request(Request {
            id: RequestId::from(id),
            method: "window/workDoneProgress/create".to_string(),
            params: json!({ "token": token }),
        }),
    );
    match drain(client).into_iter().next() {
        Some(Message::Request(r)) => r.params["token"].clone(),
        other => panic!("create was not forwarded: {other:?}"),
    }
}

#[test]
fn partial_result_token_is_not_forwarded_to_rust_analyzer() {
    let (ra, log) = mock_ra("partial");
    let mut state = State::new();
    state.ra = Some(ra);
    let (connection, _client) = Connection::memory();
    dispatch_client_request(
        &mut state,
        &connection,
        Request {
            id: RequestId::from(7),
            method: "workspace/symbol".to_string(),
            params: json!({
                "query": "App",
                "partialResultToken": "rustAnalyzer/results",
                "workDoneToken": "client-wd"
            }),
        },
    );
    let got = ra_received(&log, "workspace/symbol");
    let req = got
        .iter()
        .find(|m| m["method"] == "workspace/symbol")
        .expect("forwarded");
    assert!(req["params"].get("partialResultToken").is_none(), "{req}");
    assert_eq!(req["params"]["workDoneToken"], "client-wd");
}

#[test]
fn progress_for_an_editor_allocated_token_passes_untouched() {
    for supports_work_done in [true, false] {
        let mut state = State::new();
        state.client_supports_work_done_progress = supports_work_done;
        let (connection, client) = Connection::memory();
        let value = json!([{ "name": "App", "kind": 12 }]);
        dispatch_ra_message(
            &mut state,
            &connection,
            progress("rustAnalyzer/results", value.clone()),
        );
        let out = forwarded_progress(&client);
        assert_eq!(
            out.len(),
            1,
            "dropped (work-done support: {supports_work_done})"
        );
        assert_eq!(out[0]["token"], "rustAnalyzer/results");
        assert_eq!(out[0]["value"], value);
    }
}

#[test]
fn work_done_progress_cancel_reaches_rust_analyzer_under_its_own_token() {
    let (ra, log) = mock_ra("cancel");
    let mut state = State::new();
    state.client_supports_work_done_progress = true;
    state.ra = Some(ra);
    let (connection, client) = Connection::memory();
    let editor_token = create_token(
        &mut state,
        &connection,
        &client,
        41,
        "rust-analyzer/flycheck/0",
    );
    dispatch_client_response(
        &mut state,
        Response::new_ok(RequestId::from(1_000_000_000), Value::Null),
    );
    dispatch_client_notification(
        &mut state,
        &connection,
        Notification::new(
            "window/workDoneProgress/cancel".to_string(),
            json!({ "token": editor_token }),
        ),
    );
    dispatch_client_notification(
        &mut state,
        &connection,
        Notification::new(
            "window/workDoneProgress/cancel".to_string(),
            json!({ "token": "client-wd" }),
        ),
    );
    let got = ra_received(&log, "client-wd");
    let tokens: Vec<&Value> = got
        .iter()
        .filter(|m| m["method"] == "window/workDoneProgress/cancel")
        .map(|m| &m["params"]["token"])
        .collect();
    assert_eq!(
        tokens,
        [&json!("rust-analyzer/flycheck/0"), &json!("client-wd")]
    );
}

#[test]
fn distinct_upstream_tokens_stay_distinct_and_backend_free() {
    let mut state = State::new();
    state.client_supports_work_done_progress = true;
    let (connection, client) = Connection::memory();
    let real = [
        "rustAnalyzer/Fetching",
        "rustAnalyzer/Loading",
        "rustAnalyzer/Roots Scanned",
        "rustAnalyzer/Indexing",
        "rustAnalyzer/cachePriming",
        "rustAnalyzer/Building CrateGraph",
        "rust-analyzer/flycheck/0",
        "rust-analyzer/flycheck/1",
        "outou/Indexing",
    ];
    let mut seen = std::collections::HashSet::new();
    for (i, token) in real.iter().enumerate() {
        let out = create_token(&mut state, &connection, &client, i as i32 + 10, token);
        let text = out.as_str().unwrap().to_lowercase();
        assert!(
            !text.contains("rust-analyzer") && !text.contains("rustanalyzer"),
            "{out}"
        );
        assert!(seen.insert(out.clone()), "collision on {token}: {out}");
    }
}

#[test]
fn malformed_server_status_does_not_rearm_the_notice() {
    let status = |params: Value| {
        Message::Notification(Notification::new(
            "experimental/serverStatus".to_string(),
            params,
        ))
    };
    let shown = |client: &Connection| {
        drain(client)
            .iter()
            .filter(|m| matches!(m, Message::Notification(n) if n.method == "window/showMessage"))
            .count()
    };
    for bad in [
        json!({}),
        json!({ "health": null }),
        json!({ "health": "degraded" }),
        json!({ "health": 3 }),
    ] {
        let mut state = State::new();
        let (connection, client) = Connection::memory();
        let error = json!({ "health": "error", "quiescent": true });
        dispatch_ra_message(&mut state, &connection, status(error.clone()));
        dispatch_ra_message(&mut state, &connection, status(bad.clone()));
        dispatch_ra_message(&mut state, &connection, status(error));
        assert_eq!(shown(&client), 1, "re-notified after {bad}");
    }
}
