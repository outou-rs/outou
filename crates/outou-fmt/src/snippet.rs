//! The placeholder -> `rustfmt` -> splice pipeline for one region of Rust
//! source, whether that region is a whole `.rsx` file or the content of
//! one JSX expression island.
//!
//! A `.rsx` file is already guaranteed to be a valid Rust file once every
//! JSX expression in it is replaced by a Rust expression (`docs/grammar.md`
//! §1). This module leans on that guarantee directly: it substitutes every
//! *top-level* JSX element within a byte range with a placeholder
//! identifier, hands the (now plain Rust) text to `rustfmt`, and then
//! replaces each placeholder with that element's own recursively
//! formatted JSX text (`crate::jsx_print`) — an element nested only
//! inside another element's tag structure (a child element, never
//! through an island) is formatted by that recursive call, not by a
//! separate `rustfmt` pass.
//!
//! A byte range that is not already a complete, top-level Rust file (an
//! expression island's content, `{ ... }` without the braces) is wrapped
//! in a throwaway function first, so `rustfmt` always receives a
//! complete file; [`strip_wrapper`] removes that wrapper afterward and
//! dedents the result back to "as if this text started at column 0",
//! which is exactly the form a caller needs to reindent the snippet
//! wherever it is being spliced.

use std::ops::Range;

use outou_syntax::ast;

use crate::error::FormatError;
use crate::jsx_print;
use crate::placeholder::PlaceholderSource;
use crate::rustfmt_proc::run_rustfmt;
use crate::width::INDENT_UNIT;
use crate::FormatOptions;

const WRAPPER_FN_NAME: &str = "__outou_fmt_wrapper";

/// Formats the Rust source in `source[range]`, recursively formatting any
/// JSX in `top_level` (elements directly within that range, not nested
/// inside another JSX element's own structure — see the module doc).
///
/// `wrap` is `false` only for a whole file, where `source[range]` is
/// already a complete Rust file once placeholders are substituted; it is
/// `true` for anything else (an island's content), which needs the
/// synthetic-function wrapper to become one.
///
/// Returns the fully formatted, fully spliced text: for `wrap: false`
/// this is the finished file; for `wrap: true` it is dedented to column
/// 0, ready for the caller to reindent to wherever it is being placed.
pub(crate) fn format_rust_snippet(
    source: &str,
    range: Range<usize>,
    top_level: &[&ast::JsxElement],
    wrap: bool,
    options: &FormatOptions,
) -> Result<String, FormatError> {
    let mut placeholders = PlaceholderSource::new(source);
    let (with_placeholders, sites) =
        substitute_placeholders(source, range, top_level, &mut placeholders);

    let to_format = if wrap {
        wrap_in_function(&with_placeholders)
    } else {
        with_placeholders
    };

    let formatted = run_rustfmt(&to_format, options)?;
    let mut body = if wrap {
        strip_wrapper(&formatted)?
    } else {
        formatted
    };

    for (placeholder, element) in &sites {
        let line_indent = indent_of_line_containing(&body, placeholder)?;
        let jsx_text = jsx_print::format_jsx_element(element, line_indent, source, options)?;
        body = replace_once(&body, placeholder, &jsx_text)?;
    }

    Ok(body)
}

