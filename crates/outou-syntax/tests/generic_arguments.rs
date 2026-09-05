//! Grammar §4 rule 3 / §7.2: a tag with explicit generic arguments
//! (`<List<T> ... />`) commits to JSX, but attribute parsing must resume
//! right after the skipped `<...>` generic-argument list, not after the
//! whole element (HIGH-2, issue #4 fix list item 1). `rule3_scan` decides
//! Rust-vs-JSX by scanning from the *whole tag's* own `<` to the point its
//! angle depth returns to zero — which, for a self-closing tag, is only
//! reached at the tag's own `/>` — but that same position must never be
//! used as the tag parser's resume point, or every attribute is swallowed
//! into the (bogus) element span.

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

#[test]
fn self_closing_tag_with_generics_resumes_attribute_parsing_after_the_generic_list() {
    let source = "fn f() { <List<T> x={1} />; }";
    let parsed = outou_syntax::parse(source);
    let element = find_jsx_element(&parsed.file).expect("jsx element");

    // The element's span must end exactly at `/>`, not swallow the `;`
    // and whatever follows it.
    let end = element.span.end as usize;
    assert_eq!(&source[..end], "fn f() { <List<T> x={1} />");
    assert!(source[end..].starts_with(';'), "{:?}", &source[end..]);

    // The attribute must actually be found, not lost.
    assert_eq!(element.attributes.len(), 1, "{:?}", element.attributes);
    let attr = &element.attributes[0];
    assert_eq!(attr.name.name, "x");
    match &attr.value {
        Some(ast::JsxAttributeValue::Expression(island)) => {
            assert_eq!(
                &source[island.span.start as usize..island.span.end as usize],
                "1"
            );
        }
        other => panic!("expected an expression value, got {other:?}"),
    }

    // Exactly one diagnostic: the generic-arguments-not-supported message.
    assert_eq!(
        parsed.diagnostics.len(),
        1,
        "expected exactly one diagnostic, got {:?}",
        parsed.diagnostics
    );
    assert_eq!(
        parsed.diagnostics[0].message,
        "generic arguments on a tag are not supported in Phase 0"
    );
}

#[test]
fn open_tag_with_generics_still_finds_its_children_and_close() {
    let source = "fn f() { <List<T>>hello</List>; }";
    let parsed = outou_syntax::parse(source);
    let element = find_jsx_element(&parsed.file).expect("jsx element");

    assert_eq!(element.children.len(), 1, "{:?}", element.children);
    match &element.children[0] {
        ast::JsxChild::Text(text) => assert_eq!(text.value, "hello"),
        other => panic!("expected Text, got {other:?}"),
    }
    assert!(
        matches!(&element.close, Some(ast::JsxTag::Named { name, .. }) if name.name == "List"),
        "{:?}",
        element.close
    );
    // Only the generic-arguments diagnostic; the close resolves cleanly.
    assert_eq!(
        parsed.diagnostics.len(),
        1,
        "expected exactly one diagnostic, got {:?}",
        parsed.diagnostics
    );
}

#[test]
fn generic_type_with_path_separator_still_stays_rust() {
    let source = "fn f() { <Vec<i32>>::new(); }";
    let parsed = outou_syntax::parse(source);
    assert!(
        find_jsx_element(&parsed.file).is_none(),
        "{:#?}",
        parsed.file
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
}
