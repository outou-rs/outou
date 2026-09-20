//! Formats one `{ ... }` Rust expression island — a child expression or
//! an attribute value — by delegating its Rust content back to
//! [`crate::snippet::format_rust_snippet`] (with the synthetic-wrapper
//! path, since an island's content on its own is not a complete file)
//! and then either keeping it on one line or indenting it as its own
//! block.

use outou_syntax::ast;

use crate::collect;
use crate::error::FormatError;
use crate::multiline_guard::island_has_unsafe_multiline_content;
use crate::snippet::format_rust_snippet;
use crate::width::{fits, INDENT_UNIT};
use crate::FormatOptions;

/// Formats `island`, including its surrounding braces, starting at
/// column `indent`.
///
/// If anything in `island`'s subtree is a multi-line token or multi-line
/// JSX text (`crate::multiline_guard`'s doc comment explains exactly
/// why), the island is left byte-for-byte as written instead: this
/// crate's line-based reindentation would otherwise corrupt it, and
/// worse, corrupt it a little more on every subsequent format
/// (`docs/phase0/issues/13-formatter.md`'s "leave verbatim" policy).
pub(crate) fn format_island(
    island: &ast::Island,
    indent: usize,
    source: &str,
    options: &FormatOptions,
) -> Result<String, FormatError> {
    if island_has_unsafe_multiline_content(island, source) {
        return Ok(verbatim(island, source));
    }

    let range = island.span.start as usize..island.span.end as usize;
    let top_level = collect::top_level_jsx(island.parts.iter());
    let body = format_rust_snippet(source, range, &top_level, true, options)?;

    if !body.contains('\n') && fits(indent, body.chars().count() + 2) {
        return Ok(format!("{{{body}}}"));
    }

    let inner_indent = indent + INDENT_UNIT;
    let reindented = indent_every_line(&body, inner_indent);
    Ok(format!(
        "{{\n{reindented}\n{close_pad}}}",
        close_pad = " ".repeat(indent)
    ))
}

/// `island`'s exact source bytes, braces included, completely untouched
/// — not re-split into lines, not reindented at all. The braces are
/// exactly one byte each (`crate::snippet`'s module doc: every island's
/// span is the content strictly between them), so `span.start - 1` and
/// `span.end + 1` are always in bounds for a real island.
fn verbatim(island: &ast::Island, source: &str) -> String {
    source[island.span.start as usize - 1..island.span.end as usize + 1].to_string()
}

/// Prepends `indent` spaces to every non-blank line of `text`; blank
/// lines stay blank rather than becoming trailing whitespace.
fn indent_every_line(text: &str, indent: usize) -> String {
    let pad = " ".repeat(indent);
    text.lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{pad}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only_attribute_island(source: &str) -> ast::Island {
        let parsed = outou_syntax::parse(source);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let element = &parsed.file.jsx_elements()[0];
        match &element.attributes[0].value {
            Some(ast::JsxAttributeValue::Expression(island)) => island.clone(),
            other => panic!("expected an expression attribute, got {other:?}"),
        }
    }

    #[test]
    fn a_short_expression_stays_on_one_line() {
        let source = "fn f() { <input value={x} /> }";
        let island = only_attribute_island(source);
        let out = format_island(&island, 0, source, &FormatOptions::default()).unwrap();
        assert_eq!(out, "{x}");
    }
}
