//! Dispatch-level tests for keyword-named props (`type` on
//! `<Field type="text" />`, lowered to `r#type`): ledger rows 17, 29 and 61,
//! ADR 0013 amendment (#31).
//!
//! Each test builds a workspace from the real backend's output (so the
//! `r#` + name attribute mapping is the one codegen really emits), sends
//! `prepareRename`, `rename` or `references` through
//! [`dispatch_client_request`] to a mock rust-analyzer (which logs what it
//! is sent), then feeds back a rust-analyzer-shaped answer through
//! [`dispatch_ra_message`] and checks what the editor receives.

use std::path::Path;

use lsp_server::{Connection, Message, Request, RequestId, Response};
use outou_codegen::{Backend, GenerateOptions, Mode};
use outou_sourcemap::{file_uri, Position, PositionRange, Registry, Span};
use serde_json::{json, Value};

use super::progress_tests::{mock_ra, ra_received};
use super::requests::dispatch_client_request;
use super::responses::dispatch_ra_message;
use super::State;
use crate::documents::{test_generated_unit, RsxDocument, Workspace};
use crate::uri;

const REQUEST_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// A component with a keyword-named prop, declared and used in the same
/// file: the declaration is verbatim Rust (`r#type`), each call site is an
/// attribute (`type`, lowered to `r#type`).
const PROP_DECLARED_AS_TYPE: &str = "#[component]\nfn Field(r#type: String) -> Element {\n    let _ = &r#type;\n    <input />\n}\nfn App() -> Element {\n    <div><Field type=\"text\" /><Field type=\"b\" /></div>\n}\n";

/// The same shape with the prop named `kind`, for renaming it to `type`.
const PROP_DECLARED_AS_KIND: &str = "#[component]\nfn Field(kind: String) -> Element {\n    let _ = &kind;\n    <input />\n}\nfn App() -> Element {\n    <div><Field kind=\"text\" /><Field kind=\"b\" /></div>\n}\n";

pub(super) struct Fixture {
    pub(super) state: State,
    pub(super) rsx_uri: lsp_types::Uri,
    generated_uri: lsp_types::Uri,
    pub(super) source: &'static str,
    generated: String,
    pub(super) log: std::path::PathBuf,
}

/// `name` keeps each test's mock rust-analyzer in its own temp dir.
pub(super) fn fixture(name: &str, source: &'static str) -> Fixture {
    let rsx_uri = file_uri(Path::new("/app/src/main.rsx"));
    let generated_uri = file_uri(Path::new("/app/src/.generated/main.rs"));
    let parsed = crate::documents::parse_rsx(source);
    let generated = outou_backend_dioxus::DioxusBackend
        .generate(
            &parsed,
            source,
            Mode::Strict,
            &GenerateOptions::new(generated_uri.clone(), rsx_uri.clone()),
        )
        .expect("generation succeeds");
    let mut workspace = Workspace {
        manifest_dir: std::path::PathBuf::from("/app"),
        plan: None,
        registry: Registry::new().with_map(generated.source_map.clone()),
        rsx: std::collections::HashMap::new(),
        generated: std::collections::HashMap::new(),
        rsx_to_generated: std::collections::HashMap::new(),
        plan_error: None,
    };
    workspace.rsx.insert(
        rsx_uri.as_str().to_string(),
        RsxDocument::new(source.to_string(), 1),
    );
    workspace.generated.insert(
        generated_uri.as_str().to_string(),
        test_generated_unit(&generated_uri, &rsx_uri, &generated.rust),
    );
    workspace.rsx_to_generated.insert(
        rsx_uri.as_str().to_string(),
        generated_uri.as_str().to_string(),
    );
    let (ra, log) = mock_ra(name);
    let mut state = State::new();
    state.workspace = Some(workspace);
    state.ra = Some(ra);
    Fixture {
        state,
        rsx_uri: uri::to_lsp(&rsx_uri),
        generated_uri: uri::to_lsp(&generated_uri),
        source,
        generated: generated.rust,
        log,
    }
}

