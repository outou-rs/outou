//! Attribute-value parsing (grammar §5.1): M1 — a raw string
//! (`r#"…"#`) is a perfectly ordinary Rust string literal and must be
//! accepted as an attribute value; conversely, a missing or unquoted
//! value (`title=`, `title=value`) must be diagnosed instead of silently
//! treated as a bare attribute.

use outou_syntax::ast;

fn find_jsx_element(file: &ast::File) -> Option<&ast::JsxElement> {
    for item in &file.items {
        if let ast::Item::Function(f) = item {
            for expr in f.body.statements.iter().chain(f.body.tail.as_deref()) {
                if let ast::Expr::Jsx(el) = expr {
                    return Some(el);
                }
            }
        }
    }
    None
}

const INVALID_VALUE_MESSAGE: &str =
    "attribute values must be double-quoted strings or `{…}` expressions";

#[test]
fn raw_string_attribute_value_is_accepted() {
    let source = r####"fn f() { <A title=r#"he"llo"# />; }"####;
    let parsed = outou_syntax::parse(source);
    assert!(
        parsed.diagnostics.is_empty(),
        "source: {source}: {:?}",
        parsed.diagnostics
    );
    let element = find_jsx_element(&parsed.file).expect("jsx element");
    assert_eq!(element.attributes.len(), 1, "{:?}", element.attributes);
    let attr = &element.attributes[0];
    assert_eq!(attr.name.name, "title");
    match &attr.value {
        Some(ast::JsxAttributeValue::Text(text)) => assert_eq!(text.value, "he\"llo"),
        other => panic!("expected Text value, got {other:?}"),
    }
}

#[test]
fn byte_and_c_string_attribute_values_are_rejected() {
    // Grammar §5.1 allows exactly Rust's plain and raw *string* grammar as
    // an attribute value — never a byte-string or C-string, even though
    // both look like an ordinary string apart from their prefix (M1,
    // MEDIUM-4, issue #4 fix list item 5).
    for source in [
        r#"fn f() { <A title=b"x" />; }"#,
        r####"fn f() { <A title=br#"x"#/>; }"####,
        r#"fn f() { <A title=c"x" />; }"#,
        r####"fn f() { <A title=cr#"x"#/>; }"####,
    ] {
        let parsed = outou_syntax::parse(source);
        let element = find_jsx_element(&parsed.file).expect("jsx element");
        let title_attr = element
            .attributes
            .iter()
            .find(|a| a.name.name == "title")
            .unwrap_or_else(|| panic!("no `title` attribute: {:?}", element.attributes));
        assert!(
            matches!(title_attr.value, Some(ast::JsxAttributeValue::Error(_))),
            "source: {source}: {:?}",
            title_attr.value
        );
        let matching = parsed
            .diagnostics
            .iter()
            .filter(|d| d.message == INVALID_VALUE_MESSAGE)
            .count();
        assert_eq!(matching, 1, "source: {source}: {:?}", parsed.diagnostics);
    }
}

#[test]
fn invalid_non_string_attribute_value_is_diagnosed_exactly_once() {
    // MEDIUM-5, issue #4 fix list item 6: leaving the offending literal
    // unconsumed made the attribute loop's fallback re-scan it one byte at
    // a time, producing one extra diagnostic per byte.
    for (source, expected_title_end) in [
        ("fn f() { <A title=42 />; }", "fn f() { <A title=42"),
        (
            "fn f() { <A href=12345678 />; }",
            "fn f() { <A href=12345678",
        ),
    ] {
        let parsed = outou_syntax::parse(source);
        let element = find_jsx_element(&parsed.file).expect("jsx element");
        assert_eq!(
            element.attributes.len(),
            1,
            "source: {source}: {:?}",
            element.attributes
        );
        let matching = parsed
            .diagnostics
            .iter()
            .filter(|d| d.message == INVALID_VALUE_MESSAGE)
            .count();
        assert_eq!(matching, 1, "source: {source}: {:?}", parsed.diagnostics);
        assert_eq!(
            &source[..element.attributes[0].span.end as usize],
            expected_title_end,
            "source: {source}"
        );
    }
}

