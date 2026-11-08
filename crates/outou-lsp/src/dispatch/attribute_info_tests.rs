//! Dispatch-level tests for hover and definition at a JSX attribute name
//! (`docs/backend-leakage.md` row 62, #31): answered by the server itself,
//! never by rust-analyzer, and in Outou's terms.
//!
//! Each test builds a workspace from the real backend's output, sends the
//! request through [`dispatch_client_request`] and reads the answer the
//! editor would get. A mock rust-analyzer logs whatever it is sent, so a
//! request that must not be forwarded is proven not to be: the answer is
//! there before dispatch returns, nothing is pending, and the log is empty.

use lsp_server::{Connection, Message, Request, RequestId};
use serde_json::{json, Value};

use super::keyword_prop_tests::{answer, fixture, text_at, Fixture};
use super::requests::dispatch_client_request;
use super::PendingKind;
use crate::documents::{RsxDocument, PARSE_COUNT};

/// `Field` is declared with doc comments and a keyword prop; `App` calls it
/// with every kind of attribute.
const SOURCE: &str = "\
#[component]
fn Field(
    /// The text shown above the input.
    label: String,
    /// The input type, such as `text`.
    r#type: String,
    size: Option<u32>,
) -> Element {
    <div><label>{label}</label><input type={r#type.clone()} class=\"f\" onclick={move |_| ()} /></div>
}
fn App() -> Element {
    <div class=\"app\" onclick={move |_| ()} key=\"k\">
        <Field label=\"Name\" type=\"text\" size={Some(3)} key=\"a\" bogus=\"x\" />
        <Other r#type=\"y\" />
        <External label=\"z\" />
    </div>
}
";

/// A second file's component, found through the workspace.
const OTHER: &str = "\
#[component]
pub fn Other(
    /// Declared in another file.
    r#type: String,
) -> Element {
    <p />
}
";

const OTHER_PATH: &str = "/app/src/other.rsx";

/// Words that mean a backend leaked.
const MARKERS: &[&str] = &[
    "dioxus",
    "Dioxus",
    "typed-builder",
    "typed_builder",
    "AttributeDescription",
    ".cargo/registry",
    "PropsBuilder",
    "rsx!",
    "r#",
    ".generated",
    "__",
];

struct Session {
    f: Fixture,
    connection: Connection,
    client: Connection,
}

fn session(name: &str) -> Session {
    let mut f = fixture(name, SOURCE);
    let other_uri = outou_sourcemap::file_uri(std::path::Path::new(OTHER_PATH));
    f.state.workspace.as_mut().unwrap().rsx.insert(
        other_uri.as_str().to_string(),
        RsxDocument::new(OTHER.to_string(), 1),
    );
    let (connection, client) = Connection::memory();
    Session {
        f,
        connection,
        client,
    }
}

/// A session whose main document holds `text` instead of [`SOURCE`], for
/// text the backend would refuse (a broken tag): the answers under test are
/// local and never read the generated unit.
fn session_with_text(name: &str, text: &str) -> Session {
    let mut s = session(name);
    let text: &'static str = Box::leak(text.to_string().into_boxed_str());
    s.f.source = text;
    let key = crate::uri::to_outou(&s.f.rsx_uri).as_str().to_string();
    s.f.state
        .workspace
        .as_mut()
        .unwrap()
        .rsx
        .insert(key, RsxDocument::new(text.to_string(), 2));
    s
}

impl Session {
    /// The `.rsx` offset `delta` bytes after the first `needle`.
    fn at(&self, needle: &str, delta: usize) -> usize {
        self.f
            .source
            .find(needle)
            .unwrap_or_else(|| panic!("{needle}"))
            + delta
    }

    /// Sends `method` at `offset` and returns the answer, which must already
    /// be there: nothing may have been forwarded to rust-analyzer.
    fn local(&mut self, method: &str, offset: usize) -> Value {
        self.request(method, offset, json!({}));
        let Some(Message::Response(response)) = self.client.receiver.try_recv().ok() else {
            panic!("{method} at {offset} was not answered locally");
        };
        assert!(
            self.f.state.pending.is_empty(),
            "{method} at {offset} was forwarded"
        );
        let log = std::fs::read_to_string(&self.f.log).unwrap_or_default();
        assert!(!log.contains(method), "rust-analyzer was sent {method}");
        let result = response.response_result.expect("a result");
        let text = result.to_string();
        for marker in MARKERS {
            assert!(!text.contains(marker), "{marker:?} in {text}");
        }
        result
    }

    fn hover(&mut self, offset: usize) -> Value {
        self.local("textDocument/hover", offset)
    }

    fn definition(&mut self, offset: usize) -> Value {
        self.local("textDocument/definition", offset)
    }

    /// Sends `method` at `offset` and returns the id the request got on its
    /// way to rust-analyzer. That it was forwarded is read from the pending
    /// table, which is set before dispatch returns, rather than from the
    /// mock's log, which a loaded machine can flush late.
    fn forwarded(&mut self, method: &str, offset: usize, extra: Value) -> RequestId {
        self.request(method, offset, extra);
        assert!(
            self.client.receiver.try_recv().is_err(),
            "{method} was answered locally"
        );
        let mut ids = self.f.state.pending.iter();
        let (id, pending) = ids.next().expect("a pending request");
        assert!(ids.next().is_none(), "one pending request");
        let expected_kind = match method {
            "textDocument/definition" => matches!(pending.kind, PendingKind::Definition),
            "textDocument/references" => matches!(pending.kind, PendingKind::References),
            other => panic!("{other}"),
        };
        assert!(expected_kind, "{method} was queued as another kind");
        id.clone()
    }

    fn request(&mut self, method: &str, offset: usize, extra: Value) {
        let mut params = json!({
            "textDocument": { "uri": self.f.rsx_uri.as_str() },
            "position": self.f.rsx_position(offset),
        });
        for (key, value) in extra.as_object().unwrap() {
            params[key] = value.clone();
        }
        dispatch_client_request(
            &mut self.f.state,
            &self.connection,
            Request {
                id: RequestId::from(7),
                method: method.to_string(),
                params,
            },
        );
    }
}

fn hover_text(hover: &Value) -> &str {
    hover["contents"]["value"].as_str().expect("hover text")
}

#[test]
fn hover_on_a_component_prop_shows_the_parameter_and_its_doc() {
    let mut s = session("ai-hover-prop");
    let hover = s.hover(s.at("<Field label", 8));
    assert_eq!(
        hover_text(&hover),
        "label: String\n\nThe text shown above the input."
    );
    assert_eq!(hover["contents"]["kind"], "plaintext");
    assert_eq!(text_at(SOURCE, &hover["range"]), "label");
}

#[test]
fn hover_asks_for_markdown_when_the_editor_prefers_it() {
    let mut s = session("ai-hover-markdown");
    s.f.state.client_prefers_markdown_hover = true;
    let hover = s.hover(s.at("<Field label", 8));
    assert_eq!(hover["contents"]["kind"], "markdown");
    assert_eq!(
        hover_text(&hover),
        "```rust\nlabel: String\n```\n\nThe text shown above the input."
    );
}

#[test]
fn hover_on_a_keyword_prop_shows_the_plain_name() {
    let mut s = session("ai-hover-keyword");
    let hover = s.hover(s.at("type=\"text\"", 1));
    assert_eq!(
        hover_text(&hover),
        "type: String\n\nThe input type, such as `text`."
    );
    assert_eq!(text_at(SOURCE, &hover["range"]), "type");
}

#[test]
fn hover_on_a_raw_identifier_attribute_shows_the_plain_name() {
    let mut s = session("ai-hover-raw");
    let hover = s.hover(s.at("<Other r#type", 9));
    assert_eq!(
        hover_text(&hover),
        "type: String\n\nDeclared in another file."
    );
    // The range is the name as written.
    assert_eq!(text_at(SOURCE, &hover["range"]), "r#type");
}

#[test]
fn hover_on_a_prop_without_a_doc_shows_only_the_signature() {
    let mut s = session("ai-hover-nodoc");
    let hover = s.hover(s.at("size={Some", 1));
    assert_eq!(hover_text(&hover), "size: Option<u32>");
}

#[test]
fn hover_on_the_equals_sign_after_a_name_answers_for_that_name() {
    let mut s = session("ai-hover-equals");
    let hover = s.hover(s.at("label=\"Name\"", "label".len()));
    assert_eq!(hover_text(&hover).lines().next(), Some("label: String"));
}

#[test]
fn definition_on_a_prop_jumps_to_its_parameter() {
    let mut s = session("ai-def-prop");
    let found = s.definition(s.at("<Field label", 8));
    let locations = found.as_array().expect("a location list");
    assert_eq!(locations.len(), 1);
    assert_eq!(locations[0]["uri"], s.f.rsx_uri.as_str());
    assert_eq!(text_at(SOURCE, &locations[0]["range"]), "label");
    let line = locations[0]["range"]["start"]["line"].as_u64().unwrap() as usize;
    assert!(SOURCE.lines().nth(line).unwrap().contains("label: String"));
}

#[test]
fn definition_on_a_keyword_prop_selects_the_parameter_as_written() {
    let mut s = session("ai-def-keyword");
    let found = s.definition(s.at("type=\"text\"", 1));
    assert_eq!(text_at(SOURCE, &found[0]["range"]), "r#type");
}

#[test]
fn definition_on_a_prop_of_a_component_in_another_file_jumps_there() {
    let mut s = session("ai-def-other-file");
    let found = s.definition(s.at("<Other r#type", 9));
    let other_uri = outou_sourcemap::file_uri(std::path::Path::new(OTHER_PATH));
    assert_eq!(found[0]["uri"], other_uri.as_str());
    assert_eq!(text_at(OTHER, &found[0]["range"]), "r#type");
}

#[test]
fn an_element_attribute_has_no_hover_and_no_definition() {
    let mut s = session("ai-element-attr");
    let positions = [
        s.at("<div class=\"app\"", 7),
        s.at("onclick={move |_| ()} key", 3),
        s.at("<input type=", "<input ".len() + 1),
        s.at("class=\"f\"", 2),
    ];
    for offset in positions {
        assert_eq!(s.hover(offset), Value::Null, "hover at {offset}");
        assert_eq!(s.definition(offset), Value::Null, "definition at {offset}");
    }
}

#[test]
fn key_is_described_by_outou_on_a_component_and_on_an_element() {
    let mut s = session("ai-key");
    for offset in [s.at("key=\"a\"", 1), s.at("key=\"k\"", 1)] {
        let hover = s.hover(offset);
        let text = hover_text(&hover);
        assert!(text.starts_with("key\n\nIdentifies this element among its siblings"));
        assert_eq!(text_at(SOURCE, &hover["range"]), "key");
        assert_eq!(s.definition(offset), Value::Null);
    }
}

#[test]
fn an_unknown_prop_and_an_undeclared_component_answer_null() {
    let mut s = session("ai-unknown");
    for offset in [s.at("bogus=", 2), s.at("<External label", 12)] {
        assert_eq!(s.hover(offset), Value::Null);
        assert_eq!(s.definition(offset), Value::Null);
    }
}

#[test]
fn the_text_of_an_attribute_value_answers_null() {
    let mut s = session("ai-value-text");
    for offset in [s.at("\"Name\"", 2), s.at("\"text\"", 1), s.at("\"app\"", 1)] {
        assert_eq!(s.hover(offset), Value::Null);
        assert_eq!(s.definition(offset), Value::Null);
    }
}

#[test]
fn no_attribute_position_answers_with_backend_vocabulary() {
    // `Session::local` rejects every marker; walk every byte of every
    // attribute name and every value string.
    let mut s = session("ai-sweep");
    let source = SOURCE;
    let mut offsets = Vec::new();
    for word in [
        "label=", "type=", "size=", "key=", "bogus=", "class=", "onclick=", "r#type=",
    ] {
        let mut from = 0;
        while let Some(found) = source[from..].find(word) {
            let start = from + found;
            offsets.extend(start..start + word.len());
            from = start + word.len();
        }
    }
    for offset in offsets {
        s.hover(offset);
        s.definition(offset);
    }
}

#[test]
fn a_request_parses_only_the_current_document() {
    let mut s = session("ai-parses");
    let offset = s.at("<Field label", 8);
    let before = PARSE_COUNT.with(|count| count.get());
    s.hover(offset);
    s.definition(offset);
    let used = PARSE_COUNT.with(|count| count.get()) - before;
    // The tag-name check and the attribute lookup parse the open document
    // once each for hover, once for definition; the other file is cached.
    assert!(used <= 3, "{used} parses for two requests");
}

#[test]
fn an_attribute_value_expression_is_still_forwarded() {
    let mut s = session("ai-forward-expression");
    let offset = s.at("{r#type.clone()}", 3);
    s.forwarded("textDocument/definition", offset, json!({}));
}

const REGISTRY_FILE: &str =
    "file:///Users/me/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/dioxus-core-macro-0.7.10/src/lib.rs";

fn one_line(path: &str) -> Value {
    json!({
        "uri": path,
        "range": { "start": { "line": 3, "character": 0 }, "end": { "line": 3, "character": 4 } },
    })
}

#[test]
fn a_backend_source_location_is_dropped_from_a_forwarded_definition() {
    let mut s = session("ai-filter-definition");
    let offset = s.at("{r#type.clone()}", 3);
    let ra_id = s.forwarded("textDocument/definition", offset, json!({}));
    let response = answer(
        &mut s.f,
        &s.connection,
        &s.client,
        ra_id,
        json!([
            one_line(REGISTRY_FILE),
            one_line("file:///app/src/helpers.rs")
        ]),
    );
    let result = response.response_result.unwrap();
    assert_eq!(result, json!([one_line("file:///app/src/helpers.rs")]));
}

#[test]
fn a_backend_source_location_is_dropped_from_forwarded_references() {
    let mut s = session("ai-filter-references");
    let offset = s.at("class=\"f\"", 2);
    let ra_id = s.forwarded(
        "textDocument/references",
        offset,
        json!({ "context": { "includeDeclaration": true } }),
    );
    let response = answer(
        &mut s.f,
        &s.connection,
        &s.client,
        ra_id,
        json!([
            one_line(REGISTRY_FILE),
            one_line("file:///app/src/helpers.rs"),
            one_line("file:///Users/me/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/dioxus-html-0.7.10/src/elements.rs"),
        ]),
    );
    let result = response.response_result.unwrap();
    assert_eq!(result, json!([one_line("file:///app/src/helpers.rs")]));
}

#[test]
fn a_dependency_that_is_not_the_backend_is_still_a_definition_target() {
    let mut s = session("ai-keep-dependency");
    let offset = s.at("{r#type.clone()}", 3);
    let ra_id = s.forwarded("textDocument/definition", offset, json!({}));
    let serde = "file:///Users/me/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/serde-1.0.200/src/lib.rs";
    let response = answer(
        &mut s.f,
        &s.connection,
        &s.client,
        ra_id,
        json!([one_line(serde)]),
    );
    assert_eq!(response.response_result.unwrap(), json!([one_line(serde)]));
}

const FIELD: &str = "#[component]\nfn Field(\n    /// Uses *stars*, `code`, <b>html</b> and _under_.\n    label: String,\n) -> Element {\n    <p />\n}\n";

#[test]
fn a_hyphenated_element_attribute_answers_null() {
    let text =
        format!("{FIELD}fn App() -> Element {{\n    <div data-id=\"1\" aria-label=\"x\" />\n}}\n");
    let mut s = session_with_text("ai-hyphen", &text);
    for offset in [s.at("data-id", 3), s.at("aria-label", 6)] {
        assert_eq!(s.hover(offset), Value::Null);
        assert_eq!(s.definition(offset), Value::Null);
    }
}

#[test]
fn a_raw_key_is_the_key_attribute() {
    let text = format!(
        "{FIELD}fn App() -> Element {{\n    <div r#key=\"a\"><Field r#key=\"b\" label=\"x\" /></div>\n}}\n"
    );
    let mut s = session_with_text("ai-raw-key", &text);
    for offset in [s.at("r#key=\"a\"", 3), s.at("r#key=\"b\"", 3)] {
        let hover = s.hover(offset);
        assert!(hover_text(&hover).starts_with("key\n\nIdentifies this element"));
        assert_eq!(s.definition(offset), Value::Null);
    }
}

#[test]
fn an_attribute_on_a_broken_tag_is_still_answered_locally() {
    for (name, tail) in [
        ("ai-broken-eof", "<Field label=\"Name\" bo"),
        ("ai-broken-novalue", "<Field label=\n    <p />"),
        ("ai-broken-unclosed", "<div><Field label=\"Name\" ></div>"),
    ] {
        let text = format!("{FIELD}fn App() -> Element {{\n    {tail}\n}}\n");
        let mut s = session_with_text(name, &text);
        let offset = s.at("<Field label", 8);
        let hover = s.hover(offset);
        assert!(hover_text(&hover).starts_with("label: String"), "{name}");
        let found = s.definition(offset);
        assert_eq!(found.as_array().map(Vec::len), Some(1), "{name}");
    }
}

#[test]
fn attributes_inside_a_fragment_are_answered_and_the_fragment_itself_is_null() {
    let text = format!(
        "{FIELD}fn App() -> Element {{\n    <><Field label=\"x\" /><p class=\"c\" /></>\n}}\n"
    );
    let mut s = session_with_text("ai-fragment", &text);
    let prop = s.at("<Field label", 8);
    assert!(hover_text(&s.hover(prop)).starts_with("label: String"));
    let element_attr = s.at("class=", 2);
    assert_eq!(s.hover(element_attr), Value::Null);
    assert_eq!(s.definition(element_attr), Value::Null);
    let open = s.at("<><Field", 1);
    assert_eq!(s.hover(open), Value::Null);
}

#[test]
fn markdown_in_a_doc_comment_is_passed_through_after_the_code_block() {
    let text = format!("{FIELD}fn App() -> Element {{\n    <Field label=\"x\" />\n}}\n");
    for markdown in [true, false] {
        let mut s = session_with_text("ai-markdown-doc", &text);
        s.f.state.client_prefers_markdown_hover = markdown;
        let hover = s.hover(s.at("<Field label", 8));
        let doc = "Uses *stars*, `code`, <b>html</b> and _under_.";
        let expected = if markdown {
            format!("```rust\nlabel: String\n```\n\n{doc}")
        } else {
            format!("label: String\n\n{doc}")
        };
        assert_eq!(hover_text(&hover), expected);
    }
}

#[test]
fn a_definition_that_was_only_a_backend_location_becomes_null() {
    let mut s = session("ai-only-backend");
    let offset = s.at("<div class=\"app\"", 2);
    let ra_id = s.forwarded("textDocument/definition", offset, json!({}));
    let response = answer(
        &mut s.f,
        &s.connection,
        &s.client,
        ra_id,
        json!([one_line(REGISTRY_FILE)]),
    );
    assert_eq!(response.response_result.unwrap(), Value::Null);
}