impl Fixture {
    pub(super) fn rsx_position(&self, offset: usize) -> Value {
        let doc =
            &self.state.workspace.as_ref().unwrap().rsx[uri::to_outou(&self.rsx_uri).as_str()];
        let position = doc.line_index.offset_to_position(offset as u32);
        json!({ "line": position.line, "character": position.character })
    }

    /// The `.rsx` offset one byte into the first attribute spelled `word`
    /// after `<Field `.
    fn call_site_offset(&self, word: &str) -> usize {
        self.source.find(&format!("<Field {word}")).unwrap() + "<Field ".len() + 1
    }

    fn generated_range(&self, start: usize, len: usize) -> Value {
        let unit = &self.state.workspace.as_ref().unwrap().generated
            [uri::to_outou(&self.generated_uri).as_str()];
        let range: PositionRange = unit
            .line_index
            .span_to_range(Span::new(start as u32, (start + len) as u32));
        json!({
            "start": { "line": range.start.line, "character": range.start.character },
            "end": { "line": range.end.line, "character": range.end.character },
        })
    }

    /// Every occurrence of `needle` in the generated text, as LSP ranges
    /// (`skip` bytes in, so `2` leaves out the `r#` of a raw identifier).
    fn generated_occurrences(&self, needle: &str, skip: usize) -> Vec<Value> {
        self.generated
            .match_indices(needle)
            .map(|(start, _)| self.generated_range(start + skip, needle.len() - skip))
            .collect()
    }

    /// Sends `method` at `.rsx` `offset` and returns the id the request got
    /// on its way to rust-analyzer, plus the request the mock received.
    fn send(
        &mut self,
        connection: &Connection,
        method: &str,
        offset: usize,
        extra: Value,
    ) -> (RequestId, Value) {
        let mut params = json!({
            "textDocument": { "uri": self.rsx_uri.as_str() },
            "position": self.rsx_position(offset),
        });
        for (key, value) in extra.as_object().unwrap() {
            params[key] = value.clone();
        }
        dispatch_client_request(
            &mut self.state,
            connection,
            Request {
                id: RequestId::from(9),
                method: method.to_string(),
                params,
            },
        );
        let forwarded = ra_received(&self.log, method)
            .into_iter()
            .find(|m| m["method"] == method)
            .unwrap_or_else(|| panic!("{method} was not forwarded"));
        let ra_id = self
            .state
            .pending
            .keys()
            .next()
            .expect("a pending request")
            .clone();
        (ra_id, forwarded)
    }

    /// The generated byte offset a forwarded request's position names.
    fn forwarded_offset(&self, forwarded: &Value) -> usize {
        let position: lsp_types::Position =
            serde_json::from_value(forwarded["params"]["position"].clone()).unwrap();
        assert_eq!(
            forwarded["params"]["textDocument"]["uri"],
            self.generated_uri.as_str()
        );
        let unit = &self.state.workspace.as_ref().unwrap().generated
            [uri::to_outou(&self.generated_uri).as_str()];
        unit.line_index
            .position_to_offset(Position::new(position.line, position.character)) as usize
    }
}

pub(super) fn answer(
    fixture: &mut Fixture,
    connection: &Connection,
    client: &Connection,
    ra_id: RequestId,
    result: Value,
) -> Response {
    dispatch_ra_message(
        &mut fixture.state,
        connection,
        Message::Response(Response::new_ok(ra_id, result)),
    );
    let Message::Response(response) = client.receiver.recv_timeout(REQUEST_WAIT).unwrap() else {
        panic!("expected a response");
    };
    response
}

/// The `.rsx` text a range of `edits`/`locations` covers.
pub(super) fn text_at<'a>(source: &'a str, range: &Value) -> &'a str {
    let range: lsp_types::Range = serde_json::from_value(range.clone()).unwrap();
    let index = outou_sourcemap::LineIndex::new(source);
    let span = index.range_to_span(PositionRange::new(
        Position::new(range.start.line, range.start.character),
        Position::new(range.end.line, range.end.character),
    ));
    &source[span.start as usize..span.end as usize]
}

