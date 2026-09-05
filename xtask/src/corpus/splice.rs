//! The Phase 0 round-trip contract itself (`docs/grammar.md` §9,
//! `crates/outou-syntax/README.md`): splicing every node's own source
//! slice back together, in order, must reproduce the original file
//! exactly. This is the same algorithm
//! `crates/outou-syntax/tests/fixtures.rs::source_round_trips_by_splicing`
//! checks against the in-repo fixtures; `corpus test` runs it against
//! every corpus file instead, since the corpus is plain Rust and has no
//! `.expected` output to compare against — a splice mismatch, or any JSX
//! element found at all, is itself the interesting signal (a Outou
//! false-positive JSX detection).

use outou_sourcemap::Span;
use outou_syntax::ast;

/// Rebuilds `source` by walking `file` and splicing each leaf's own
/// source slice, verbatim for Rust and `source[element.span]` for JSX.
pub fn rebuild_source(file: &ast::File, source: &str) -> String {
    let mut out = String::new();
    for item in &file.items {
        rebuild_item(item, source, &mut out);
    }
    out
}

fn rebuild_item(item: &ast::Item, source: &str, out: &mut String) {
    match item {
        ast::Item::Function(f) => {
            out.push_str(&source[f.span.start as usize..f.signature.span.start as usize]);
            out.push_str(slice(source, f.signature.span));
            rebuild_block(&f.body, source, out);
        }
        ast::Item::Module(m) => out.push_str(slice(source, m.span)),
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
        return;
    }
    let open_end = block.span.start + 1;
    out.push_str(&source[block.span.start as usize..open_end as usize]);
    let mut exprs: Vec<&ast::Expr> = block.statements.iter().collect();
    exprs.extend(block.tail.as_deref());
    exprs.sort_by_key(|e| expr_span(e).start);
    for expr in exprs {
        rebuild_expr(expr, source, out);
    }
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

fn expr_span(expr: &ast::Expr) -> Span {
    match expr {
        ast::Expr::Rust(rs) => rs.span,
        ast::Expr::Jsx(el) => el.span,
        ast::Expr::Error(e) => e.span,
    }
}

fn slice(source: &str, span: Span) -> &str {
    &source[span.start as usize..span.end as usize]
}

/// Every JSX element found anywhere in `file`, in a depth-first,
/// source-order-ish traversal (top-level items in order, then each
/// element's attributes before its children). On plain Rust input, any
/// element found here is by definition a mis-detection: `corpus test`
/// reports the first one's span as the offending location.
pub fn collect_jsx_elements(file: &ast::File) -> Vec<&ast::JsxElement> {
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

/// Converts a byte offset into `source` to a 1-based `(line, column)`
/// pair, counting columns in characters, the same convention
/// `outou_syntax::render` uses for diagnostics. Kept as a small local
/// copy rather than a dependency on that private function.
pub fn line_col(source: &str, byte_offset: u32) -> (usize, usize) {
    let offset = (byte_offset as usize).min(source.len());
    let mut line = 1usize;
    let mut col = 1usize;
    for (idx, ch) in source.char_indices() {
        if idx >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_rust_round_trips_and_has_no_jsx() {
        let source = "fn f(a: i32) -> i32 {\n    let b = a < 1 && a > 0;\n    b as i32\n}\n";
        let parsed = outou_syntax::parse(source);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        assert!(collect_jsx_elements(&parsed.file).is_empty());
        assert_eq!(rebuild_source(&parsed.file, source), source);
    }

    #[test]
    fn real_jsx_is_found_and_round_trips() {
        let source = "fn f() { <div>hi</div> }";
        let parsed = outou_syntax::parse(source);
        let elements = collect_jsx_elements(&parsed.file);
        assert_eq!(elements.len(), 1);
        assert_eq!(rebuild_source(&parsed.file, source), source);
    }

    #[test]
    fn line_col_is_one_based() {
        assert_eq!(line_col("abc", 0), (1, 1));
        assert_eq!(line_col("ab\ncd", 3), (2, 1));
    }
}
