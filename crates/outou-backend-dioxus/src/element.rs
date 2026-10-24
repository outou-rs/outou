//! Lowering a [`JsxElement`] to Dioxus `rsx!` element syntax.
//!
//! Elements and components share exactly one lowering path: Dioxus's
//! `rsx!` itself decides "component" vs. "element" from the identifier's
//! first letter, the same rule Outou's own grammar uses (grammar §5), so
//! codegen never has to branch on it.

use outou_codegen::Writer;
use outou_sourcemap::{MappingKind, Span};
use outou_syntax::ast::{Island, JsxAttributeValue, JsxChild, JsxElement, JsxTag};

use crate::attribute::{is_single_expression, lower_attribute};
use crate::bindings::CallBindings;
use crate::escape::rust_string_literal;
use crate::expr::lower_parts;
use crate::recovery::emit_recovery_element;
use crate::{Cx, PRIVATE_PATH};

/// Lowers a JSX element that appears in Rust expression position — a
/// statement, a tail expression, or inside an island — to
/// `::outou::__private::rsx! { … }`.
///
/// A broken element (grammar §9: the opening tag never recovered a name)
/// cannot be reconstructed as `Name { … }` at all, so it is replaced
/// wholesale by the recovery placeholder instead of being wrapped in
/// `rsx!` (there is no element syntax to put inside one).
pub fn lower_element_as_rsx(writer: &mut Writer, source: &str, element: &JsxElement, cx: Cx<'_>) {
    if is_broken(element) {
        emit_recovery_element(writer, element.span);
        return;
    }
    // MEDIUM-13, issue #6 fix list item 8: Dioxus reports macro-level
    // errors at the `rsx!` invocation site itself, not at any span
    // inside it. Mapping this prefix and the closing brace to the
    // element's own span (rather than leaving them entirely unmapped, as
    // `Writer::raw` would) is what lets such an error land back on the
    // JSX element instead of nowhere.
    //
    // Issue #26: the call's keys are evaluated first and bound in a block
    // around it (`crate::key`); the `rsx!` text then refers to them by a
    // synthesized identifier. The block's own braces map like the call's.
    let bindings = CallBindings::collect(element, cx.selects);
    if !bindings.is_empty() {
        writer.mapped("{ ", &[element.span], MappingKind::Other, None);
        bindings.write_bindings(writer, source, cx);
    }
    let prefix = format!("{PRIVATE_PATH}::rsx! {{ ");
    writer.mapped(&prefix, &[element.span], MappingKind::Other, None);
    lower_element_body(writer, source, element, &bindings, cx);
    writer.mapped(" }", &[element.span], MappingKind::Other, None);
    if !bindings.is_empty() {
        writer.mapped(" }", &[element.span], MappingKind::Other, None);
    }
}

/// Lowers a nested element or component already inside an `rsx!` call (a
/// child of another element) to `Name { attr: value, …, children }`.
fn lower_element_body(
    writer: &mut Writer,
    source: &str,
    element: &JsxElement,
    bindings: &CallBindings,
    cx: Cx<'_>,
) {
    if is_broken(element) {
        emit_child_recovery_element(writer, element.span);
        return;
    }

    let JsxTag::Named { name, .. } = &element.open else {
        unreachable!("is_broken already handled the only other JsxTag variant");
    };
    let mut sources = vec![name.span];
    if let Some(JsxTag::Named {
        name: close_name, ..
    }) = &element.close
    {
        sources.push(close_name.span);
    }
    writer.mapped(&name.name, &sources, MappingKind::Identifier, None);
    writer.raw(" {");

    for attribute in &element.attributes {
        // Decision (docs/phase0/issues/06-dioxus-backend-facade.md): a
        // broken attribute value is dropped along with its name — there
        // is nothing safe to splice for `name={<broken>}`, and dropping
        // the whole attribute (rather than e.g. `name: recovery()`) keeps
        // recovery output free of attributes typed against the wrong
        // shape while the user is still typing the value.
        if matches!(attribute.value, Some(JsxAttributeValue::Error(_))) {
            continue;
        }
        writer.raw(" ");
        lower_attribute(writer, source, attribute, bindings, cx);
        writer.raw(",");
    }
    // Issue #28: an `<option>` inside a `<select value>` is selected when its
    // value is the select's.
    if let Some(text) = bindings.selected_attribute(element, cx.selects) {
        writer.raw(&format!(" {text},"));
    }

    for child in &element.children {
        writer.raw(" ");
        lower_child(writer, source, child, bindings, cx);
    }

    writer.raw(" }");
}