#[test]
fn prepare_rename_on_a_keyword_attribute_asks_rust_analyzer_past_the_raw_prefix() {
    let mut f = fixture("kw-prepare", PROP_DECLARED_AS_TYPE);
    let (connection, client) = Connection::memory();
    let offset = f.call_site_offset("type");
    let (ra_id, forwarded) = f.send(&connection, "textDocument/prepareRename", offset, json!({}));

    // One byte into `type` is three bytes into a generated `r#type`.
    let generated_offset = f.forwarded_offset(&forwarded);
    assert_eq!(
        &f.generated[generated_offset - 3..generated_offset + 3],
        "r#type"
    );

    // rust-analyzer answers with the whole raw identifier: the editor is
    // told the plain `type` is what gets renamed.
    let whole = f.generated_occurrences("r#type", 0)[2].clone();
    let response = answer(&mut f, &connection, &client, ra_id, whole);
    let range = response.response_result.unwrap();
    assert_eq!(text_at(f.source, &range), "type");
    assert_eq!(
        range["start"]["character"].as_u64().unwrap() as usize,
        f.source.lines().nth(6).unwrap().find("type").unwrap()
    );
}

#[test]
fn prepare_rename_with_a_prefixless_range_and_placeholder_maps_the_range() {
    let mut f = fixture("kw-prepare-name", PROP_DECLARED_AS_TYPE);
    let (connection, client) = Connection::memory();
    let offset = f.call_site_offset("type");
    let (ra_id, _) = f.send(&connection, "textDocument/prepareRename", offset, json!({}));
    let name_only = f.generated_occurrences("r#type", 2)[2].clone();
    let response = answer(
        &mut f,
        &connection,
        &client,
        ra_id,
        json!({ "range": name_only, "placeholder": "type" }),
    );
    let result = response.response_result.unwrap();
    assert_eq!(text_at(f.source, &result["range"]), "type");
    assert_eq!(result["placeholder"], "type");
}

#[test]
fn rename_to_another_name_edits_the_plain_attribute_and_the_raw_declaration() {
    let mut f = fixture("kw-rename-to-kind", PROP_DECLARED_AS_TYPE);
    let (connection, client) = Connection::memory();
    let offset = f.call_site_offset("type");
    let (ra_id, _) = f.send(
        &connection,
        "textDocument/rename",
        offset,
        json!({ "newName": "kind" }),
    );
    let edits: Vec<Value> = f
        .generated_occurrences("r#type", 0)
        .into_iter()
        .map(|range| json!({ "range": range, "newText": "kind" }))
        .collect();
    assert_eq!(edits.len(), 4, "declaration, use, two call sites");
    let changes = json!({ "changes": { f.generated_uri.as_str(): edits } });
    let response = answer(&mut f, &connection, &client, ra_id, changes);
    let edit = response.response_result.expect("the rename is applied");
    let rsx_edits = edit["changes"][f.rsx_uri.as_str()].as_array().unwrap();
    assert_eq!(rsx_edits.len(), 4, "{rsx_edits:#?}");
    // Both call sites replace the plain `type` (4 bytes) with `kind`.
    let plain: Vec<_> = rsx_edits
        .iter()
        .filter(|e| text_at(f.source, &e["range"]) == "type")
        .collect();
    assert_eq!(plain.len(), 2, "{rsx_edits:#?}");
    for edit in rsx_edits {
        assert_eq!(edit["newText"], "kind");
    }
    // The declaration and its use are the verbatim `r#type`, replaced whole.
    let raw = rsx_edits
        .iter()
        .filter(|e| text_at(f.source, &e["range"]) == "r#type")
        .count();
    assert_eq!(raw, 2, "{rsx_edits:#?}");
}

