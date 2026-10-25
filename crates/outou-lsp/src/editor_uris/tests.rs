use lsp_server::{Request, RequestId, Response};
use serde_json::json;

use super::*;

const REAL: &str = "file:///real/app/src/main.rsx";
const LINK: &str = "file:///link/app/src/main.rsx";

/// An `EditorUris` that already remembers `REAL -> LINK`, as if the editor
/// had opened the document through a symbolic link.
fn linked() -> EditorUris {
    let uris = EditorUris::default();
    uris.register(REAL, LINK);
    uris
}

fn notification(method: &str, params: Value) -> Message {
    Message::Notification(Notification {
        method: method.to_string(),
        params,
    })
}

fn ok_response(result: Value) -> Message {
    Message::Response(Response::new_ok(RequestId::from(1), result))
}

fn result_of(message: Message) -> Value {
    match message {
        Message::Response(response) => response.response_result.expect("ok response"),
        other => panic!("not a response: {other:?}"),
    }
}

/// Runs one message through the outbound side and returns what the editor
/// would receive.
fn out(uris: &EditorUris, message: Message) -> Vec<Message> {
    uris.restore_outgoing(message)
}

fn only(mut messages: Vec<Message>) -> Message {
    assert_eq!(messages.len(), 1, "{messages:?}");
    messages.remove(0)
}

fn diagnostics_to(uri: &str, count: usize) -> Message {
    let list: Vec<Value> = (0..count)
        .map(|i| json!({ "message": format!("e{i}") }))
        .collect();
    notification(
        "textDocument/publishDiagnostics",
        json!({ "uri": uri, "diagnostics": list }),
    )
}

/// `(uri, number of diagnostics)` of every publishDiagnostics in `messages`.
fn published(messages: &[Message]) -> Vec<(String, usize)> {
    messages
        .iter()
        .map(|message| match message {
            Message::Notification(note) => (
                note.params["uri"].as_str().unwrap().to_string(),
                note.params["diagnostics"].as_array().unwrap().len(),
            ),
            other => panic!("not a notification: {other:?}"),
        })
        .collect()
}

#[test]
fn diagnostics_for_a_rsx_file_go_back_to_the_editor_spelling() {
    let sent = out(&linked(), diagnostics_to(REAL, 0));
    assert_eq!(published(&sent), vec![(LINK.to_string(), 0)]);
}

