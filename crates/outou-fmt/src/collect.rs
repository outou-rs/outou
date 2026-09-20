//! Finds the JSX elements that are placeholder sites for one `rustfmt`
//! pass: the JSX directly reachable while walking plain Rust structure
//! (items, blocks, expression islands), stopping the moment a
//! [`ast::JsxElement`] itself is reached rather than descending into its
//! attributes or children.
//!
//! A JSX element nested inside another one only through JSX structure
//! (a child element, or an attribute/child island) is *not* one of these
//! sites — it is formatted afterwards, recursively, once the outer
//! element's own text is being rendered (`crate::jsx_print`). This
//! mirrors [`ast::File::jsx_elements`] but stops one level earlier; see
//! that method's doc comment for why one canonical walk matters.

use outou_sourcemap::Span;
use outou_syntax::ast;

/// Every top-level JSX element reachable from `file`'s items, in an
/// unspecified order (callers that need source order sort by span).
pub(crate) fn file_top_level_jsx(file: &ast::File) -> Vec<&ast::JsxElement> {
    let mut out = Vec::new();
    for item in &file.items {
        collect_item(item, &mut out);
    }
    out
}

fn collect_item<'a>(item: &'a ast::Item, out: &mut Vec<&'a ast::JsxElement>) {
    match item {
        ast::Item::Function(function) => {
            out.extend(top_level_jsx(ordered_block_exprs(&function.body)));
        }
        ast::Item::Module(module) => {
            for inner in module.items.iter().flatten() {
                collect_item(inner, out);
            }
        }
        ast::Item::Rust(rust_item) => {
            out.extend(top_level_jsx(rust_item.parts.iter()));
        }
        ast::Item::Error(_) => {}
    }
}

/// A [`ast::Block`]'s statements and tail expression, in source order.
/// [`ast::Block::statements`] alone is not necessarily in source order
/// (its own doc comment says so); this is the one place that sorts it,
/// mirroring the same caveat on [`ast::Island::parts`].
pub(crate) fn ordered_block_exprs(block: &ast::Block) -> Vec<&ast::Expr> {
    let mut exprs: Vec<&ast::Expr> = block.statements.iter().collect();
    if let Some(tail) = &block.tail {
        exprs.push(tail);
    }
    exprs.sort_by_key(|expr| expr_span(expr).start);
    exprs
}

/// The JSX elements directly in `exprs` — not nested inside one of them.
pub(crate) fn top_level_jsx<'a>(
    exprs: impl IntoIterator<Item = &'a ast::Expr>,
) -> Vec<&'a ast::JsxElement> {
    exprs
        .into_iter()
        .filter_map(|expr| match expr {
            ast::Expr::Jsx(element) => Some(element),
            _ => None,
        })
        .collect()
}

fn expr_span(expr: &ast::Expr) -> Span {
    match expr {
        ast::Expr::Rust(source) => source.span,
        ast::Expr::Jsx(element) => element.span,
        ast::Expr::Error(error) => error.span,
    }
}

/// Whether `file` contains any [`ast::ErrorNode`] anywhere (a defensive
/// second check alongside the caller's own diagnostics check — a broken
/// construct should always come with a diagnostic, but formatting must
/// never touch one either way).
pub(crate) fn has_error_nodes(file: &ast::File) -> bool {
    file.items.iter().any(item_has_error)
}

fn item_has_error(item: &ast::Item) -> bool {
    match item {
        ast::Item::Function(function) => ordered_block_exprs(&function.body)
            .iter()
            .any(|expr| expr_has_error(expr)),
        ast::Item::Module(module) => module.items.iter().flatten().any(item_has_error),
        ast::Item::Rust(rust_item) => rust_item.parts.iter().any(expr_has_error),
        ast::Item::Error(_) => true,
    }
}

pub(crate) fn expr_has_error(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Error(_) => true,
        ast::Expr::Rust(_) => false,
        ast::Expr::Jsx(element) => jsx_has_error(element),
    }
}

fn jsx_has_error(element: &ast::JsxElement) -> bool {
    if !element.errors.is_empty() {
        return true;
    }
    if matches!(element.open, ast::JsxTag::Incomplete(_)) {
        return true;
    }
    if let Some(ast::JsxTag::Incomplete(_)) = &element.close {
        return true;
    }
    for attr in &element.attributes {
        if let Some(ast::JsxAttributeValue::Error(_)) = &attr.value {
            return true;
        }
        if let Some(ast::JsxAttributeValue::Expression(island)) = &attr.value {
            if island.parts.iter().any(expr_has_error) {
                return true;
            }
        }
    }
    for child in &element.children {
        match child {
            ast::JsxChild::Error(_) => return true,
            ast::JsxChild::Expression(island) => {
                if island.parts.iter().any(expr_has_error) {
                    return true;
                }
            }
            ast::JsxChild::Element(nested) => {
                if jsx_has_error(nested) {
                    return true;
                }
            }
            ast::JsxChild::Text(_) => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_top_level_element_in_a_function_body() {
        let parsed = outou_syntax::parse("fn f() { <div/> }");
        let sites = file_top_level_jsx(&parsed.file);
        assert_eq!(sites.len(), 1);
    }

    #[test]
    fn does_not_descend_into_a_nested_elements_children() {
        // Only the outer <div> is a top-level site; <span> is reached
        // through JSX structure, not plain Rust, so it must not appear.
        let parsed = outou_syntax::parse("fn f() { <div><span/></div> }");
        let sites = file_top_level_jsx(&parsed.file);
        assert_eq!(sites.len(), 1);
    }

    #[test]
    fn finds_jsx_nested_inside_plain_rust_at_any_brace_depth() {
        let parsed = outou_syntax::parse("fn f() { if true { <div/> } }");
        let sites = file_top_level_jsx(&parsed.file);
        assert_eq!(sites.len(), 1);
    }

    #[test]
    fn clean_input_has_no_error_nodes() {
        let parsed = outou_syntax::parse("fn f() { <div>{1}</div> }");
        assert!(!has_error_nodes(&parsed.file));
    }
}
