//! Attribute opacity (grammar §3): the lexer MUST NOT enter a JSX mode
//! inside an attribute's token tree, no matter what trivia (whitespace,
//! comments) precedes the `#`. H5: `try_attribute` used to require the
//! literal byte `#` at the exact scan position, unlike its trivia-tolerant
//! siblings `try_macro_rules`/`try_macro_invocation`, so a `#[...]`
//! preceded by a comment inside a region scan was not recognized as
//! opaque and its content was scanned for JSX like ordinary Rust.

use outou_syntax::ast;

fn collect_jsx_names(expr: &ast::Expr, out: &mut Vec<String>) {
    if let ast::Expr::Jsx(element) = expr {
        out.push(element_name(element));
        for attr in &element.attributes {
            if let Some(ast::JsxAttributeValue::Expression(island)) = &attr.value {
                for part in &island.parts {
                    collect_jsx_names(part, out);
                }
            }
        }
        for child in &element.children {
            match child {
                ast::JsxChild::Expression(island) => {
                    for part in &island.parts {
                        collect_jsx_names(part, out);
                    }
                }
                ast::JsxChild::Element(nested) => {
                    collect_jsx_names(&ast::Expr::Jsx(nested.clone()), out);
                }
                ast::JsxChild::Text(_) | ast::JsxChild::Error(_) => {}
            }
        }
    }
}

fn element_name(element: &ast::JsxElement) -> String {
    match &element.open {
        ast::JsxTag::Named { name, .. } => name.name.clone(),
        ast::JsxTag::Incomplete(tag) => tag
            .name
            .as_ref()
            .map(|n| n.name.clone())
            .unwrap_or_default(),
    }
}

/// All JSX element tag names found anywhere in `source`'s function bodies,
/// in source order, recursing into nested elements and islands.
fn all_jsx_names(source: &str) -> Vec<String> {
    let parsed = outou_syntax::parse(source);
    let mut out = Vec::new();
    for item in &parsed.file.items {
        if let ast::Item::Function(f) = item {
            for expr in f.body.statements.iter().chain(f.body.tail.as_deref()) {
                collect_jsx_names(expr, &mut out);
            }
        }
    }
    out
}

#[test]
fn attributes_are_opaque_after_trivia() {
    let cases = [
        "fn f() {\n    #[custom(<B/>)]\n    let x = 1;\n}",
        "fn f() {\n    /* c */ #[custom(<B/>)]\n    let x = 1;\n}",
        "fn f() {\n    // c\n    #[custom(<B/>)]\n    let x = 1;\n}",
    ];
    for source in cases {
        let names = all_jsx_names(source);
        assert!(names.is_empty(), "source: {source}: found {names:?}");
    }
}

/// The same opacity property must hold inside an island (`scan_region` is
/// shared by function bodies and islands), and a comment right before the
/// attribute must not cause the opaque skip to over- or under-consume: the
/// enclosing `<div>` is still found, the JSX-shaped text inside the
/// attribute's payload is not, and the ordinary statement that follows the
/// attribute is still scanned normally.
#[test]
fn attribute_inside_an_island_stays_opaque_after_a_comment() {
    let source = "fn f() { <div>{ /* c */ #[cfg(<B/>)] let y = 1; }</div>; }";
    let names = all_jsx_names(source);
    assert_eq!(names, vec!["div".to_string()], "source: {source}");
}