#[test]
fn plain_string_attribute_value_decodes_escapes() {
    // MEDIUM-8, issue #6 fix list item 9: grammar.md §9 says a plain
    // attribute string "uses Rust's own string literal grammar", which
    // includes escape processing — `JsxAttributeValue::Text::value` must
    // be the *decoded* string, matching how a raw string's content
    // (which has no escapes at all) is already used as-is.
    let source = r#"fn f() { <A title="tab\there\nline \\ and \"quote\"" />; }"#;
    let parsed = outou_syntax::parse(source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let element = find_jsx_element(&parsed.file).expect("jsx element");
    let attr = &element.attributes[0];
    match &attr.value {
        Some(ast::JsxAttributeValue::Text(text)) => {
            assert_eq!(text.value, "tab\there\nline \\ and \"quote\"");
        }
        other => panic!("expected Text value, got {other:?}"),
    }
}

#[test]
fn raw_string_attribute_value_has_no_escape_processing() {
    // Raw strings have no escapes at all: their content is exactly the
    // bytes between the delimiters, contrasted here with
    // `plain_string_attribute_value_decodes_escapes` above.
    let source = r####"fn f() { <A title=r#"tab\tliteral"# />; }"####;
    let parsed = outou_syntax::parse(source);
    let element = find_jsx_element(&parsed.file).expect("jsx element");
    match &element.attributes[0].value {
        Some(ast::JsxAttributeValue::Text(text)) => {
            assert_eq!(text.value, "tab\\tliteral");
        }
        other => panic!("expected Text value, got {other:?}"),
    }
}

#[test]
fn missing_or_unquoted_attribute_value_is_diagnosed() {
    for source in ["fn f() { <A title= />; }", "fn f() { <A title=value />; }"] {
        let parsed = outou_syntax::parse(source);
        let element = find_jsx_element(&parsed.file).expect("jsx element");
        let title_attr = element
            .attributes
            .iter()
            .find(|a| a.name.name == "title")
            .unwrap_or_else(|| panic!("no `title` attribute: {:?}", element.attributes));
        assert!(
            matches!(title_attr.value, Some(ast::JsxAttributeValue::Error(_))),
            "source: {source}: {:?}",
            title_attr.value
        );
        let matching = parsed
            .diagnostics
            .iter()
            .filter(|d| d.message == INVALID_VALUE_MESSAGE)
            .count();
        assert_eq!(matching, 1, "source: {source}: {:?}", parsed.diagnostics);
    }
}

/// Issue #19: a sibling element after an attribute whose value is missing
/// must stay its own node, not be folded into the broken tag's attributes.
#[test]
fn sibling_after_missing_attribute_value_is_not_folded_into_the_broken_tag() {
    let source = "fn f() {\n    <section>\n        <div class=\n        <Greeting name=\"Outou\" />\n    </section>\n}\n";
    let parsed = outou_syntax::parse(source);
    let section = find_jsx_element(&parsed.file).expect("section element");
    let elements: Vec<&ast::JsxElement> = section
        .children
        .iter()
        .filter_map(|child| match child {
            ast::JsxChild::Element(element) => Some(element),
            _ => None,
        })
        .collect();
    assert_eq!(elements.len(), 2, "{:#?}", section.children);
    let names: Vec<&str> = elements[0]
        .attributes
        .iter()
        .map(|a| a.name.name.as_str())
        .collect();
    assert_eq!(names, ["class"], "broken tag swallowed its sibling");
    assert!(elements[0].children.is_empty());
    match &elements[1].open {
        ast::JsxTag::Named { name, .. } => assert_eq!(name.name, "Greeting"),
        other => panic!("expected `Greeting`, got {other:?}"),
    }
    let greeting: Vec<&str> = elements[1]
        .attributes
        .iter()
        .map(|a| a.name.name.as_str())
        .collect();
    assert_eq!(greeting, ["name"]);
}

/// Issue #19 follow-up: a tag ended by `<` must not leave JSX behind for
/// the function body's Rust text to carry verbatim (the Recovery output
/// would not parse). The full-output check is the `incomplete/` fixtures'.
#[test]
fn tag_ended_by_a_following_tag_leaves_no_jsx_in_rust_text() {
    let sources = [
        "fn f() {\n    <div class=\n    <Greeting />\n}\n",
        "fn f() {\n    <div class=\n    </div>\n}\n",
        "fn f() {\n    let x = <div class=\n    <Greeting />;\n    x\n}\n",
        "fn f() {\n    <section>\n        <div class=\n        </div>\n    </section>\n}\n",
    ];
    for source in sources {
        let parsed = outou_syntax::parse(source);
        assert_eq!(
            parsed.diagnostics.len(),
            1,
            "{source}: {:?}",
            parsed.diagnostics
        );
        for item in &parsed.file.items {
            let ast::Item::Function(f) = item else {
                continue;
            };
            for expr in f.body.statements.iter().chain(f.body.tail.as_deref()) {
                if let ast::Expr::Rust(rust) = expr {
                    assert!(
                        !rust.text.contains("</") && !rust.text.contains("/>"),
                        "{source}: JSX leaked into Rust text {:?}",
                        rust.text
                    );
                }
            }
        }
    }
}

// Issue #31: a raw identifier names the same attribute as the plain one
// (`r#type` is how `type` is written where Rust needs a raw identifier).

#[test]
fn a_raw_identifier_attribute_name_is_the_same_name_as_the_plain_one() {
    assert_eq!(ast::unraw_name("r#key"), "key");
    assert_eq!(ast::unraw_name("r#type"), "type");
    assert_eq!(ast::unraw_name("key"), "key");
    assert_eq!(ast::unraw_name("data-r#id"), "data-id");
    assert_eq!(ast::unraw_name("r#data-r#id"), "data-id");
    assert_eq!(ast::unraw_name("rust"), "rust");
}

#[test]
fn a_raw_spelling_of_an_attribute_is_a_duplicate_of_the_plain_one() {
    let source = "fn f() { <div key=\"a\" r#key=\"b\" r#type=\"x\" type=\"y\" />; }";
    let parsed = outou_syntax::parse(source);
    let messages: Vec<&str> = parsed
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(
        messages,
        [
            "duplicate attribute `key` on this tag",
            "duplicate attribute `type` on this tag"
        ],
        "{:?}",
        parsed.diagnostics
    );
}
