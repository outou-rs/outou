//! Golden fixture tests (issue #4, Gate 1).
//!
//! - `tests/fixtures/formatting/<case>/`: parses `input.rsx`, renders the
//!   children of the function's returned JSX element per that directory's
//!   `README.md` format, and compares against `expected.txt`.
//! - `tests/fixtures/{incomplete,diagnostics}/*.rsx`: compares
//!   `Parsed::render_diagnostics` against the sibling `.expected` file,
//!   verbatim.
//! - Every fixture is additionally checked for: no panic, and a coverage
//!   invariant standing in for "concatenating the source text of all
//!   leaves in order reproduces the input" (see `assert_full_coverage`).

use std::fs;
use std::panic;
use std::path::{Path, PathBuf};

use outou_sourcemap::Span;
use outou_syntax::ast;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/outou-syntax has a parent")
        .parent()
        .expect("crates/ has a parent")
        .to_path_buf()
}

fn fixtures_dir(sub: &str) -> PathBuf {
    repo_root().join("tests").join("fixtures").join(sub)
}

// ---------------------------------------------------------------------
// formatting/
// ---------------------------------------------------------------------

#[test]
fn formatting_fixtures_match_expected_children() {
    let dir = fixtures_dir("formatting");
    let mut cases: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    cases.sort();
    assert!(
        !cases.is_empty(),
        "expected at least one formatting fixture"
    );

    for case in cases {
        let name = case.file_name().unwrap().to_string_lossy().to_string();
        let input_path = case.join("input.rsx");
        let expected_path = case.join("expected.txt");
        let source = fs::read_to_string(&input_path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", input_path.display()));
        let expected = fs::read_to_string(&expected_path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", expected_path.display()));

        let parsed = panic::catch_unwind(|| outou_syntax::parse(&source))
            .unwrap_or_else(|_| panic!("parse panicked on formatting/{name}"));
        assert_full_coverage(&parsed.file, &source, &name);

        let element = find_returned_jsx_element(&parsed.file).unwrap_or_else(|| {
            panic!("formatting/{name}: no top-level returned JSX element found")
        });
        let mut out = String::new();
        render_children(&element.children, &source, 0, &mut out);

        assert_eq!(
            out, expected,
            "formatting/{name}: rendered children mismatch"
        );
    }
}

/// Finds the JSX element returned by the fixture's `fn App` (its body's
/// tail expression).
fn find_returned_jsx_element(file: &ast::File) -> Option<&ast::JsxElement> {
    for item in &file.items {
        if let ast::Item::Function(function) = item {
            if let Some(tail) = &function.body.tail {
                if let ast::Expr::Jsx(element) = tail.as_ref() {
                    return Some(element);
                }
            }
        }
    }
    None
}

fn render_children(children: &[ast::JsxChild], source: &str, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    for child in children {
        match child {
            ast::JsxChild::Text(text) => {
                out.push_str(&indent);
                out.push_str("Text(\"");
                out.push_str(&escape_rust_string(&text.value));
                out.push_str("\")\n");
            }
            ast::JsxChild::Expression(island) => {
                out.push_str(&indent);
                out.push_str("Expr(");
                out.push_str(&source[island.span.start as usize..island.span.end as usize]);
                out.push_str(")\n");
            }
            ast::JsxChild::Element(element) => {
                out.push_str(&indent);
                out.push_str("Element(");
                out.push_str(&element_name(element));
                out.push_str(")\n");
                render_children(&element.children, source, depth + 1, out);
            }
            ast::JsxChild::Error(_) => {
                out.push_str(&indent);
                out.push_str("Error\n");
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

fn expr_span(expr: &ast::Expr) -> Span {
    match expr {
        ast::Expr::Rust(rs) => rs.span,
        ast::Expr::Jsx(el) => el.span,
        ast::Expr::Error(e) => e.span,
    }
}

fn escape_rust_string(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                out.push_str(&format!("\\u{{{:x}}}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------
// incomplete/, diagnostics/
// ---------------------------------------------------------------------

#[test]
fn incomplete_fixtures_match_expected_diagnostics() {
    check_diagnostics_dir("incomplete");
}

#[test]
fn diagnostics_fixtures_match_expected_diagnostics() {
    check_diagnostics_dir("diagnostics");
}

fn check_diagnostics_dir(sub: &str) {
    let dir = fixtures_dir(sub);
    let mut rsx_files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("rsx"))
        .collect();
    rsx_files.sort();
    assert!(
        !rsx_files.is_empty(),
        "expected at least one .rsx fixture in {sub}"
    );

    for rsx_path in rsx_files {
        let file_name = rsx_path.file_name().unwrap().to_string_lossy().to_string();
        let expected_path = rsx_path.with_extension("expected");
        let source = fs::read_to_string(&rsx_path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", rsx_path.display()));
        let expected = fs::read_to_string(&expected_path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", expected_path.display()));

        let parsed = panic::catch_unwind(|| outou_syntax::parse(&source))
            .unwrap_or_else(|_| panic!("parse panicked on {sub}/{file_name}"));
        assert_full_coverage(&parsed.file, &source, &file_name);

        let rendered = parsed.render_diagnostics(&file_name);
        assert_eq!(
            rendered.trim_end(),
            expected.trim_end(),
            "{sub}/{file_name}: diagnostics mismatch"
        );
    }
}

// ---------------------------------------------------------------------
// Span coverage ("leaves reproduce the input"), for every fixture.
// ---------------------------------------------------------------------

/// Asserts that every item's span, in source order, exactly partitions
/// `[0, source.len())` with no gap and no overlap, and recurses into
/// function bodies and JSX elements to check the same invariant for their
/// children. Since every span's text is a verbatim slice of `source`,
/// full coverage with no gaps or overlaps is equivalent to "concatenating
/// the source text of all leaves in source order reproduces the input".
fn assert_full_coverage(file: &ast::File, source: &str, case: &str) {
    let spans: Vec<Span> = file.items.iter().map(item_span).collect();
    assert_contiguous(&spans, 0, source.len() as u32, case, "top-level items");
    for item in &file.items {
        check_item(item, source, case);
    }
}

fn check_item(item: &ast::Item, source: &str, case: &str) {
    match item {
        ast::Item::Function(function) => {
            let block = &function.body;
            if block.span.start == block.span.end {
                // A synthetic, zero-width block (`item.rs`'s bodyless-`fn`
                // recovery: no `{` was ever found, so there is nothing to
                // check coverage of; MEDIUM-6, issue #4 fix list item 4).
                assert!(
                    block.statements.is_empty() && block.tail.is_none(),
                    "{case}: zero-width block unexpectedly has statements"
                );
                return;
            }
            let mut spans: Vec<Span> = function.statements_and_tail_spans();
            spans.sort_by_key(|s| s.start);
            let inner_start = block.span.start + 1;
            // `Block::close` — set only when the region scanner actually
            // found this block's own matching `}` — is the ground truth
            // for where its content ends, never a sniff of whether
            // `source` happens to end in a `}` byte (MEDIUM-6): a broken
            // construct nested inside the block can itself swallow the
            // source's last `}` without that being this block's own
            // close.
            let inner_end = match block.close {
                Some(close) => close.start,
                None => block.span.end,
            };
            assert_contiguous(&spans, inner_start, inner_end, case, "function body");
            for expr in function
                .body
                .statements
                .iter()
                .chain(function.body.tail.as_deref())
            {
                check_expr(expr, source, case);
            }
        }
        ast::Item::Module(module) => {
            if let Some(items) = &module.items {
                for inner in items {
                    check_item(inner, source, case);
                }
            }
        }
        ast::Item::Rust(rust_item) => {
            let spans: Vec<Span> = rust_item.parts.iter().map(expr_span).collect();
            assert_contiguous(
                &spans,
                rust_item.span.start,
                rust_item.span.end,
                case,
                "item-level Rust run",
            );
            for part in &rust_item.parts {
                check_expr(part, source, case);
            }
        }
        ast::Item::Error(_) => {}
    }
}

fn check_expr(expr: &ast::Expr, source: &str, case: &str) {
    if let ast::Expr::Jsx(element) = expr {
        check_element(element, source, case);
    }
}

fn check_element(element: &ast::JsxElement, source: &str, case: &str) {
    for attr in &element.attributes {
        if let Some(ast::JsxAttributeValue::Expression(island)) = &attr.value {
            check_island(island, source, case);
        }
    }
    for child in &element.children {
        match child {
            ast::JsxChild::Expression(island) => check_island(island, source, case),
            ast::JsxChild::Element(nested) => check_element(nested, source, case),
            ast::JsxChild::Text(_) | ast::JsxChild::Error(_) => {}
        }
    }
}

/// Island coverage (grammar §9, D3): `parts` must exactly partition the
/// island's content span, and any JSX nested inside a part is checked
/// recursively.
fn check_island(island: &ast::Island, source: &str, case: &str) {
    let spans: Vec<Span> = island.parts.iter().map(expr_span).collect();
    assert_contiguous(&spans, island.span.start, island.span.end, case, "island");
    for part in &island.parts {
        check_expr(part, source, case);
    }
}

fn item_span(item: &ast::Item) -> Span {
    match item {
        ast::Item::Function(f) => f.span,
        ast::Item::Module(m) => m.span,
        ast::Item::Rust(r) => r.span,
        ast::Item::Error(e) => e.span,
    }
}

fn assert_contiguous(spans: &[Span], start: u32, end: u32, case: &str, what: &str) {
    let mut cursor = start;
    for span in spans {
        assert_eq!(
            span.start, cursor,
            "{case}: {what}: gap or overlap before {span:?}"
        );
        cursor = span.end;
    }
    assert_eq!(
        cursor, end,
        "{case}: {what}: does not reach the expected end"
    );
}

// ---------------------------------------------------------------------
// Span exactness for the splicing contract (M9, decision D3, issue #4
// fix list item 16).
// ---------------------------------------------------------------------

/// Every JSX element's span must be the exact byte range of that element
/// — never widened over adjacent trivia (D3 point 1). Checked over every
/// `formatting/` fixture (all well-formed JSX) plus an explicit table
/// covering M9's repro and its variants. `incomplete/` and `diagnostics/`
/// fixtures are deliberately broken (an element cut short by `}` or end
/// of input never sees its own `>` at all) and are exercised by
/// `assert_full_coverage`'s gap/overlap check instead, not this
/// "starts-with-`<`-ends-with-`>`" shape check, which assumes a complete
/// element.
#[test]
fn jsx_element_spans_are_exact() {
    let mut sources: Vec<(String, String)> = Vec::new();
    let dir = fixtures_dir("formatting");
    for entry in fs::read_dir(&dir).unwrap_or_else(|e| panic!("reading {}: {e}", dir.display())) {
        let path = entry.expect("dir entry").path();
        if !path.is_dir() {
            continue;
        }
        let input_path = path.join("input.rsx");
        if let Ok(source) = fs::read_to_string(&input_path) {
            sources.push((input_path.display().to_string(), source));
        }
    }
    for (label, source) in [
        (
            "m9-attribute-island",
            "fn f() { <div id={x}>\n hi\n</div>; }",
        ),
        ("m9-nested-element", "fn f() { <a><b>\n hi\n</b></a>; }"),
        ("m9-self-closing", "fn f() { <a/>\n; }"),
    ] {
        sources.push((label.to_string(), source.to_string()));
    }

    assert!(!sources.is_empty());
    for (label, source) in sources {
        let parsed = outou_syntax::parse(&source);
        let mut elements = Vec::new();
        collect_jsx_elements(&parsed.file, &mut elements);
        assert!(!elements.is_empty(), "{label}: no JSX elements found");
        for element in elements {
            let text = &source[element.span.start as usize..element.span.end as usize];
            assert!(
                text.starts_with('<'),
                "{label}: {text:?} does not start with `<`"
            );
            assert!(
                text.ends_with('>'),
                "{label}: {text:?} does not end with `>`"
            );
        }
    }
}

fn collect_jsx_elements<'a>(file: &'a ast::File, out: &mut Vec<&'a ast::JsxElement>) {
    for item in &file.items {
        collect_jsx_elements_in_item(item, out);
    }
}

fn collect_jsx_elements_in_item<'a>(item: &'a ast::Item, out: &mut Vec<&'a ast::JsxElement>) {
    match item {
        ast::Item::Function(f) => {
            for expr in f.body.statements.iter().chain(f.body.tail.as_deref()) {
                collect_jsx_elements_in_expr(expr, out);
            }
        }
        ast::Item::Module(m) => {
            for inner in m.items.iter().flatten() {
                collect_jsx_elements_in_item(inner, out);
            }
        }
        ast::Item::Rust(r) => {
            for part in &r.parts {
                collect_jsx_elements_in_expr(part, out);
            }
        }
        ast::Item::Error(_) => {}
    }
}

fn collect_jsx_elements_in_expr<'a>(expr: &'a ast::Expr, out: &mut Vec<&'a ast::JsxElement>) {
    if let ast::Expr::Jsx(element) = expr {
        collect_jsx_elements_in_element(element, out);
    }
}

fn collect_jsx_elements_in_element<'a>(
    element: &'a ast::JsxElement,
    out: &mut Vec<&'a ast::JsxElement>,
) {
    out.push(element);
    for attr in &element.attributes {
        if let Some(ast::JsxAttributeValue::Expression(island)) = &attr.value {
            for part in &island.parts {
                collect_jsx_elements_in_expr(part, out);
            }
        }
    }
    for child in &element.children {
        match child {
            ast::JsxChild::Expression(island) => {
                for part in &island.parts {
                    collect_jsx_elements_in_expr(part, out);
                }
            }
            ast::JsxChild::Element(nested) => collect_jsx_elements_in_element(nested, out),
            ast::JsxChild::Text(_) | ast::JsxChild::Error(_) => {}
        }
    }
}

/// The Phase 0 round-trip contract itself (D3, `crates/outou-syntax/README.md`):
/// walking the tree and splicing each node's own source slice — verbatim
/// for Rust, `source[element.span]` for JSX — must reproduce the original
/// file exactly, for every fixture.
#[test]
fn source_round_trips_by_splicing() {
    let mut sources: Vec<(String, String)> = Vec::new();
    for sub in ["formatting", "incomplete", "diagnostics"] {
        let dir = fixtures_dir(sub);
        collect_rsx_sources(&dir, &mut sources);
    }
    assert!(!sources.is_empty());
    for (label, source) in sources {
        let parsed = outou_syntax::parse(&source);
        let rebuilt = rebuild_source(&parsed.file, &source);
        assert_eq!(rebuilt, parsed.source, "{label}: source did not round-trip");
    }
}

fn collect_rsx_sources(dir: &Path, out: &mut Vec<(String, String)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            collect_rsx_sources(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rsx") {
            if let Ok(source) = fs::read_to_string(&path) {
                out.push((path.display().to_string(), source));
            }
        }
    }
}

fn rebuild_source(file: &ast::File, source: &str) -> String {
    let mut out = String::new();
    for item in &file.items {
        rebuild_item(item, source, &mut out);
    }
    out
}

fn rebuild_item(item: &ast::Item, source: &str, out: &mut String) {
    match item {
        ast::Item::Function(f) => {
            // `f.span` starts before any leading attributes/doc comments;
            // `f.signature.span` starts at `fn` itself. The gap between
            // them is exactly that attribute/trivia prefix, contiguous in
            // the source (`Function::attributes` spans do not need to be
            // spliced separately from it).
            out.push_str(&source[f.span.start as usize..f.signature.span.start as usize]);
            out.push_str(slice(source, f.signature.span));
            rebuild_block(&f.body, source, out);
        }
        // A module's own header (attributes, `mod name`, and either `;` or
        // `{`) is not itself modeled as a separate span, so its whole
        // span is spliced verbatim; this is still lossless (D3 point 3
        // only requires *some* partition that reconstructs the source,
        // and modules are not this item's focus — item-level JSX inside a
        // module's own items is exercised through `Item::Rust` and
        // `Item::Function`, both reachable from `module.items`).
        ast::Item::Module(m) => {
            out.push_str(slice(source, m.span));
        }
        ast::Item::Rust(r) => {
            for part in &r.parts {
                rebuild_expr(part, source, out);
            }
        }
        ast::Item::Error(e) => out.push_str(slice(source, e.span)),
    }
}

fn rebuild_block(block: &ast::Block, source: &str, out: &mut String) {
    if block.span.start == block.span.end {
        // A synthetic, zero-width block (no real `{` was ever found): no
        // bytes to splice at all (MEDIUM-6, issue #4 fix list item 4).
        return;
    }
    let open_end = block.span.start + 1;
    out.push_str(&source[block.span.start as usize..open_end as usize]);
    // `tail` is not necessarily last in *byte order*: trailing trivia
    // between the tail expression and the block's closing `}` is its own
    // trailing statement (decision D3 point 1 forbids widening a `Jsx`
    // span to absorb it instead). Sort by span start rather than assuming
    // `statements` then `tail` is source order.
    let mut exprs: Vec<&ast::Expr> = block.statements.iter().collect();
    exprs.extend(block.tail.as_deref());
    exprs.sort_by_key(|e| expr_span(e).start);
    for expr in exprs {
        rebuild_expr(expr, source, out);
    }
    // `Block::close` is the ground truth for whether this block found its
    // own matching `}` — never a sniff of whether `source` happens to end
    // in a `}` byte, which a broken construct nested inside the block can
    // swallow without that being this block's own close (MEDIUM-6).
    if let Some(close) = block.close {
        out.push_str(slice(source, close));
    }
}

fn rebuild_expr(expr: &ast::Expr, source: &str, out: &mut String) {
    match expr {
        ast::Expr::Rust(rs) => out.push_str(slice(source, rs.span)),
        ast::Expr::Jsx(el) => out.push_str(slice(source, el.span)),
        ast::Expr::Error(e) => out.push_str(slice(source, e.span)),
    }
}

fn slice(source: &str, span: Span) -> &str {
    &source[span.start as usize..span.end as usize]
}

trait FunctionSpans {
    fn statements_and_tail_spans(&self) -> Vec<Span>;
}

impl FunctionSpans for ast::Function {
    fn statements_and_tail_spans(&self) -> Vec<Span> {
        self.body
            .statements
            .iter()
            .map(expr_span)
            .chain(self.body.tail.as_deref().map(expr_span))
            .collect()
    }
}
