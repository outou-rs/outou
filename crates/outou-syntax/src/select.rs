//! Which `<option>` elements a `<select value={V}>` selects (alpha gate A5,
//! issue #28), and the warnings for the shapes where it cannot.
//!
//! The browser applies a select's `value` before its options exist, so the
//! backend alone shows the wrong option on mount (`docs/backend-leakage.md`
//! row 37). Outou gives `<select value>` the meaning it has in React by
//! lowering: every `<option value=..>` that is *lexically* inside the
//! `<select>` gets a `selected` computed from the select's value. This module
//! finds those options once, for codegen and for diagnostics alike.
//!
//! "Lexically inside" means written in the select's children: directly, in
//! another lowercase element (`<optgroup>`), or in a `{ … }` child island, at
//! any depth of `.map(|p| <option …>)`, `if` or `match`. It does not mean an
//! option reached through a variable, returned by a function, or rendered by
//! a component (`<Choice />`): the source does not show it. An option inside
//! another `<select>` belongs to that select. Attribute islands are not
//! searched.

use std::collections::HashMap;

use outou_sourcemap::Span;

use crate::ast::{
    unraw_name, Expr, File, Island, JsxAttribute, JsxAttributeValue, JsxChild, JsxElement, JsxTag,
};
use crate::lexer::rust_token::{next_token, RtKind};
use crate::{Diagnostic, Severity};

const SELECT: &str = "select";
const OPTION: &str = "option";
const VALUE: &str = "value";
const SELECTED: &str = "selected";
const MULTIPLE: &str = "multiple";

/// The result of [`scan`]: for each `<option>` that is rewritten, the select
/// it belongs to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectScan {
    /// Option element start offset → select element start offset.
    select_of_option: HashMap<u32, u32>,
}

impl SelectScan {
    /// The start offset of the `<select>` whose `value` selects `option`, or
    /// `None` when `option` is not rewritten (not in such a select, no
    /// `value`, or an explicit `selected`).
    pub fn select_of(&self, option: &JsxElement) -> Option<u32> {
        self.select_of_option.get(&option.span.start).copied()
    }
}

/// The element's tag name, if its opening tag has one.
pub fn tag_name(element: &JsxElement) -> Option<&str> {
    match &element.open {
        JsxTag::Named { name, .. } => Some(&name.name),
        JsxTag::Incomplete(_) => None,
    }
}

fn attribute<'a>(element: &'a JsxElement, name: &str) -> Option<&'a JsxAttribute> {
    element
        .attributes
        .iter()
        .find(|attribute| unraw_name(&attribute.name.name) == name)
}

fn is_usable(attribute: &JsxAttribute) -> bool {
    !matches!(attribute.value, Some(JsxAttributeValue::Error(_)))
}

/// Whether a `<select>` allows several selections, as far as the source
/// shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Multiple {
    /// No `multiple` attribute, or `multiple={false}`: one selection.
    No,
    /// A bare `multiple`, `multiple={true}` or a string value (which JSX
    /// treats as true): several selections.
    Yes,
    /// Any other expression (`multiple={flag}`). Its value is only known at
    /// run time, so it is treated like [`Multiple::Yes`] and warned about.
    Unknown,
}

fn multiple_of(element: &JsxElement) -> Multiple {
    let Some(attribute) = attribute(element, MULTIPLE) else {
        return Multiple::No;
    };
    match &attribute.value {
        None | Some(JsxAttributeValue::Text(_)) => Multiple::Yes,
        Some(JsxAttributeValue::Expression(island)) => match island.parts.as_slice() {
            [Expr::Rust(rust)] => match rust.text.trim() {
                "true" => Multiple::Yes,
                "false" => Multiple::No,
                _ => Multiple::Unknown,
            },
            _ => Multiple::Unknown,
        },
        Some(JsxAttributeValue::Error(_)) => Multiple::Unknown,
    }
}

