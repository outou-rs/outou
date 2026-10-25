//! Attribute-name completion on intrinsic elements: which names are
//! offered (already-written ones excluded, any lowercase tag is an
//! element), the shape of each item, and the positions it must not claim.

use super::*;

const URI: &str = "file:///app/src/main.rsx";

fn workspace_with(source: &str) -> Workspace {
    let mut workspace = Workspace {
        manifest_dir: std::path::PathBuf::from("/app"),
        plan: None,
        registry: outou_sourcemap::Registry::new(),
        rsx: std::collections::HashMap::new(),
        generated: std::collections::HashMap::new(),
        rsx_to_generated: std::collections::HashMap::new(),
    };
    workspace.rsx.insert(
        URI.to_string(),
        crate::documents::RsxDocument::new(source.to_string(), 1),
    );
    workspace
}

/// Completes at the end of the last occurrence of `marker` in `source`.
fn complete_after(source: &str, marker: &str) -> Option<Vec<Value>> {
    let offset = source.rfind(marker).expect("marker in source") + marker.len();
    complete_at(source, offset)
}

fn complete_at(source: &str, offset: usize) -> Option<Vec<Value>> {
    let workspace = workspace_with(source);
    let doc = workspace.rsx.get(URI).unwrap();
    let offset = offset as u32;
    let position = mapping::to_lsp_range(
        doc.line_index
            .span_to_range(outou_sourcemap::Span::new(offset, offset)),
    )
    .start;
    local_completion(&workspace, URI, position)
}

fn labels(items: &[Value]) -> Vec<&str> {
    items
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect()
}

fn app(body: &str) -> String {
    format!("#[component]\nfn App() -> Element {{\n{body}\n}}\n")
}

// ---- already-written attributes are not offered again ----------------

#[test]
fn an_attribute_that_is_already_written_is_not_offered_again() {
    let source = app("    <input checked={done} ch />");
    let items = complete_after(&source, "ch").expect("answered locally");
    assert!(labels(&items).is_empty(), "{:?}", labels(&items));
}

#[test]
fn the_attribute_being_typed_is_still_offered() {
    let source = app("    <input type=\"checkbox\" ch />");
    let items = complete_after(&source, " ch").expect("answered locally");
    assert_eq!(labels(&items), vec!["checked"]);
}

#[test]
fn a_written_class_is_not_offered_but_its_neighbours_are() {
    let source = app("    <div class=\"a\" cl />");
    let items = complete_after(&source, " cl").expect("answered locally");
    assert!(labels(&items).is_empty(), "{:?}", labels(&items));
}

#[test]
fn a_partial_name_that_is_a_prefix_of_a_written_one_offers_the_others() {
    // `class` is written; `cl` must not offer it, but a longer word that
    // merely starts the same way is unaffected.
    let source = app("    <div class=\"a\" c />");
    let items = complete_after(&source, "\" c").expect("answered locally");
    let labels = labels(&items);
    assert_eq!(labels, vec!["contenteditable"]);
    assert!(labels.iter().all(|label| label.starts_with('c')));
}

#[test]
fn written_hyphenated_attributes_are_excluded() {
    let source = app("    <div aria-label=\"x\" data-id=\"1\" aria-l />");
    let items = complete_after(&source, "aria-l").expect("answered locally");
    let labels = labels(&items);
    assert!(!labels.contains(&"aria-label"));
    assert!(labels.contains(&"aria-labelledby"));
}

#[test]
fn island_valued_bare_and_multiline_attributes_are_all_excluded() {
    let source = app(
        "    <input\n        disabled\n        value={text}\n        onchange={move |_| {}}\n        placeholder=\"x\"\n        \n    />",
    );
    let offset =
        source.find("placeholder=\"x\"\n        ").unwrap() + "placeholder=\"x\"\n        ".len();
    let items = complete_at(&source, offset).expect("answered locally");
    let labels = labels(&items);
    for written in ["disabled", "value", "onchange", "placeholder"] {
        assert!(!labels.contains(&written), "{written} offered again");
    }
    assert!(labels.contains(&"checked") && labels.contains(&"onclick"));
}

#[test]
fn a_duplicated_written_attribute_is_excluded_once() {
    let source = app("    <input checked checked={a} ch />");
    let items = complete_after(&source, " ch").expect("answered locally");
    assert!(labels(&items).is_empty());
}

#[test]
fn attributes_after_the_cursor_are_excluded_too() {
    let source = app("    <input \n        \n        checked={done}\n    />");
    let offset = source.find("<input \n        ").unwrap() + "<input \n        ".len();
    let items = complete_at(&source, offset).expect("answered locally");
    assert!(!labels(&items).contains(&"checked"));
    assert!(labels(&items).contains(&"class"));
}

#[test]
fn a_component_prop_that_is_already_written_is_not_offered_again() {
    let source = "#[component]\nfn UserCard(user: User, label: String) -> Element {\n    <p>{label}</p>\n}\n\n#[component]\nfn App() -> Element {\n    <UserCard user={u} />\n}\n";
    let offset = source.find("<UserCard user={u} ").unwrap() + "<UserCard user={u} ".len();
    let items = complete_at(source, offset).expect("answered locally");
    assert_eq!(labels(&items), vec!["label"]);
}