#[test]
fn definition_and_references_locations_go_back_to_the_editor_spelling() {
    let range =
        json!({ "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } });
    let message = ok_response(json!([
        { "uri": REAL, "range": range },
        { "uri": "file:///elsewhere/lib.rs", "range": range },
    ]));
    let result = result_of(only(out(&linked(), message)));
    assert_eq!(result[0]["uri"], LINK);
    assert_eq!(result[1]["uri"], "file:///elsewhere/lib.rs");
}

#[test]
fn a_rename_workspace_edit_goes_back_to_the_editor_spelling() {
    let edit = json!([{ "range": {}, "newText": "x" }]);
    let by_changes = ok_response(json!({ "changes": { REAL: edit, "file:///o/a.rs": edit } }));
    let by_document_changes = ok_response(json!({
        "documentChanges": [{ "textDocument": { "uri": REAL, "version": 3 }, "edits": edit }]
    }));
    let uris = linked();

    let changes = result_of(only(out(&uris, by_changes)));
    assert!(changes["changes"].get(LINK).is_some());
    assert!(changes["changes"].get(REAL).is_none());
    assert!(changes["changes"].get("file:///o/a.rs").is_some());
    let document_changes = result_of(only(out(&uris, by_document_changes)));
    assert_eq!(
        document_changes["documentChanges"][0]["textDocument"]["uri"],
        LINK
    );
}

#[test]
fn with_nothing_remembered_every_message_is_unchanged() {
    let original = json!({ "uri": REAL, "diagnostics": [] });
    let message = notification("textDocument/publishDiagnostics", original.clone());
    let Message::Notification(note) = only(out(&EditorUris::default(), message)) else {
        panic!()
    };
    assert_eq!(note.params, original);
}

#[test]
fn unrelated_notifications_are_not_rewritten() {
    let message = notification("window/logMessage", json!({ "uri": REAL }));
    let Message::Notification(note) = only(out(&linked(), message)) else {
        panic!()
    };
    assert_eq!(note.params["uri"], REAL);
}

#[test]
fn incoming_requests_and_notifications_are_normalized_and_remembered() {
    let dir = std::env::temp_dir().join(format!("outou-lsp-editor-uris-{}", std::process::id()));
    let real_dir = dir.join("real");
    std::fs::create_dir_all(&real_dir).unwrap();
    std::fs::write(real_dir.join("a.rsx"), "").unwrap();
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(dir.join("link"));
        std::os::unix::fs::symlink(&real_dir, dir.join("link")).unwrap();
        let link_uri = outou_sourcemap::file_uri(&dir.join("link/a.rsx"));
        let real_uri = outou_sourcemap::file_uri(&real_dir.canonicalize().unwrap().join("a.rsx"));

        let uris = EditorUris::default();
        let mut request = Message::Request(Request {
            id: RequestId::from(7),
            method: "textDocument/completion".to_string(),
            params: json!({ "textDocument": { "uri": link_uri.as_str() }, "position": {} }),
        });
        uris.normalize_incoming(&mut request);
        let Message::Request(request) = request else {
            panic!()
        };
        assert_eq!(request.params["textDocument"]["uri"], real_uri.as_str());
        assert_eq!(uris.alias_count(), 1);

        let sent = out(&uris, diagnostics_to(real_uri.as_str(), 0));
        assert_eq!(published(&sent), vec![(link_uri.as_str().to_string(), 0)]);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_real_path_editor_leaves_no_alias_and_non_rsx_documents_are_untouched() {
    let uris = EditorUris::default();
    let mut rs = notification(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": "file:///x/../x/lib.rs" } }),
    );
    uris.normalize_incoming(&mut rs);
    let Message::Notification(note) = rs else {
        panic!()
    };
    assert_eq!(note.params["textDocument"]["uri"], "file:///x/../x/lib.rs");
    assert_eq!(uris.alias_count(), 0);
}

#[test]
fn the_outbound_thread_restores_in_order_and_forgets_after_didclose() {
    let uris = linked();
    let (to_editor, from_server) = crossbeam_channel::unbounded();
    let (sender, handle) = spawn_outbound(to_editor, uris.clone());

    let close = uris.note_close(REAL, LINK).forget.expect("a marker");
    sender.send(diagnostics_to(REAL, 0)).unwrap();
    sender.send(close).unwrap();
    sender.send(diagnostics_to(REAL, 0)).unwrap();
    drop(sender);
    handle.join().unwrap();

    let sent: Vec<Message> = from_server.try_iter().collect();
    // The forget marker is consumed; the diagnostics before it are
    // restored, the one after it is not.
    assert_eq!(
        published(&sent),
        vec![(LINK.to_string(), 0), (REAL.to_string(), 0)]
    );
    assert_eq!(uris.alias_count(), 0);
}

// -- diagnostics when the spelling in use changes (review of 875e766, 1) --

#[test]
fn registering_an_alias_clears_the_real_path_diagnostics_once() {
    // A file nobody has opened gets its diagnostics at the real path; the
    // editor then opens it through the link.
    let uris = EditorUris::default();
    let first = out(&uris, diagnostics_to(REAL, 1));
    assert_eq!(published(&first), vec![(REAL.to_string(), 1)]);

    uris.register(REAL, LINK);
    let fixed = out(&uris, diagnostics_to(REAL, 0));
    let mut seen = published(&fixed);
    seen.sort();
    assert_eq!(seen, vec![(LINK.to_string(), 0), (REAL.to_string(), 0)]);

    // Only once: the real path is clean now.
    let again = out(&uris, diagnostics_to(REAL, 0));
    assert_eq!(published(&again), vec![(LINK.to_string(), 0)]);
}

#[test]
fn a_clean_real_path_is_not_cleared_again_and_a_nonempty_one_moves_to_the_link() {
    let uris = EditorUris::default();
    out(&uris, diagnostics_to(REAL, 0)); // nothing wrong was ever shown
    uris.register(REAL, LINK);
    let sent = out(&uris, diagnostics_to(REAL, 2));
    assert_eq!(published(&sent), vec![(LINK.to_string(), 2)]);
}

#[test]
fn closing_the_alias_clears_the_link_and_hands_the_diagnostics_back_to_the_real_path() {
    let uris = linked();
    assert_eq!(
        published(&out(&uris, diagnostics_to(REAL, 1))),
        vec![(LINK.to_string(), 1)]
    );
    // `didClose` reloads the file from disk and publishes before the
    // marker is processed.
    let close = uris.note_close(REAL, LINK);
    assert!(!close.swallow);
    assert_eq!(
        published(&out(&uris, diagnostics_to(REAL, 1))),
        vec![(LINK.to_string(), 1)]
    );
    let forgotten = out(&uris, close.forget.expect("a marker"));
    let mut seen = published(&forgotten);
    seen.sort();
    assert_eq!(seen, vec![(LINK.to_string(), 0), (REAL.to_string(), 1)]);
    assert_eq!(uris.alias_count(), 0);
}

#[test]
fn closing_an_alias_with_nothing_shown_sends_nothing() {
    let uris = linked();
    out(&uris, diagnostics_to(REAL, 0));
    let close = uris.note_close(REAL, LINK);
    assert!(out(&uris, close.forget.unwrap()).is_empty());
}

// -- one document open under several spellings (review of 875e766, 2) --

#[test]
fn diagnostics_go_to_every_spelling_the_document_is_open_under() {
    let uris = EditorUris::default();
    uris.register(REAL, REAL);
    uris.register(REAL, LINK);
    let sent = out(&uris, diagnostics_to(REAL, 1));
    let mut seen = published(&sent);
    seen.sort();
    assert_eq!(seen, vec![(LINK.to_string(), 1), (REAL.to_string(), 1)]);
}

#[test]
fn the_document_is_closed_only_when_its_last_spelling_closes() {
    let uris = EditorUris::default();
    uris.register(REAL, REAL);
    uris.register(REAL, LINK);

    let first = uris.note_close(REAL, LINK);
    assert!(first.swallow, "the real-path tab is still open");
    let second = uris.note_close(REAL, REAL);
    assert!(!second.swallow, "the last tab closed");
    let third = uris.note_close(REAL, REAL);
    assert!(!third.swallow, "a close with nothing open is not swallowed");
}

#[test]
fn closing_one_spelling_clears_only_that_spelling() {
    let uris = EditorUris::default();
    uris.register(REAL, REAL);
    uris.register(REAL, LINK);
    out(&uris, diagnostics_to(REAL, 1));

    let close = uris.note_close(REAL, LINK);
    let forgotten = out(&uris, close.forget.unwrap());
    assert_eq!(published(&forgotten), vec![(LINK.to_string(), 0)]);
    let later = out(&uris, diagnostics_to(REAL, 1));
    assert_eq!(published(&later), vec![(REAL.to_string(), 1)]);
}

#[test]
fn a_response_uses_the_spelling_of_the_request_it_answers() {
    let uris = EditorUris::default();
    uris.register(REAL, REAL);
    uris.register(REAL, LINK); // the most recent spelling
    uris.remember_request(RequestId::from(1), REAL, REAL);
    uris.remember_request(RequestId::from(2), REAL, LINK);

    let by_real = Message::Response(Response::new_ok(RequestId::from(1), json!({ "uri": REAL })));
    let by_link = Message::Response(Response::new_ok(RequestId::from(2), json!({ "uri": REAL })));
    let other = Message::Response(Response::new_ok(RequestId::from(3), json!({ "uri": REAL })));
    assert_eq!(result_of(only(out(&uris, by_real)))["uri"], REAL);
    assert_eq!(result_of(only(out(&uris, by_link)))["uri"], LINK);
    // Not answering an `.rsx` request: the most recently used spelling.
    assert_eq!(result_of(only(out(&uris, other)))["uri"], LINK);
}
