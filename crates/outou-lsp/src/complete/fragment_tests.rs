//! Fragments (`<>…</>`, ADR 0015) in the cursor classifier: the punctuation
//! is a position of its own (hover and rename are answered locally, there is
//! no name to complete), `</` typed directly inside a fragment offers `</>`,
//! and nothing inside a fragment changes how its children classify.

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
        plan_error: None,
    };
    workspace.rsx.insert(
        URI.to_string(),
        crate::documents::RsxDocument::new(source.to_string(), 1),
    );
    workspace
}

fn app(body: &str) -> String {
    format!("#[component]\nfn App() -> Element {{\n{body}\n}}\n")
}

fn position_at(workspace: &Workspace, offset: usize) -> lsp_types::Position {
    let doc = workspace.rsx.get(URI).unwrap();
    let offset = offset as u32;
    mapping::to_lsp_range(
        doc.line_index
            .span_to_range(outou_sourcemap::Span::new(offset, offset)),
    )
    .start
}

fn cursor_at(source: &str, offset: usize) -> Cursor {
    let parsed = crate::documents::parse_rsx(source);
    classify(&parsed.file, source, offset as u32)
}

fn complete_at(source: &str, offset: usize) -> Option<Vec<Value>> {
    let workspace = workspace_with(source);
    local_completion(&workspace, URI, position_at(&workspace, offset))
}

fn labels(items: &[Value]) -> Vec<&str> {
    items
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect()
}

#[test]
fn the_punctuation_of_a_fragment_is_a_fragment_tag_position() {
    let source = app("    <>\n        <p>hi</p>\n    </>");
    let open = source.find("<>").unwrap();
    let close = source.find("</>").unwrap();
    for offset in [open, open + 1, close, close + 1, close + 2] {
        assert!(
            matches!(cursor_at(&source, offset), Cursor::FragmentTag),
            "offset {offset}"
        );
    }
    // After the `>` the cursor is in the children.
    assert!(!matches!(cursor_at(&source, open + 2), Cursor::FragmentTag));
    assert!(!matches!(
        cursor_at(&source, close + 3),
        Cursor::FragmentTag
    ));
}

#[test]
fn hover_and_rename_treat_the_punctuation_as_a_non_renameable_tag() {
    let source = app("    <>\n        <p>hi</p>\n    </>");
    let workspace = workspace_with(&source);
    let on_open = position_at(&workspace, source.find("<>").unwrap());
    let on_close = position_at(&workspace, source.find("</>").unwrap() + 1);
    let on_child = position_at(&workspace, source.find("<p>").unwrap() + 1);
    assert!(is_tag_name_position(&workspace, URI, on_open));
    assert!(is_tag_name_position(&workspace, URI, on_close));
    assert!(is_fragment_tag_position(&workspace, URI, on_open));
    assert!(is_fragment_tag_position(&workspace, URI, on_close));
    // The child's own name stays a tag name, but not a fragment tag.
    assert!(is_tag_name_position(&workspace, URI, on_child));
    assert!(!is_fragment_tag_position(&workspace, URI, on_child));
    assert_eq!(tag_name_at_position(&workspace, URI, on_open), None);
}

#[test]
fn completion_on_the_punctuation_is_forwarded_as_before() {
    let source = app("    <>\n        <p>hi</p>\n    </>");
    assert!(complete_at(&source, source.find("<>").unwrap() + 1).is_none());
}

#[test]
fn a_tag_name_inside_a_fragment_completes_like_anywhere_else() {
    let source = app("    <>\n        <di\n    </>");
    let items = complete_at(&source, source.find("<di").unwrap() + 3).expect("answered locally");
    assert!(labels(&items).contains(&"div"), "{:?}", labels(&items));
}

#[test]
fn typing_the_closing_slash_inside_a_fragment_offers_the_fragment_close() {
    // `|` marks the cursor.
    for body in [
        "    <>\n        <p>hi</p>\n        </|\n",
        "    <>\n        <p>hi</p>\n        </|",
        "    <>\n        </|\n    </>",
        "    <></|",
    ] {
        let source = app(&body.replace('|', ""));
        let offset = app(body).find('|').unwrap();
        let items =
            complete_at(&source, offset).unwrap_or_else(|| panic!("answered locally for {body:?}"));
        assert_eq!(labels(&items), ["</>"], "{body:?}");
        assert_eq!(items[0]["textEdit"]["newText"], ">", "{body:?}");
        assert_eq!(items[0]["filterText"], "", "{body:?}");
    }
}

#[test]
fn the_fragment_close_is_not_offered_inside_an_unclosed_child() {
    let source = app("    <>\n        <div>\n        </\n");
    let offset = source.rfind("</").unwrap() + 2;
    let found = complete_at(&source, offset);
    assert!(
        found.is_none_or(|items| !labels(&items).contains(&"</>")),
        "an open `<div>` owns the `</`"
    );
}

#[test]
fn the_fragment_close_is_not_offered_for_an_ordinary_element() {
    let source = app("    <div>\n        </\n");
    let offset = source.rfind("</").unwrap() + 2;
    let found = complete_at(&source, offset);
    assert!(found.is_none_or(|items| !labels(&items).contains(&"</>")));
}

#[test]
fn no_cursor_position_in_a_fragment_panics() {
    for body in [
        "    <>\n        <p>{x}</p>{y}\n    </>",
        "    < /* c */ >text</ /* c */ >",
        "    <><></></>",
        "    <>\n        <",
        "    <>\n        </",
        "    </>",
        "    <></div>",
        "    <div></>",
    ] {
        let source = app(body);
        let workspace = workspace_with(&source);
        for offset in 0..=source.len() {
            if !source.is_char_boundary(offset) {
                continue;
            }
            let position = position_at(&workspace, offset);
            let _ = local_completion(&workspace, URI, position);
            let _ = is_tag_name_position(&workspace, URI, position);
            let _ = is_fragment_tag_position(&workspace, URI, position);
            let _ = tag_name_at_position(&workspace, URI, position);
        }
    }
}
