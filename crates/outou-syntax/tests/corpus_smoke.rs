//! A small in-repo stand-in for the external corpus (issue #12): plain
//! Rust snippets, deliberately chosen from the categories `cargo xtask
//! corpus test` reports on (macro token trees, qualified paths, raw
//! strings, lifetimes, generics, comparison chains that could be
//! mistaken for a JSX tag), asserting `outou_syntax::parse` finds no JSX
//! and raises no diagnostics on any of them. This runs in the normal
//! `cargo test --workspace` suite, unlike the external corpus (which
//! needs `cargo xtask corpus fetch` first and is exercised by the
//! nightly job instead).

use outou_syntax::ast;

/// (label, source). Every source is plain Rust with no JSX in it; a
/// diagnostic or a detected JSX element on any of these is a Outou
/// false-positive detection bug, not a fixture-authoring mistake.
const SNIPPETS: &[(&str, &str)] = &[
    (
        "raw-string-with-angle-brackets",
        r####"fn f() -> &'static str { r"a < b" }"####,
    ),
    (
        "raw-string-with-hash-and-quotes",
        r####"fn f() -> &'static str { r#"contains "quotes" and < > too"# }"####,
    ),
    (
        "comparison-chain",
        "fn f(a: i32, b: i32, c: i32) -> bool { a < b && b > c }",
    ),
    (
        "comparison-chain-parenthesized",
        "fn f(a: i32, b: i32, c: i32, d: i32) -> bool { (a < b) == (c > d) }",
    ),
    (
        "turbofish",
        "fn f() -> Vec<i32> { Vec::<i32>::new() }",
    ),
    (
        "nested-turbofish",
        "fn f() -> Vec<Vec<i32>> { Vec::<Vec<i32>>::new() }",
    ),
    (
        "higher-ranked-trait-bound",
        "fn f<T>() where T: for<'a> Fn(&'a str) -> bool {}",
    ),
    (
        "impl-with-generic-and-hrtb-bound",
        "trait Trait<'a> {} struct Foo<T>(T); impl<T: Clone> Trait<'_> for Foo<T> where T: for<'a> Fn(&'a str) {}",
    ),
    (
        "macro-body-with-jsx-like-tokens",
        r#"macro_rules! m { () => { let x = "<div>not jsx</div>"; } } fn f() { m!(); }"#,
    ),
    (
        "macro-invocation-with-comparison",
        "macro_rules! m { ($x:expr) => { $x } } fn f() -> bool { m!(1 < 2) }",
    ),
    (
        "byte-string",
        r#"fn f() -> &'static [u8] { b"bytes < more bytes" }"#,
    ),
    (
        "byte-char-literal",
        "fn f() -> u8 { b'<' }",
    ),
    (
        "char-literals",
        "fn f() -> (char, char) { ('<', '>') }",
    ),
    (
        "lifetimes",
        "fn f<'a>(x: &'a str) -> &'a str { x }",
    ),
    (
        "struct-with-lifetime-and-generic",
        "struct Pair<'a, T> { a: &'a T, b: &'a T }",
    ),
    (
        "nested-generics",
        "use std::collections::HashMap; fn f() -> Vec<Vec<HashMap<String, i32>>> { Vec::new() }",
    ),
    (
        "qualified-path-method",
        "trait Trait { fn f() -> i32; } struct Foo; impl Trait for Foo { fn f() -> i32 { 0 } } fn g() -> i32 { <Foo as Trait>::f() }",
    ),
    (
        "qualified-path-assoc-const",
        "trait Trait { const N: i32; } struct Foo; impl Trait for Foo { const N: i32 = 1; } fn g() -> i32 { <Foo as Trait>::N }",
    ),
    (
        "chained-comparisons-in-if",
        "fn f(x: i32, y: i32, z: i32, w: i32) -> bool { if x < y && y < z && z < w { true } else { false } }",
    ),
    (
        "generic-fn-with-where-and-comparison-body",
        "fn f<T: PartialOrd>(a: T, b: T) -> bool where T: Copy { a < b }",
    ),
];

#[test]
fn no_snippet_produces_diagnostics_or_detects_jsx() {
    assert!(
        SNIPPETS.len() >= 20,
        "expected at least 20 corpus-smoke snippets, found {}",
        SNIPPETS.len()
    );
    for (label, source) in SNIPPETS {
        let parsed = outou_syntax::parse(source);
        assert!(
            parsed.diagnostics.is_empty(),
            "{label}: expected no diagnostics, got {:?}",
            parsed.diagnostics
        );
        let elements = collect_jsx_elements(&parsed.file);
        assert!(
            elements.is_empty(),
            "{label}: expected no JSX elements, found {} (first span: {:?})",
            elements.len(),
            elements[0].span
        );
    }
}

fn collect_jsx_elements(file: &ast::File) -> Vec<&ast::JsxElement> {
    let mut out = Vec::new();
    for item in &file.items {
        collect_in_item(item, &mut out);
    }
    out
}

fn collect_in_item<'a>(item: &'a ast::Item, out: &mut Vec<&'a ast::JsxElement>) {
    match item {
        ast::Item::Function(f) => {
            for expr in f.body.statements.iter().chain(f.body.tail.as_deref()) {
                collect_in_expr(expr, out);
            }
        }
        ast::Item::Module(m) => {
            for inner in m.items.iter().flatten() {
                collect_in_item(inner, out);
            }
        }
        ast::Item::Rust(r) => {
            for part in &r.parts {
                collect_in_expr(part, out);
            }
        }
        ast::Item::Error(_) => {}
    }
}

fn collect_in_expr<'a>(expr: &'a ast::Expr, out: &mut Vec<&'a ast::JsxElement>) {
    if let ast::Expr::Jsx(element) = expr {
        collect_in_element(element, out);
    }
}

fn collect_in_element<'a>(element: &'a ast::JsxElement, out: &mut Vec<&'a ast::JsxElement>) {
    out.push(element);
    for attr in &element.attributes {
        if let Some(ast::JsxAttributeValue::Expression(island)) = &attr.value {
            for part in &island.parts {
                collect_in_expr(part, out);
            }
        }
    }
    for child in &element.children {
        match child {
            ast::JsxChild::Expression(island) => {
                for part in &island.parts {
                    collect_in_expr(part, out);
                }
            }
            ast::JsxChild::Element(nested) => collect_in_element(nested, out),
            ast::JsxChild::Text(_) | ast::JsxChild::Error(_) => {}
        }
    }
}