/// Builds `source[range]` with every element of `top_level` replaced by a
/// fresh placeholder identifier, alongside the list of
/// `(placeholder, element)` pairs so the caller can splice each one back.
fn substitute_placeholders<'a>(
    source: &str,
    range: Range<usize>,
    top_level: &[&'a ast::JsxElement],
    placeholders: &mut PlaceholderSource<'_>,
) -> (String, Vec<(String, &'a ast::JsxElement)>) {
    let mut sites: Vec<&ast::JsxElement> = top_level.to_vec();
    sites.sort_by_key(|element| element.span.start);

    let mut out = String::new();
    let mut cursor = range.start;
    let mut placeholder_sites = Vec::with_capacity(sites.len());
    for element in sites {
        let start = element.span.start as usize;
        let end = element.span.end as usize;
        if start > cursor {
            out.push_str(&source[cursor..start]);
        }
        let placeholder = placeholders.fresh();
        out.push_str(&placeholder);
        placeholder_sites.push((placeholder, element));
        cursor = end;
    }
    if cursor < range.end {
        out.push_str(&source[cursor..range.end]);
    }
    (out, placeholder_sites)
}

fn wrap_in_function(content: &str) -> String {
    format!("fn {WRAPPER_FN_NAME}() {{\n{content}\n}}\n")
}

/// Removes the synthetic wrapper function [`wrap_in_function`] added and
/// dedents every remaining line by exactly [`INDENT_UNIT`] — the one,
/// uniform level of indentation `rustfmt` adds for the wrapper's body,
/// regardless of how deeply nested the content inside it is (indentation
/// is additive per line, so removing a constant amount from every line
/// preserves the content's own relative structure exactly).
fn strip_wrapper(formatted: &str) -> Result<String, FormatError> {
    let mut lines: Vec<&str> = formatted.lines().collect();
    if lines.len() < 2 {
        return Err(FormatError::Internal(format!(
            "rustfmt produced unexpectedly short output for a wrapped snippet: {formatted:?}"
        )));
    }
    let header = lines.remove(0);
    if !header.starts_with(&format!("fn {WRAPPER_FN_NAME}")) {
        return Err(FormatError::Internal(format!(
            "wrapped snippet lost its wrapper header: {header:?}"
        )));
    }
    let footer = lines.pop().expect("checked len >= 2 above");
    if footer.trim() != "}" {
        return Err(FormatError::Internal(format!(
            "wrapped snippet lost its wrapper footer: {footer:?}"
        )));
    }
    let dedented: Vec<String> = lines.iter().map(|line| dedent_one_level(line)).collect();
    Ok(dedented.join("\n"))
}

fn dedent_one_level(line: &str) -> String {
    match line.strip_prefix(&" ".repeat(INDENT_UNIT)) {
        Some(rest) => rest.to_string(),
        // A blank line, or (defensively) a line rustfmt did not indent as
        // expected: never panic on a slice out of range.
        None => line.trim_start_matches(' ').to_string(),
    }
}

/// The number of leading spaces on the line of `text` that contains
/// `needle`. Used to decide how far to indent a placeholder's replacement
/// (`crate::jsx_print`'s continuation lines line up with the statement's
/// own indent, not the placeholder's exact column — matching how rustfmt
/// itself indents a wrapped macro call's arguments).
fn indent_of_line_containing(text: &str, needle: &str) -> Result<usize, FormatError> {
    let at = text.find(needle).ok_or_else(|| {
        FormatError::Internal(format!("placeholder `{needle}` did not survive formatting"))
    })?;
    let line_start = text[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line = &text[line_start..at];
    Ok(line.chars().take_while(|c| *c == ' ').count())
}

/// Replaces exactly one occurrence of `needle` in `text`, erroring
/// (rather than silently doing nothing, or replacing an unintended
/// second occurrence) if it is not found exactly once — a placeholder
/// name is unique by construction, so anything else means an internal
/// invariant broke.
fn replace_once(text: &str, needle: &str, replacement: &str) -> Result<String, FormatError> {
    let count = text.matches(needle).count();
    if count != 1 {
        return Err(FormatError::Internal(format!(
            "expected placeholder `{needle}` exactly once, found {count}"
        )));
    }
    Ok(text.replacen(needle, replacement, 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_a_whole_file_with_no_jsx() {
        let source = "fn main( ) {\nlet x=1;\n}\n";
        let out = format_rust_snippet(
            source,
            0..source.len(),
            &[],
            false,
            &FormatOptions::default(),
        )
        .unwrap();
        assert_eq!(out, "fn main() {\n    let x = 1;\n}\n");
    }

    #[test]
    fn wraps_and_dedents_a_snippet() {
        let source = "1 + 1";
        let out = format_rust_snippet(
            source,
            0..source.len(),
            &[],
            true,
            &FormatOptions::default(),
        )
        .unwrap();
        assert_eq!(out, "1 + 1");
    }
}