/// The `value` attribute of a `<select>` whose options are selected by it:
/// the element is a `<select>`, has a usable `value`, and is not `multiple`
/// (`multiple={false}` is not `multiple`; an expression that is not a
/// literal `true` or `false` counts as `multiple`).
pub fn select_value_attribute(element: &JsxElement) -> Option<&JsxAttribute> {
    if tag_name(element) != Some(SELECT) || multiple_of(element) != Multiple::No {
        return None;
    }
    attribute(element, VALUE).filter(|attribute| is_usable(attribute))
}

/// The `value` attribute of an `<option>` that is a candidate for being
/// selected: it has a usable `value` and no `selected` of its own.
pub fn option_value_attribute(element: &JsxElement) -> Option<&JsxAttribute> {
    if tag_name(element) != Some(OPTION) || attribute(element, SELECTED).is_some() {
        return None;
    }
    attribute(element, VALUE).filter(|attribute| is_usable(attribute))
}

/// Finds the options every `<select value>` of `file` selects, and the
/// warnings for what it cannot reach.
pub fn scan(file: &File) -> (SelectScan, Vec<Diagnostic>) {
    let mut walk = Walk::default();
    for element in file.jsx_elements() {
        walk.select(element);
    }
    (
        SelectScan {
            select_of_option: walk.select_of_option,
        },
        walk.diagnostics,
    )
}

#[derive(Default)]
struct Walk {
    select_of_option: HashMap<u32, u32>,
    diagnostics: Vec<Diagnostic>,
}

/// What one `<select>`'s subtree held.
#[derive(Default)]
struct Found {
    /// Options with a `value` (rewritten) or a `selected` (the user's own).
    options: u32,
    /// Warnings for what cannot be reached.
    unreachable: u32,
}

impl Walk {
    fn warn(&mut self, span: Span, message: String) {
        self.diagnostics.push(Diagnostic {
            span,
            message,
            severity: Severity::Warning,
        });
    }

    fn select(&mut self, element: &JsxElement) {
        if tag_name(element) != Some(SELECT) {
            return;
        }
        let Some(value) = attribute(element, VALUE).filter(|value| is_usable(value)) else {
            return;
        };
        match multiple_of(element) {
            Multiple::No => {}
            Multiple::Yes => {
                self.warn(
                    value.span,
                    "`value` on a `<select multiple>` does not select any `<option>`; give each selected `<option>` a `selected` attribute".to_string(),
                );
                return;
            }
            Multiple::Unknown => {
                self.warn(
                    value.span,
                    "`multiple` on this `<select>` is not a literal `true` or `false`, so it is treated as `multiple` and `value` does not select any `<option>`; write `multiple={false}` (or leave `multiple` out) for a single choice, or give each selected `<option>` a `selected` attribute".to_string(),
                );
                return;
            }
        }
        let mut found = Found::default();
        self.children(&element.children, element.span.start, &mut found);
        if found.options == 0 && found.unreachable == 0 {
            self.warn(
                value.span,
                "no `<option>` is written inside this `<select>`, so `value` selects nothing; write the `<option>` elements inside the `<select>`".to_string(),
            );
        }
    }

    fn children(&mut self, children: &[JsxChild], select: u32, found: &mut Found) {
        for child in children {
            match child {
                JsxChild::Element(element) => self.element(element, select, found),
                JsxChild::Expression(island) => self.island(island, select, found),
                JsxChild::Text(_) | JsxChild::Error(_) => {}
            }
        }
    }

    fn island(&mut self, island: &Island, select: u32, found: &mut Found) {
        let mut has_jsx = false;
        for part in &island.parts {
            if let Expr::Jsx(element) = part {
                has_jsx = true;
                self.element(element, select, found);
            }
        }
        if !has_jsx && island_has_code(island) {
            found.unreachable += 1;
            self.warn(
                island.span,
                "this expression is not JSX, so an `<option>` it produces is not selected by `value` on the enclosing `<select>`; write the `<option>` elements inside the `<select>`".to_string(),
            );
        }
    }