#[test]
fn rename_to_a_keyword_writes_the_plain_name_in_attributes_and_raw_in_rust() {
    let mut f = fixture("kw-rename-to-type", PROP_DECLARED_AS_KIND);
    let (connection, client) = Connection::memory();
    let offset = f.call_site_offset("kind");
    let (ra_id, _) = f.send(
        &connection,
        "textDocument/rename",
        offset,
        json!({ "newName": "type" }),
    );
    // rust-analyzer's own spelling for a keyword is the raw identifier.
    let edits: Vec<Value> = f
        .generated_occurrences("kind", 0)
        .into_iter()
        .map(|range| json!({ "range": range, "newText": "r#type" }))
        .collect();
    let changes = json!({ "changes": {
        f.generated_uri.as_str(): edits,
        "file:///app/src/other.rs": [
            { "range": { "start": {"line":0,"character":0}, "end": {"line":0,"character":4} }, "newText": "r#type" }
        ],
    } });
    let response = answer(&mut f, &connection, &client, ra_id, changes);
    let edit = response.response_result.expect("the rename is applied");
    let rsx_edits = edit["changes"][f.rsx_uri.as_str()].as_array().unwrap();
    assert_eq!(rsx_edits.len(), 4, "{rsx_edits:#?}");
    let line_of = |e: &Value| e["range"]["start"]["line"].as_u64().unwrap();
    for edit in rsx_edits {
        // Lines 1 and 2 are the declaration and its use (Rust: `r#type`);
        // line 6 holds the call-site attributes (`.rsx`: plain `type`).
        let expected = if line_of(edit) == 6 { "type" } else { "r#type" };
        assert_eq!(edit["newText"], expected, "{rsx_edits:#?}");
    }
    assert_eq!(
        rsx_edits.iter().filter(|e| line_of(e) == 6).count(),
        2,
        "{rsx_edits:#?}"
    );
    // A plain `.rs` file keeps rust-analyzer's text untouched.
    assert_eq!(
        edit["changes"]["file:///app/src/other.rs"][0]["newText"],
        "r#type"
    );
}

#[test]
fn references_from_a_keyword_attribute_include_every_occurrence() {
    let mut f = fixture("kw-references", PROP_DECLARED_AS_TYPE);
    let (connection, client) = Connection::memory();
    let offset = f.call_site_offset("type");
    let (ra_id, _) = f.send(
        &connection,
        "textDocument/references",
        offset,
        json!({ "context": { "includeDeclaration": true } }),
    );
    // rust-analyzer reports some occurrences with the prefix, some without,
    // and one in a plain `.rs` file.
    let with_prefix = f.generated_occurrences("r#type", 0);
    let without_prefix = f.generated_occurrences("r#type", 2);
    let locations = json!([
        { "uri": f.generated_uri.as_str(), "range": with_prefix[0] },
        { "uri": f.generated_uri.as_str(), "range": without_prefix[1] },
        { "uri": f.generated_uri.as_str(), "range": with_prefix[2] },
        { "uri": f.generated_uri.as_str(), "range": without_prefix[3] },
        { "uri": "file:///app/src/other.rs", "range": { "start": {"line":3,"character":1}, "end": {"line":3,"character":7} } },
    ]);
    let response = answer(&mut f, &connection, &client, ra_id, locations);
    let items = response.response_result.unwrap();
    let items = items.as_array().unwrap();
    assert_eq!(items.len(), 5, "{items:#?}");
    let rsx: Vec<&str> = items
        .iter()
        .filter(|i| i["uri"] == f.rsx_uri.as_str())
        .map(|i| text_at(f.source, &i["range"]))
        .collect();
    // Declaration and use are verbatim `r#type` / its name part; the two
    // call sites are the plain attribute name.
    assert_eq!(rsx, ["r#type", "type", "type", "type"]);
    assert!(items.iter().any(|i| i["uri"] == "file:///app/src/other.rs"));
}

