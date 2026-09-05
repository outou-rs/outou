//! Item-level JSX scanning (H3, decision D1): JSX must be found wherever
//! Rust allows an expression, not only inside a free function's body —
//! `const`/`static` initializers, `impl`/`trait` method bodies, and nested
//! items — while a `fn`-pointer type in item position (`type C = fn();`)
//! must never be mistaken for a function item.

use outou_syntax::ast;

fn count_jsx_in_expr(expr: &ast::Expr, count: &mut usize) {
    match expr {
        ast::Expr::Jsx(element) => {
            *count += 1;
            count_jsx_in_element(element, count);
        }
        ast::Expr::Rust(_) | ast::Expr::Error(_) => {}
    }
}

fn count_jsx_in_element(element: &ast::JsxElement, count: &mut usize) {
    for attr in &element.attributes {
        if let Some(ast::JsxAttributeValue::Expression(island)) = &attr.value {
            for part in &island.parts {
                count_jsx_in_expr(part, count);
            }
        }
    }
    for child in &element.children {
        match child {
            ast::JsxChild::Expression(island) => {
                for part in &island.parts {
                    count_jsx_in_expr(part, count);
                }
            }
            ast::JsxChild::Element(nested) => count_jsx_in_element(nested, count),
            ast::JsxChild::Text(_) | ast::JsxChild::Error(_) => {}
        }
    }
}

fn count_jsx_in_item(item: &ast::Item, count: &mut usize) {
    match item {
        ast::Item::Function(f) => {
            for expr in f.body.statements.iter().chain(f.body.tail.as_deref()) {
                count_jsx_in_expr(expr, count);
            }
        }
        ast::Item::Module(m) => {
            for inner in m.items.iter().flatten() {
                count_jsx_in_item(inner, count);
            }
        }
        ast::Item::Rust(r) => {
            for part in &r.parts {
                count_jsx_in_expr(part, count);
            }
        }
        ast::Item::Error(_) => {}
    }
}

fn total_jsx_count(file: &ast::File) -> usize {
    let mut count = 0;
    for item in &file.items {
        count_jsx_in_item(item, &mut count);
    }
    count
}

fn any_rust_item_part_text_contains(file: &ast::File, needle: &str) -> bool {
    fn check_expr(expr: &ast::Expr, needle: &str) -> bool {
        matches!(expr, ast::Expr::Rust(rs) if rs.text.contains(needle))
    }
    file.items.iter().any(|item| match item {
        ast::Item::Rust(r) => r.parts.iter().any(|p| check_expr(p, needle)),
        _ => false,
    })
}

#[test]
fn jsx_is_found_in_every_expression_bearing_context() {
    let cases = [
        "const VIEW: Element = <A/>;",
        "static S: E = <A/>;",
        "impl View { fn render(&self) -> Element { <A/> } }",
        "trait T { fn d(&self) -> E { <A/> } }",
        "type Callback = fn() -> ();\nfn view() -> Element { <A/> }",
    ];
    for source in cases {
        let parsed = outou_syntax::parse(source);
        let count = total_jsx_count(&parsed.file);
        assert_eq!(count, 1, "source: {source} -> {:#?}", parsed.file);
        assert!(
            !any_rust_item_part_text_contains(&parsed.file, "<A"),
            "source: {source}"
        );
    }
}

#[test]
fn fn_pointer_type_is_not_a_function_item() {
    let source = "type C = fn();\nstruct S { a: u8 }\n#[component] fn view() -> E { <A/> }";
    let parsed = outou_syntax::parse(source);
    let functions: Vec<&ast::Function> = parsed
        .file
        .items
        .iter()
        .filter_map(|item| match item {
            ast::Item::Function(f) => Some(f),
            _ => None,
        })
        .collect();
    assert_eq!(
        functions.len(),
        1,
        "expected exactly one function item: {:#?}",
        parsed.file
    );
    assert_eq!(functions[0].name.name, "view");
    assert!(functions[0].is_component);
}

// ---------------------------------------------------------------------
// Item 21 (L2): attribute matching must compare the attribute's exact
// meta path, not search its text as a substring — `#[not_component]`
// contains "component" and `#[not_path = "x"]` contains "path", but
// neither is the attribute it names.
// ---------------------------------------------------------------------

#[test]
fn attribute_path_is_matched_exactly() {
    let parsed = outou_syntax::parse("#[not_component]\nfn f() {}");
    let function = parsed
        .file
        .items
        .iter()
        .find_map(|item| match item {
            ast::Item::Function(f) => Some(f),
            _ => None,
        })
        .expect("function item");
    assert!(!function.is_component, "{:#?}", function);

    let parsed = outou_syntax::parse("#[not_path = \"x\"]\nmod m;");
    let module = parsed
        .file
        .items
        .iter()
        .find_map(|item| match item {
            ast::Item::Module(m) => Some(m),
            _ => None,
        })
        .expect("module item");
    assert_eq!(module.path, None, "{:#?}", module);

    let parsed = outou_syntax::parse("#[path = \"x\"]\nmod m;");
    let module = parsed
        .file
        .items
        .iter()
        .find_map(|item| match item {
            ast::Item::Module(m) => Some(m),
            _ => None,
        })
        .expect("module item");
    assert_eq!(module.path.as_deref(), Some("x"), "{:#?}", module);
}

// ---------------------------------------------------------------------
// Item 8 (LOW-11): a body-less `fn` signature (ending in `;`, as inside an
// `extern` block, or any other bodyless declaration) must stop its
// signature scan at that `;` rather than hunting for the next `{`
// anywhere in the file, which used to belong to a completely unrelated
// following item.
// ---------------------------------------------------------------------

#[test]
fn bodyless_fn_signature_does_not_swallow_the_next_item() {
    let source = "extern \"C\" fn foo();\nfn g() -> Element { <a/> }";
    let parsed = outou_syntax::parse(source);
    let functions: Vec<&ast::Function> = parsed
        .file
        .items
        .iter()
        .filter_map(|item| match item {
            ast::Item::Function(f) => Some(f),
            _ => None,
        })
        .collect();
    let g = functions
        .iter()
        .find(|f| f.name.name == "g")
        .unwrap_or_else(|| panic!("no `g` function item found: {:#?}", parsed.file));
    assert_eq!(total_jsx_count(&parsed.file), 1, "{:#?}", parsed.file);
    assert!(g.body.close.is_some(), "{:#?}", g.body);
}

#[test]
fn generic_rust_produces_no_jsx() {
    let cases = [
        "struct S<T> { a: Vec<T> }",
        "impl<T: Clone> Tr for S<T> where T: Into<u8> {}",
        "fn f() -> Box<dyn Fn() -> Vec<u8>> { todo!() }",
        "static L: Lazy<Vec<u8>> = Lazy::new(Vec::new);",
        "fn f() { f::<T>(); }",
        "fn f() { x < y && y > z; }",
    ];
    for source in cases {
        let parsed = outou_syntax::parse(source);
        let count = total_jsx_count(&parsed.file);
        assert_eq!(count, 0, "source: {source} -> {:#?}", parsed.file);
        assert!(
            parsed.diagnostics.is_empty(),
            "source: {source}: {:?}",
            parsed.diagnostics
        );
    }
}
