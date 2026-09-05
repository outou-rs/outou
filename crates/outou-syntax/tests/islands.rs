//! Islands (grammar §6) must keep every JSX node they contain, never
//! collapsing a multi-node island into one opaque Rust slice (H6). See
//! `docs/grammar.md` §9 for the round-trip contract these tests check:
//! `Island::parts` exactly partitions the island's content span.

use outou_syntax::ast;

fn parse_fn(body: &str) -> outou_syntax::Parsed {
    let source = format!("fn f() {{\n    {body}\n}}\n");
    outou_syntax::parse(&source)
}

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
            count_jsx_in_island(island, count);
        }
    }
    for child in &element.children {
        match child {
            ast::JsxChild::Expression(island) => count_jsx_in_island(island, count),
            ast::JsxChild::Element(nested) => count_jsx_in_element(nested, count),
            ast::JsxChild::Text(_) | ast::JsxChild::Error(_) => {}
        }
    }
}

fn count_jsx_in_island(island: &ast::Island, count: &mut usize) {
    for part in &island.parts {
        count_jsx_in_expr(part, count);
    }
}

fn total_jsx_count(file: &ast::File) -> usize {
    let mut count = 0;
    for item in &file.items {
        if let ast::Item::Function(f) = item {
            for expr in f.body.statements.iter().chain(f.body.tail.as_deref()) {
                count_jsx_in_expr(expr, &mut count);
            }
        }
    }
    count
}

fn any_rust_text_contains(file: &ast::File, needle: &str) -> bool {
    fn check_expr(expr: &ast::Expr, needle: &str) -> bool {
        match expr {
            ast::Expr::Rust(rs) => rs.text.contains(needle),
            ast::Expr::Jsx(el) => check_element(el, needle),
            ast::Expr::Error(_) => false,
        }
    }
    fn check_element(element: &ast::JsxElement, needle: &str) -> bool {
        element.attributes.iter().any(|attr| {
            matches!(&attr.value, Some(ast::JsxAttributeValue::Expression(island)) if check_island(island, needle))
        }) || element.children.iter().any(|child| match child {
            ast::JsxChild::Expression(island) => check_island(island, needle),
            ast::JsxChild::Element(nested) => check_element(nested, needle),
            ast::JsxChild::Text(_) | ast::JsxChild::Error(_) => false,
        })
    }
    fn check_island(island: &ast::Island, needle: &str) -> bool {
        island.parts.iter().any(|p| check_expr(p, needle))
    }
    file.items.iter().any(|item| match item {
        ast::Item::Function(f) => f
            .body
            .statements
            .iter()
            .chain(f.body.tail.as_deref())
            .any(|e| check_expr(e, needle)),
        _ => false,
    })
}

#[test]
fn island_keeps_every_jsx_node() {
    let cases: &[(&str, usize)] = &[
        ("<div>{ <A/> }</div>;", 2),
        ("<div>{ if c { <A/> } else { <B/> } }</div>;", 3),
        ("<div>{ items.iter().map(|i| <Row/>) }</div>;", 2),
        ("<div a={ let x = 1; <A/> }/>;", 2),
    ];
    for (body, expected_count) in cases {
        let parsed = parse_fn(body);
        let count = total_jsx_count(&parsed.file);
        assert_eq!(count, *expected_count, "body: {body}");
        assert!(!any_rust_text_contains(&parsed.file, "<A"), "body: {body}");
        assert!(
            !any_rust_text_contains(&parsed.file, "<Row"),
            "body: {body}"
        );
        assert!(!any_rust_text_contains(&parsed.file, "<B"), "body: {body}");
    }
}

fn assert_contiguous(spans: &[outou_sourcemap::Span], start: u32, end: u32, label: &str) {
    let mut cursor = start;
    for span in spans {
        assert_eq!(span.start, cursor, "{label}: gap/overlap before {span:?}");
        cursor = span.end;
    }
    assert_eq!(cursor, end, "{label}: does not reach expected end");
}

fn expr_span(expr: &ast::Expr) -> outou_sourcemap::Span {
    match expr {
        ast::Expr::Rust(rs) => rs.span,
        ast::Expr::Jsx(el) => el.span,
        ast::Expr::Error(e) => e.span,
    }
}

fn collect_islands(file: &ast::File, out: &mut Vec<ast::Island>) {
    fn walk_expr(expr: &ast::Expr, out: &mut Vec<ast::Island>) {
        if let ast::Expr::Jsx(el) = expr {
            walk_element(el, out);
        }
    }
    fn walk_element(element: &ast::JsxElement, out: &mut Vec<ast::Island>) {
        for attr in &element.attributes {
            if let Some(ast::JsxAttributeValue::Expression(island)) = &attr.value {
                out.push(island.clone());
                for part in &island.parts {
                    walk_expr(part, out);
                }
            }
        }
        for child in &element.children {
            match child {
                ast::JsxChild::Expression(island) => {
                    out.push(island.clone());
                    for part in &island.parts {
                        walk_expr(part, out);
                    }
                }
                ast::JsxChild::Element(nested) => walk_element(nested, out),
                ast::JsxChild::Text(_) | ast::JsxChild::Error(_) => {}
            }
        }
    }
    for item in &file.items {
        if let ast::Item::Function(f) = item {
            for expr in f.body.statements.iter().chain(f.body.tail.as_deref()) {
                walk_expr(expr, out);
            }
        }
    }
}

#[test]
fn island_parts_partition_the_content() {
    let cases = [
        "<div>{ <A/> }</div>;",
        "<div>{ if c { <A/> } else { <B/> } }</div>;",
        "<div>{ items.iter().map(|i| <Row/>) }</div>;",
        "<div a={ let x = 1; <A/> }/>;",
    ];
    for body in cases {
        let parsed = parse_fn(body);
        let mut islands = Vec::new();
        collect_islands(&parsed.file, &mut islands);
        assert!(!islands.is_empty(), "body: {body}");
        for island in &islands {
            let spans: Vec<_> = island.parts.iter().map(expr_span).collect();
            assert_contiguous(&spans, island.span.start, island.span.end, body);
        }
    }
}

#[test]
fn trivia_only_island_produces_no_child() {
    let cases = ["<A>{/* note */}</A>;", "<A>{ }</A>;"];
    for body in cases {
        let parsed = parse_fn(body);
        let element = find_jsx(&parsed.file).unwrap_or_else(|| panic!("no jsx in {body}"));
        assert!(element.children.is_empty(), "body: {body}");
        assert!(
            parsed.diagnostics.is_empty(),
            "body: {body}: {:?}",
            parsed.diagnostics
        );
    }
}

#[test]
fn empty_attribute_value_island_is_diagnosed() {
    let cases = ["<A x={/* note */}/>;", "<A x={ }/>;"];
    for body in cases {
        let parsed = parse_fn(body);
        let element = find_jsx(&parsed.file).unwrap_or_else(|| panic!("no jsx in {body}"));
        let attr = &element.attributes[0];
        assert!(
            matches!(attr.value, Some(ast::JsxAttributeValue::Error(_))),
            "body: {body}: {:?}",
            attr.value
        );
        assert_eq!(
            parsed.diagnostics.len(),
            1,
            "body: {body}: {:?}",
            parsed.diagnostics
        );
        assert_eq!(
            parsed.diagnostics[0].message, "expected an expression for the value of attribute `x`",
            "body: {body}"
        );
    }
}

fn find_jsx(file: &ast::File) -> Option<&ast::JsxElement> {
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