// ---- any lowercase tag is an element ---------------------------------

#[test]
fn a_lowercase_tag_missing_from_the_element_list_still_gets_attributes() {
    let source = app("    <details cl />");
    let items = complete_after(&source, "cl").expect("answered locally");
    let labels = labels(&items);
    assert!(labels.contains(&"class"));
    assert!(labels.iter().all(|label| label.starts_with("cl")));

    let source = app("    <details open />");
    let items = complete_after(&source, "<details ").expect("answered locally");
    let labels = labels_owned(&items);
    for global in ["class", "id", "key", "style", "title", "hidden", "tabindex"] {
        assert!(labels.iter().any(|l| l == global), "{global}");
    }
    assert!(labels.iter().any(|l| l == "aria-label"));
    assert!(labels.iter().any(|l| l == "onclick"));
}

fn labels_owned(items: &[Value]) -> Vec<String> {
    labels(items).into_iter().map(str::to_string).collect()
}

#[test]
fn an_unknown_lowercase_and_a_custom_element_get_the_global_attributes() {
    let source = app("    <my-widget cl />");
    let items = complete_after(&source, "cl").expect("answered locally");
    assert!(labels(&items).contains(&"class"));
    let source = app("    <nonsense cl />");
    let items = complete_after(&source, "cl").expect("answered locally");
    assert!(labels(&items).contains(&"class"));
}

#[test]
fn an_uppercase_tag_is_a_component_and_never_uses_the_html_table() {
    // No such component in the workspace: nothing is offered, and above
    // all not the HTML attributes.
    let source = app("    <Unknown cl />");
    let items = complete_after(&source, "cl").expect("answered locally");
    assert!(labels(&items).is_empty());

    // A known component offers exactly its own props.
    let source = "#[component]\nfn Card(class_name: String) -> Element {\n    <p></p>\n}\n\n#[component]\nfn App() -> Element {\n    <Card cl />\n}\n";
    let items = complete_after(source, "<Card cl").expect("answered locally");
    assert_eq!(labels(&items), vec!["class_name"]);
}

// ---- the table ---------------------------------------------------------

#[test]
fn the_requested_attribute_vocabulary_is_offered() {
    let names = crate::html_attrs::attribute_names("input");
    for expected in [
        "autofocus",
        "autocomplete",
        "form",
        "name",
        "value",
        "placeholder",
        "required",
        "readonly",
        "disabled",
        "checked",
        "min",
        "max",
        "step",
        "pattern",
        "maxlength",
        "minlength",
        "size",
        "multiple",
        "aria-label",
        "aria-labelledby",
        "aria-describedby",
        "aria-hidden",
        "aria-live",
        "aria-pressed",
        "aria-expanded",
        "aria-checked",
        "aria-controls",
        "aria-current",
        "aria-disabled",
        "aria-selected",
        "aria-required",
        "aria-invalid",
        "aria-haspopup",
        "aria-modal",
        "role",
    ] {
        assert!(names.contains(&expected), "input lacks {expected}");
    }
    let by_element = [
        ("textarea", &["rows", "cols", "wrap"][..]),
        ("a", &["target", "rel", "href", "download"]),
        ("form", &["action", "method", "enctype", "novalidate"]),
        (
            "img",
            &[
                "src",
                "srcset",
                "sizes",
                "alt",
                "width",
                "height",
                "loading",
                "crossorigin",
            ],
        ),
        (
            "video",
            &[
                "poster",
                "preload",
                "controls",
                "autoplay",
                "loop",
                "muted",
                "playsinline",
            ],
        ),
        ("label", &["for"]),
        ("option", &["selected", "label"]),
        ("td", &["colspan", "rowspan"]),
    ];
    for (tag, expected) in by_element {
        let names = crate::html_attrs::attribute_names(tag);
        for name in expected {
            assert!(names.contains(name), "{tag} lacks {name}");
        }
    }
}

#[test]
fn a_data_prefix_has_no_candidates_but_is_answered_locally() {
    let source = app("    <div data-i />");
    let items = complete_after(&source, "data-i").expect("answered locally");
    assert!(items.is_empty());
}

// ---- EOF -----------------------------------------------------------------

#[test]
fn a_blank_position_at_the_end_of_the_file_is_an_attribute_name_position() {
    let source = "#[component]\nfn App() -> Element {\n    <input ";
    let items = complete_at(source, source.len()).expect("answered locally");
    assert!(labels(&items).contains(&"checked"));

    let source = "#[component]\nfn App() -> Element {\n    <input type=\"text\"\n        ";
    let items = complete_at(source, source.len()).expect("answered locally");
    let labels = labels(&items);
    assert!(labels.contains(&"checked") && !labels.contains(&"type"));
}