/// The recovery placeholder in child position. A bare path call there is not
/// valid `rsx!` syntax (it expects `name {` or `{expr}`), so it is braced the
/// way every other child expression is.
fn emit_child_recovery_element(writer: &mut Writer, span: Span) {
    writer.raw("{");
    emit_recovery_element(writer, span);
    writer.raw("}");
}

/// A JSX element is unrecoverable as element syntax only when its opening
/// tag itself never resolved to a name (grammar §9). Everything else —
/// including a missing or mismatched closing tag — is still fully
/// reconstructible, because Dioxus's own syntax closes an element with a
/// synthesized `}`, never a textual closing tag; `close` only ever
/// contributes an *extra* source span for the identifier's mapping.
fn is_broken(element: &JsxElement) -> bool {
    matches!(element.open, JsxTag::Incomplete(_))
}

fn lower_child(
    writer: &mut Writer,
    source: &str,
    child: &JsxChild,
    bindings: &CallBindings,
    cx: Cx<'_>,
) {
    match child {
        JsxChild::Text(text) => {
            let literal = rust_string_literal(&text.value);
            writer.mapped(&literal, &[text.span], MappingKind::Text, None);
            writer.raw(",");
        }
        JsxChild::Expression(island) => {
            lower_child_island(writer, source, island, cx);
            writer.raw(",");
        }
        JsxChild::Element(nested) => {
            lower_element_body(writer, source, nested, bindings, cx);
            writer.raw(",");
        }
        JsxChild::Error(error) => {
            emit_child_recovery_element(writer, error.span);
            writer.raw(",");
        }
    }
}

/// Lowers a child expression `{ … }` (issue #27) to
/// `{::outou::__private::child(EXPR)}`, so the facade's `IntoChild`, not
/// the backend's own conversion, decides what may be a child.
///
/// A single expression is passed as the argument as it is written
/// (`child(count)`, `child(if c { <a/> } else { <b/> })`), with no braces of
/// its own. A block would end the life of the temporaries in its tail early
/// (2024 edition: `{String::from("a").as_str()}` is E0716), and a brace-less
/// argument is only valid Rust for one expression, see
/// [`crate::expr_shape`]. Every other shape (statements with or without a
/// `;`, an item, an empty or comment-only island, a broken part) keeps its
/// braces: `child({ … })`, where the block's value is the child and an empty
/// block is the unit value, which renders nothing.
fn lower_child_island(writer: &mut Writer, source: &str, island: &Island, cx: Cx<'_>) {
    writer.raw(&format!("{{{PRIVATE_PATH}::child("));
    if is_single_expression(island) {
        lower_parts(writer, source, &island.parts, island.span, cx);
    } else {
        lower_island(writer, source, island, cx);
    }
    writer.raw(")}");
}

/// Lowers a `{ … }` island exactly as it was written: the braces are
/// reproduced (synthesized — they carry no meaningful source span of their
/// own beyond the one byte each) and the content is lowered part by part,
/// so a JSX element nested inside an `if`/`match`/closure becomes its own
/// `rsx! { … }` call and the island keeps whatever type Dioxus needs
/// there (`Element`, `bool`, an iterator, …).
pub fn lower_island(writer: &mut Writer, source: &str, island: &Island, cx: Cx<'_>) {
    writer.raw("{");
    lower_parts(writer, source, &island.parts, island.span, cx);
    writer.raw("}");
}
