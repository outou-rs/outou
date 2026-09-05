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