    fn element(&mut self, element: &JsxElement, select: u32, found: &mut Found) {
        let JsxTag::Named { name, .. } = &element.open else {
            return;
        };
        match name.name.as_str() {
            OPTION => self.option(element, select, found),
            // Its options belong to it; it is visited on its own.
            SELECT => {}
            other if other.chars().next().is_some_and(char::is_uppercase) => {
                found.unreachable += 1;
                self.warn(
                    name.span,
                    format!(
                        "`<{other}>` is inside a `<select value=…>`: an `<option>` it renders is not selected by `value`; write the `<option>` elements inside the `<select>`"
                    ),
                );
            }
            _ => self.children(&element.children, select, found),
        }
    }

    fn option(&mut self, element: &JsxElement, select: u32, found: &mut Found) {
        if attribute(element, SELECTED).is_some() {
            found.options += 1;
        } else if option_value_attribute(element).is_some() {
            found.options += 1;
            self.select_of_option.insert(element.span.start, select);
        } else if attribute(element, VALUE).is_none() {
            found.unreachable += 1;
            if let JsxTag::Named { name, .. } = &element.open {
                self.warn(
                    name.span,
                    "this `<option>` has no `value`, so Outou does not mark it as selected for the `value` of the enclosing `<select>`; add a `value` attribute to have it selected".to_string(),
                );
            }
        }
    }
}

/// Whether `island` holds anything but whitespace and comments.
fn island_has_code(island: &Island) -> bool {
    island.parts.iter().any(|part| match part {
        Expr::Rust(rust) => rust_has_code(&rust.text),
        Expr::Jsx(_) => true,
        Expr::Error(_) => false,
    })
}