#[test]
fn a_partial_name_at_the_end_of_the_file_is_still_completed() {
    let source = "#[component]\nfn App() -> Element {\n    <input ch";
    let items = complete_at(source, source.len()).expect("answered locally");
    assert_eq!(labels(&items), vec!["checked"]);
}

#[test]
fn whitespace_in_element_content_at_the_end_of_the_file_is_not_claimed() {
    let source = "#[component]\nfn App() -> Element {\n    <div class=\"a\">\n        ";
    assert_eq!(complete_at(source, source.len()), None);
}

// ---- item shape ----------------------------------------------------------

#[test]
fn an_item_replaces_only_the_typed_prefix_and_carries_no_insert_text() {
    let source = app("  <input ch />");
    let items = complete_after(&source, "ch").expect("answered locally");
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item["label"], "checked");
    assert_eq!(item["kind"], 5);
    assert_eq!(item["textEdit"]["newText"], "checked");
    assert!(item.get("insertText").is_none());
    // `  <input ch` starts on line 2 (0-based); `ch` is columns 9..11.
    assert_eq!(item["textEdit"]["range"]["start"]["line"], 2);
    assert_eq!(item["textEdit"]["range"]["start"]["character"], 9);
    assert_eq!(item["textEdit"]["range"]["end"]["line"], 2);
    assert_eq!(item["textEdit"]["range"]["end"]["character"], 11);
}

#[test]
fn a_blank_position_inserts_at_a_zero_width_range() {
    let source = app("  <input \n  />");
    let items = complete_after(&source, "<input ").expect("answered locally");
    let range = &items[0]["textEdit"]["range"];
    assert_eq!(range["start"], range["end"]);
}

#[test]
fn the_edit_range_is_in_utf16_units_after_japanese_text() {
    let line = "  日本語😀<input ch />";
    let source = app(&format!("  <div>\n{line}\n  </div>"));
    let items = complete_after(&source, "<input ch").expect("answered locally");
    let item = items
        .iter()
        .find(|item| item["label"] == "checked")
        .expect("checked offered");
    let start = line[..line.find("ch").unwrap()].encode_utf16().count();
    assert_eq!(item["textEdit"]["range"]["start"]["character"], start);
    assert_eq!(item["textEdit"]["range"]["end"]["character"], start + 2);
}

#[test]
fn the_response_is_complete_and_wraps_the_items() {
    let response = completion_response(vec![json!({ "label": "checked" })]);
    assert_eq!(response["isIncomplete"], false);
    assert_eq!(response["items"][0]["label"], "checked");
}

// ---- line endings and tabs -------------------------------------------------

#[test]
fn crlf_and_tab_indented_attributes_are_completed() {
    let source = "#[component]\r\nfn Row() -> Element {\r\n\t<input\r\n\t\ttype=\"checkbox\"\r\n\t\tch\r\n\t\tchecked={done}\r\n\t/>\r\n}\r\n";
    let typed = source.find("\t\tch\r\n").unwrap() + "\t\tch".len();
    let items = complete_at(source, typed).expect("answered locally");
    // `checked` is written on the next line and nothing else starts with `ch`.
    assert!(labels(&items).is_empty(), "{:?}", labels(&items));

    let blank = source.find("\t\tch\r\n").unwrap();
    let items = complete_at(source, blank + 2).expect("answered locally");
    let labels = labels_owned(&items);
    assert!(labels.iter().any(|l| l == "class"));
    assert!(!labels.iter().any(|l| l == "type" || l == "checked"));
    let range = &items[0]["textEdit"]["range"];
    assert_eq!(range["start"]["line"], 4);
    assert_eq!(range["start"]["character"], 2);
}

// ---- positions this module must not claim ----------------------------------

#[test]
fn inside_a_string_attribute_value_nothing_is_claimed() {
    let source = app("    <div class=\"ab\" />");
    assert_eq!(complete_after(&source, "class=\"a"), None);
}

#[test]
fn inside_an_island_attribute_value_nothing_is_claimed() {
    let source = app("    <div class={user.na} />");
    assert_eq!(complete_after(&source, "user.na"), None);
}

#[test]
fn a_rust_comparison_and_generics_are_not_jsx_positions() {
    let source = "fn compare(a: u32, b: u32) -> bool {\n    a < b\n}\n\nfn make() -> Vec<String> {\n    let v: Vec<String> = Vec::new();\n    v\n}\n";
    assert_eq!(complete_after(source, "a <"), None);
    assert_eq!(complete_after(source, "a < "), None);
    assert_eq!(complete_after(source, "Vec<"), None);
    assert_eq!(complete_after(source, "Vec<Str"), None);
}

#[test]
fn a_closing_tag_name_is_a_tag_name_position_not_an_attribute_one() {
    let source = app("    <div class=\"a\">hi</div>");
    let items = complete_after(&source, "</di").expect("answered locally");
    let labels = labels(&items);
    assert!(labels.contains(&"div"));
    assert!(!labels.contains(&"class"));
}

#[test]
fn text_inside_element_content_is_not_an_attribute_position() {
    let source = app("    <div class=\"a\">hello wor</div>");
    assert_eq!(complete_after(&source, "hello wor"), None);
}
