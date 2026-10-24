//! Identifier prefixes reserved for generated code (issue #28).
//!
//! Codegen binds the values of `key`, `<select value>` and the options it
//! selects to synthesized identifiers (`__outou_key_0`, `__outou_select_17`,
//! ...) in a block around the element. An identifier of the same name written
//! in a `{ … }` island of the user's own code (a closure parameter, say)
//! would capture that binding and change what the element does without a
//! compile error. Using one is therefore an error. Only the prefixes codegen
//! really generates are reserved: other `__outou_` names stay legal (the
//! formatter's own placeholders, which it salts to avoid collisions, share the
//! `__outou_` start).

use outou_sourcemap::Span;

use crate::ast::{Expr, File, Island, JsxAttributeValue, JsxChild, JsxElement};
use crate::lexer::rust_token::{next_token, RtKind};
use crate::{Diagnostic, Severity};

/// Prefix of a synthesized `key` binding.
pub const KEY_PREFIX: &str = "__outou_key_";
/// Prefix of a synthesized `<select value>` binding.
pub const SELECT_PREFIX: &str = "__outou_select_";
/// Prefix of a synthesized `<option value>` binding.
pub const OPTION_PREFIX: &str = "__outou_opt_";
/// Prefix of a synthesized `selected` flag binding.
pub const SELECTED_PREFIX: &str = "__outou_selected_";

const RESERVED_PREFIXES: [&str; 4] = [KEY_PREFIX, SELECT_PREFIX, OPTION_PREFIX, SELECTED_PREFIX];

/// An error for each identifier in a JSX island of `file` that starts with a
/// reserved prefix. Comments and literals are not identifiers and are not
/// looked at.
pub fn check(file: &File) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for element in file.jsx_elements() {
        check_element(element, &mut diagnostics);
    }
    diagnostics
}

fn check_element(element: &JsxElement, out: &mut Vec<Diagnostic>) {
    for attribute in &element.attributes {
        if let Some(JsxAttributeValue::Expression(island)) = &attribute.value {
            check_island(island, out);
        }
    }
    for child in &element.children {
        if let JsxChild::Expression(island) = child {
            check_island(island, out);
        }
    }
}

fn check_island(island: &Island, out: &mut Vec<Diagnostic>) {
    for part in &island.parts {
        let Expr::Rust(rust) = part else { continue };
        let bytes = rust.text.as_bytes();
        let mut pos = 0usize;
        loop {
            let token = next_token(bytes, pos);
            if token.start == token.end {
                break;
            }
            pos = token.end;
            if !matches!(token.kind, RtKind::Ident | RtKind::RawIdent) {
                continue;
            }
            let text = token.text(&rust.text);
            let name = text.strip_prefix("r#").unwrap_or(text);
            if RESERVED_PREFIXES
                .iter()
                .any(|prefix| name.starts_with(prefix))
            {
                let start = rust.span.start + token.start as u32;
                out.push(Diagnostic {
                    span: Span::new(start, start + (token.end - token.start) as u32),
                    message: format!(
                        "`{name}` is reserved for generated code: identifiers starting with `__outou_key_`, `__outou_select_`, `__outou_selected_` or `__outou_opt_` cannot be used in a JSX expression; rename it"
                    ),
                    severity: Severity::Error,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn messages(source: &str) -> Vec<String> {
        let parsed = crate::parse(source);
        check(&parsed.file)
            .into_iter()
            .map(|diagnostic| {
                format!(
                    "{}: {}",
                    &source[diagnostic.span.start as usize..diagnostic.span.end as usize],
                    diagnostic.message
                )
            })
            .collect()
    }

    #[test]
    fn a_closure_parameter_with_a_reserved_name_is_an_error_at_its_position() {
        let source = "fn f() { <select value=\"a\">{[\"b\"].into_iter().map(|__outou_select_9| <option value=\"a\">A</option>)}</select> }";
        let found = messages(source);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0]
            .starts_with("__outou_select_9: `__outou_select_9` is reserved for generated code"));
    }

    #[test]
    fn every_generated_prefix_is_reserved_in_child_and_attribute_islands() {
        for name in [
            "__outou_key_0",
            "__outou_select_1",
            "__outou_opt_2",
            "__outou_selected_3",
            "r#__outou_key_4",
        ] {
            let in_child = format!("fn f() {{ let v = 1; <p>{{{name}}}</p> }}");
            assert_eq!(messages(&in_child).len(), 1, "{name} as a child");
            let in_attribute = format!("fn f() {{ <p title={{{name}}} /> }}");
            assert_eq!(messages(&in_attribute).len(), 1, "{name} as an attribute");
        }
    }

    #[test]
    fn strings_comments_and_other_outou_names_are_fine() {
        let source = "fn f() { <p title={\"__outou_key_0\"}>{/* __outou_select_1 */ __outou_fmt_jsx_0}{x /* __outou_opt_1 */}</p> }";
        assert!(messages(source).is_empty(), "{:?}", messages(source));
    }

    #[test]
    fn plain_rust_outside_jsx_is_not_checked() {
        let source = "fn f() { let __outou_key_0 = 1; <p /> }";
        assert!(messages(source).is_empty());
    }
}