fn rust_has_code(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut pos = 0usize;
    loop {
        let token = next_token(bytes, pos);
        if token.start == token.end {
            return false;
        }
        if !matches!(token.kind, RtKind::LineComment | RtKind::BlockComment) {
            return true;
        }
        pos = token.end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_source(source: &str) -> (SelectScan, Vec<Diagnostic>, crate::Parsed) {
        let parsed = crate::parse(source);
        let (scan, diagnostics) = scan(&parsed.file);
        (scan, diagnostics, parsed)
    }

    fn messages(diagnostics: &[Diagnostic]) -> Vec<&str> {
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect()
    }

    #[test]
    fn options_in_a_map_closure_and_literal_options_belong_to_the_select() {
        let source = "fn f() { <select value={v}><option value=\"a\">A</option>{xs.map(|x| <option value={x}>{x}</option>)}</select> }";
        let (scan, diagnostics, parsed) = scan_source(source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(scan.select_of_option.len(), 2);
        let select = parsed.file.jsx_elements()[0].span.start;
        assert!(scan.select_of_option.values().all(|s| *s == select));
    }

    #[test]
    fn an_explicit_selected_wins_and_is_not_rewritten() {
        let source =
            "fn f() { <select value={v}><option value=\"a\" selected={true}>A</option></select> }";
        let (scan, diagnostics, _) = scan_source(source);
        assert!(scan.select_of_option.is_empty());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn a_select_without_value_selects_nothing_and_is_not_diagnosed() {
        let source = "fn f() { <select><option value=\"a\">A</option></select> }";
        let (scan, diagnostics, _) = scan_source(source);
        assert!(scan.select_of_option.is_empty());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn an_option_belongs_to_its_nearest_select() {
        let source = "fn f() { <select value={a}><optgroup><option value=\"1\"/></optgroup><select value={b}><option value=\"2\"/></select></select> }";
        let (scan, _, parsed) = scan_source(source);
        let elements = parsed.file.jsx_elements();
        let outer = elements[0].span.start;
        let inner = elements
            .iter()
            .filter(|e| tag_name(e) == Some("select"))
            .nth(1)
            .unwrap()
            .span
            .start;
        let owners: Vec<u32> = elements
            .iter()
            .filter(|e| tag_name(e) == Some("option"))
            .map(|e| scan.select_of(e).unwrap())
            .collect();
        assert_eq!(owners, vec![outer, inner]);
    }

    #[test]
    fn multiple_with_value_is_warned_and_not_rewritten() {
        let source = "fn f() { <select multiple value={v}><option value=\"a\"/></select> }";
        let (scan, diagnostics, parsed) = scan_source(source);
        assert!(scan.select_of_option.is_empty());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, Severity::Warning);
        assert!(diagnostics[0].message.contains("`<select multiple>`"));
        let span = diagnostics[0].span;
        assert_eq!(
            &parsed.source[span.start as usize..span.end as usize],
            "value={v}"
        );
    }

    fn first_select_scan(source: &str) -> (usize, Vec<Diagnostic>) {
        let (scan, diagnostics, _) = scan_source(source);
        (scan.select_of_option.len(), diagnostics)
    }

    #[test]
    fn multiple_false_is_a_single_select_and_is_rewritten() {
        for multiple in ["multiple={false}", "multiple={ false }"] {
            let source = format!(
                "fn f() {{ <select {multiple} value=\"b\"><option value=\"b\"/></select> }}"
            );
            let (rewritten, diagnostics) = first_select_scan(&source);
            assert_eq!(rewritten, 1, "{multiple}");
            assert!(diagnostics.is_empty(), "{multiple}: {diagnostics:?}");
        }
    }

    #[test]
    fn multiple_true_and_bare_and_string_are_multiple() {
        for multiple in ["multiple", "multiple={true}", "multiple=\"false\""] {
            let source = format!(
                "fn f() {{ <select {multiple} value=\"b\"><option value=\"b\"/></select> }}"
            );
            let (rewritten, diagnostics) = first_select_scan(&source);
            assert_eq!(rewritten, 0, "{multiple}");
            assert_eq!(diagnostics.len(), 1, "{multiple}");
            assert!(
                diagnostics[0].message.contains("`<select multiple>`"),
                "{multiple}"
            );
        }
    }

    #[test]
    fn a_dynamic_multiple_is_treated_as_multiple_and_warned() {
        for multiple in [
            "multiple={flag}",
            "multiple={!flag}",
            "multiple={if a { b } else { c }}",
        ] {
            let source = format!(
                "fn f() {{ <select {multiple} value=\"b\"><option value=\"b\"/></select> }}"
            );
            let (rewritten, diagnostics) = first_select_scan(&source);
            assert_eq!(rewritten, 0, "{multiple}");
            assert_eq!(diagnostics.len(), 1, "{multiple}");
            assert!(
                diagnostics[0]
                    .message
                    .contains("not a literal `true` or `false`"),
                "{multiple}: {}",
                diagnostics[0].message
            );
        }
    }

    #[test]
    fn an_option_without_value_is_warned() {
        let source =
            "fn f() { <select value={v}><option>A</option><option value=\"b\"/></select> }";
        let (scan, diagnostics, parsed) = scan_source(source);
        assert_eq!(scan.select_of_option.len(), 1);
        assert_eq!(diagnostics.len(), 1);
        let span = diagnostics[0].span;
        assert_eq!(
            &parsed.source[span.start as usize..span.end as usize],
            "option"
        );
        assert!(diagnostics[0].message.contains("has no `value`"));
    }

    #[test]
    fn a_component_and_a_non_jsx_island_inside_the_select_are_warned() {
        let source =
            "fn f() { <select value={v}><Choices /><option value=\"a\"/>{extra}</select> }";
        let (_, diagnostics, _) = scan_source(source);
        let messages = messages(&diagnostics);
        assert_eq!(messages.len(), 2, "{messages:?}");
        assert!(messages[0].starts_with("`<Choices>` is inside a `<select value=…>`"));
        assert!(messages[1].starts_with("this expression is not JSX"));
    }

    #[test]
    fn a_select_with_value_and_no_options_at_all_is_warned_once() {
        let source = "fn f() { <select value={v}></select> }";
        let (_, diagnostics, _) = scan_source(source);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0]
            .message
            .starts_with("no `<option>` is written"));
    }

    #[test]
    fn a_comment_only_island_is_not_an_unreachable_option_source() {
        let source = "fn f() { <select value={v}>{/* none */}<option value=\"a\"/></select> }";
        let (_, diagnostics, _) = scan_source(source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }
}