#[test]
fn a_position_in_a_hyphenated_attribute_name_is_still_refused() {
    // `data-id` lowers to the string key `"data-id"` (9 bytes for 7): a
    // different transformation, not the `r#` prefix, so it stays unmapped.
    let source = "fn f() -> Element {\n    <div data-id=\"x\" />\n}\n";
    let mut f = fixture("kw-hyphen", source);
    let (connection, client) = Connection::memory();
    let offset = source.find("data-id").unwrap() + 2;
    let params = json!({
        "textDocument": { "uri": f.rsx_uri.as_str() },
        "position": f.rsx_position(offset),
    });
    dispatch_client_request(
        &mut f.state,
        &connection,
        Request {
            id: RequestId::from(3),
            method: "textDocument/prepareRename".to_string(),
            params,
        },
    );
    let Message::Response(response) = client.receiver.recv_timeout(REQUEST_WAIT).unwrap() else {
        panic!("expected a response");
    };
    assert_eq!(response.response_result.unwrap(), Value::Null);
    assert!(f.state.pending.is_empty());
}

/// Sends `textDocument/completion` at `.rsx` `offset`; returns the labels
/// answered and whether anything was forwarded to rust-analyzer.
fn complete_at(f: &mut Fixture, name_offset: usize) -> (Vec<String>, Value, bool) {
    let (connection, client) = Connection::memory();
    let params = json!({
        "textDocument": { "uri": f.rsx_uri.as_str() },
        "position": f.rsx_position(name_offset),
    });
    dispatch_client_request(
        &mut f.state,
        &connection,
        Request {
            id: RequestId::from(4),
            method: "textDocument/completion".to_string(),
            params,
        },
    );
    let Message::Response(response) = client.receiver.recv_timeout(REQUEST_WAIT).unwrap() else {
        panic!("expected a response");
    };
    let result = response.response_result.unwrap();
    let labels = result["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["label"].as_str().unwrap().to_string())
        .collect();
    (labels, result, !f.state.pending.is_empty())
}

#[test]
fn completion_at_a_valued_component_prop_name_is_local_at_the_end_and_in_the_middle() {
    let mut f = fixture("kw-complete-component", PROP_DECLARED_AS_TYPE);
    let start = f.source.find("<Field type").unwrap() + "<Field ".len();
    for offset in [start + 4, start + 2] {
        let (labels, result, forwarded) = complete_at(&mut f, offset);
        assert!(!forwarded, "must not be forwarded to rust-analyzer");
        assert_eq!(labels, ["type"], "offset {offset}");
        // The edit replaces `type` alone, never `="text"`.
        let range = &result["items"][0]["textEdit"]["range"];
        let line = f.source.lines().nth(6).unwrap();
        let column = line.find("type").unwrap() as u64;
        assert_eq!(range["start"]["character"], column);
        assert_eq!(range["end"]["character"], column + 4);
    }
}

#[test]
fn completion_at_a_valued_element_attribute_name_is_local_at_the_end_and_in_the_middle() {
    let source = "fn f() -> Element {\n    <input id=\"x\" type=\"text\" />\n}\n";
    let mut f = fixture("kw-complete-element", source);
    let id = source.find("id=").unwrap();
    for offset in [id + 2, id + 1] {
        let (labels, result, forwarded) = complete_at(&mut f, offset);
        assert!(!forwarded, "must not be forwarded to rust-analyzer");
        assert!(labels.iter().any(|l| l == "id"), "{labels:?}");
        for backend in ["NAME_SPACE", "TAG_NAME", "into", "try_into"] {
            assert!(!labels.iter().any(|l| l == backend), "{backend}");
        }
        let range = &result["items"][0]["textEdit"]["range"];
        let column = source.lines().nth(1).unwrap().find("id").unwrap() as u64;
        assert_eq!(range["start"]["character"], column);
        assert_eq!(range["end"]["character"], column + 2);
    }
    // A keyword-named valued element attribute too.
    let ty = source.find("type=").unwrap();
    let (labels, _, forwarded) = complete_at(&mut f, ty + 4);
    assert!(!forwarded);
    assert!(labels.iter().any(|l| l == "type"), "{labels:?}");
}
