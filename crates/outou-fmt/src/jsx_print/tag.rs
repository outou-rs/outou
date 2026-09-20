//! Formats a JSX element's opening tag: the name, its attributes, and
//! `>` or `/>` — on one line when it fits, one attribute per line
//! otherwise.

use outou_syntax::ast;

use super::{element_name, island};
use crate::error::FormatError;
use crate::width::{fits, INDENT_UNIT};
use crate::FormatOptions;

/// Formats `element`'s opening tag, starting at column `indent`. Returns
/// text with no leading indentation on its first line and each
/// continuation line (only present when attributes had to wrap) indented
/// to `indent`.
pub(super) fn format_open_tag(
    element: &ast::JsxElement,
    indent: usize,
    source: &str,
    options: &FormatOptions,
) -> Result<String, FormatError> {
    let name = element_name(element)?;
    let self_closing = element.close.is_none();
    let closer = if self_closing { "/>" } else { ">" };

    let attribute_indent = indent + INDENT_UNIT;
    let mut attributes = Vec::with_capacity(element.attributes.len());
    for attribute in &element.attributes {
        attributes.push(format_attribute(
            attribute,
            attribute_indent,
            source,
            options,
        )?);
    }

    if let Some(single_line) = try_single_line(name, &attributes, closer, indent) {
        return Ok(single_line);
    }
    Ok(multi_line(name, &attributes, closer, indent))
}

fn try_single_line(
    name: &str,
    attributes: &[String],
    closer: &str,
    indent: usize,
) -> Option<String> {
    if attributes.iter().any(|attr| attr.contains('\n')) {
        return None;
    }
    let mut candidate = format!("<{name}");
    for attribute in attributes {
        candidate.push(' ');
        candidate.push_str(attribute);
    }
    if self_closing_needs_space(closer) {
        candidate.push(' ');
    }
    candidate.push_str(closer);
    fits(indent, candidate.chars().count()).then_some(candidate)
}

fn self_closing_needs_space(closer: &str) -> bool {
    closer == "/>"
}

fn multi_line(name: &str, attributes: &[String], closer: &str, indent: usize) -> String {
    if attributes.is_empty() {
        // Nothing to wrap onto its own line; the name itself must be the
        // reason this doesn't fit, and there is nothing more to do about
        // that without breaking the identifier apart.
        return format!("<{name}{closer}");
    }
    let inner_indent = indent + INDENT_UNIT;
    let pad = " ".repeat(inner_indent);
    let mut out = format!("<{name}\n");
    for attribute in attributes {
        out.push_str(&pad);
        out.push_str(attribute);
        out.push('\n');
    }
    out.push_str(&" ".repeat(indent));
    out.push_str(closer);
    out
}

/// Formats one attribute: a bare name, `name="text"` (the string literal
/// is copied verbatim from the source, quotes included, never re-decoded
/// or re-escaped — issue #13's "when in doubt, preserve"), or
/// `name={expr}`.
fn format_attribute(
    attribute: &ast::JsxAttribute,
    indent: usize,
    source: &str,
    options: &FormatOptions,
) -> Result<String, FormatError> {
    let name = &attribute.name.name;
    match &attribute.value {
        None => Ok(name.clone()),
        Some(ast::JsxAttributeValue::Text(text)) => {
            let literal = &source[text.span.start as usize..text.span.end as usize];
            Ok(format!("{name}={literal}"))
        }
        Some(ast::JsxAttributeValue::Expression(value)) => {
            let rendered = island::format_island(value, indent, source, options)?;
            Ok(format!("{name}={rendered}"))
        }
        Some(ast::JsxAttributeValue::Error(_)) => {
            // Never reached in practice: `crate::format_source` refuses
            // any file with an error node before formatting starts.
            // Falling back to the verbatim bytes is still strictly safer
            // than panicking if that invariant is ever violated.
            Ok(source[attribute.span.start as usize..attribute.span.end as usize].to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only_element(source: &str) -> ast::JsxElement {
        let parsed = outou_syntax::parse(source);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        parsed.file.jsx_elements()[0].clone()
    }

    #[test]
    fn single_attribute_stays_on_one_line() {
        let source = r#"fn f() { <input value="x" /> }"#;
        let element = only_element(source);
        let out = format_open_tag(&element, 0, source, &FormatOptions::default()).unwrap();
        assert_eq!(out, r#"<input value="x" />"#);
    }

    #[test]
    fn many_attributes_wrap_one_per_line() {
        let source = "fn f() { <input aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb /> }";
        let element = only_element(source);
        let out = format_open_tag(&element, 0, source, &FormatOptions::default()).unwrap();
        assert!(out.starts_with("<input\n"));
        assert!(out.contains("    aaaa"));
        assert!(out.ends_with("/>"));
    }
}
