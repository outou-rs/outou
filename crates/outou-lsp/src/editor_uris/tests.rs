use lsp_server::{Request, RequestId, Response};
use serde_json::json;

use super::*;

const REAL: &str = "file:///real/app/src/main.rsx";
const LINK: &str = "file:///link/app/src/main.rsx";

/// An `EditorUris` that already remembers `REAL -> LINK`, as if the editor
/// had opened the document through a symbolic link.
fn linked() -> EditorUris {
    let uris = EditorUris::default();
    uris.table().insert(REAL.to_string(), LINK.to_string());
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

#[test]
fn diagnostics_for_a_rsx_file_go_back_to_the_editor_spelling() {
    let mut message = notification(
        "textDocument/publishDiagnostics",
        json!({ "uri": REAL, "diagnostics": [] }),
    );
    linked().restore_outgoing(&mut message);
    let Message::Notification(note) = message else {
        panic!()
    };
    assert_eq!(note.params["uri"], LINK);
}

#[test]
fn definition_and_references_locations_go_back_to_the_editor_spelling() {
    let range =
        json!({ "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } });
    let mut message = ok_response(json!([
        { "uri": REAL, "range": range },
        { "uri": "file:///elsewhere/lib.rs", "range": range },
    ]));
    linked().restore_outgoing(&mut message);
    let result = result_of(message);
    assert_eq!(result[0]["uri"], LINK);
    assert_eq!(result[1]["uri"], "file:///elsewhere/lib.rs");
}

#[test]
fn a_rename_workspace_edit_goes_back_to_the_editor_spelling() {
    let edit = json!([{ "range": {}, "newText": "x" }]);
    let mut by_changes = ok_response(json!({ "changes": { REAL: edit, "file:///o/a.rs": edit } }));
    let mut by_document_changes = ok_response(json!({
        "documentChanges": [{ "textDocument": { "uri": REAL, "version": 3 }, "edits": edit }]
    }));
    let uris = linked();
    uris.restore_outgoing(&mut by_changes);
    uris.restore_outgoing(&mut by_document_changes);

    let changes = result_of(by_changes);
    assert!(changes["changes"].get(LINK).is_some());
    assert!(changes["changes"].get(REAL).is_none());
    assert!(changes["changes"].get("file:///o/a.rs").is_some());
    let document_changes = result_of(by_document_changes);
    assert_eq!(
        document_changes["documentChanges"][0]["textDocument"]["uri"],
        LINK
    );
}

#[test]
fn with_nothing_remembered_every_message_is_unchanged() {
    let original = json!({ "uri": REAL, "diagnostics": [] });
    let mut message = notification("textDocument/publishDiagnostics", original.clone());
    EditorUris::default().restore_outgoing(&mut message);
    let Message::Notification(note) = message else {
        panic!()
    };
    assert_eq!(note.params, original);
}

#[test]
fn unrelated_notifications_are_not_rewritten() {
    let mut message = notification("window/logMessage", json!({ "uri": REAL }));
    linked().restore_outgoing(&mut message);
    let Message::Notification(note) = message else {
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

        let mut diagnostics = notification(
            "textDocument/publishDiagnostics",
            json!({ "uri": real_uri.as_str() }),
        );
        uris.restore_outgoing(&mut diagnostics);
        let Message::Notification(note) = diagnostics else {
            panic!()
        };
        assert_eq!(note.params["uri"], link_uri.as_str());
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

    let close = notification(
        "textDocument/didClose",
        json!({ "textDocument": { "uri": REAL } }),
    );
    sender
        .send(notification(
            "textDocument/publishDiagnostics",
            json!({ "uri": REAL, "diagnostics": [] }),
        ))
        .unwrap();
    sender
        .send(EditorUris::forget_after(&close).expect("a .rsx didClose is forgotten"))
        .unwrap();
    sender
        .send(notification(
            "textDocument/publishDiagnostics",
            json!({ "uri": REAL, "diagnostics": [] }),
        ))
        .unwrap();
    drop(sender);
    handle.join().unwrap();

    let uris_seen: Vec<String> = from_server
        .try_iter()
        .map(|message| match message {
            Message::Notification(note) => note.params["uri"].as_str().unwrap().to_string(),
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    // The forget marker is consumed; the diagnostics before it are
    // restored, the one after it is not.
    assert_eq!(uris_seen, vec![LINK.to_string(), REAL.to_string()]);
    assert_eq!(uris.alias_count(), 0);
}
