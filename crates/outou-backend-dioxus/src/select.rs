//! Lowering of `<select value={V}>` and the `<option>` elements it selects
//! (alpha gate A5, issue #28).
//!
//! The backend applies a select's `value` before the select's options exist,
//! so the browser keeps showing the first option on mount; React selects the
//! matching option. Outou gives the JSX the React meaning (ADR 0014): the
//! options that are lexically inside a `<select value>` (found by
//! [`outou_syntax::select::scan`]) get a `selected` computed from the value.
//!
//! The select's value and each such option's value are evaluated first and
//! bound, like a `key` (`crate::key`), in the block around the `rsx!` call
//! that contains the element:
//!
//! ```text
//! { #[allow(clippy::borrow_deref_ref)] let __outou_select_17 = &::outou::__private::select_value(&(V));
//!   #[allow(clippy::borrow_deref_ref)] let __outou_opt_52 = ::outou::__private::option_value(&(OPTV));
//!   let __outou_selected_52 =
//!       ::outou::__private::option_selected(__outou_select_17, &__outou_opt_52);
//!   ::outou::__private::rsx! {
//!       select { value: "{__outou_select_17}",
//!           option { value: "{__outou_opt_52}", selected: __outou_selected_52, } } } }
//! ```
//!
//! An option written in an island (`.map(|p| <option …>)`) is part of another
//! `rsx!` call, so its binding is in that call's block and refers to the
//! select's binding of the outer block. The select's binding is a reference,
//! so a `move` closure copies it instead of moving a `String`.
//!
//! (The comparison is bound too because the backend's attribute syntax does
//! not accept a value that starts with `::`.)
//!
//! Every user expression appears exactly once in the generated text (the
//! option's value is bound, then the binding is used for `value` and for
//! `selected`), so its type errors, lints and source-map entries are not
//! duplicated. Binding names carry the start offset of the element in the
//! `.rsx` file, which is unique in the file, so an island's block can never
//! shadow the select binding an option inside it refers to.
//!
//! Evaluation order: all keys of a call, then every select value and option
//! value of the call in source order, then the rest of the call
//! (`docs/grammar.md`, "`<select value>`").

use outou_codegen::Writer;
use outou_sourcemap::MappingKind;
use outou_syntax::ast::{Island, JsxAttribute, JsxAttributeValue, JsxChild, JsxElement, JsxTag};
use outou_syntax::select::{option_value_attribute, select_value_attribute, SelectScan};

use crate::attribute::is_single_expression;
use crate::element::lower_island;
use crate::escape::plain_rust_string_literal;
use crate::expr::lower_parts;
use crate::key::BINDING_LINT_ALLOW;
use crate::{Cx, PRIVATE_PATH};

const SELECT_PREFIX: &str = outou_syntax::reserved::SELECT_PREFIX;
const OPTION_PREFIX: &str = outou_syntax::reserved::OPTION_PREFIX;
const SELECTED_PREFIX: &str = outou_syntax::reserved::SELECTED_PREFIX;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Select,
    Option,
}

struct Entry<'a> {
    kind: Kind,
    element: &'a JsxElement,
    value: &'a JsxAttribute,
}

/// The select values and option values of one `rsx!` call, in source order.
pub struct SelectBindings<'a> {
    entries: Vec<Entry<'a>>,
}

impl<'a> SelectBindings<'a> {
    /// Collects them for the call that `element` starts: its own tree and the
    /// elements written directly as children, not the islands (separate
    /// calls).
    pub fn collect(element: &'a JsxElement, scan: &SelectScan) -> Self {
        let mut entries = Vec::new();
        collect_into(element, scan, &mut entries);
        Self { entries }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The binding that replaces `attribute`'s value, when `attribute` is the
    /// `value` of one of this call's selects or options.
    pub fn identifier(&self, attribute: &JsxAttribute) -> Option<String> {
        self.entries
            .iter()
            .find(|entry| std::ptr::eq(entry.value, attribute))
            .map(|entry| binding_name(entry.kind, entry.element))
    }

    /// The generated `selected` attribute (`selected: …`) of `option`, when
    /// it is one of this call's rewritten options. It has no source mapping:
    /// it only names a binding whose own text cannot fail to compile.
    pub fn selected_attribute(&self, option: &JsxElement, scan: &SelectScan) -> Option<String> {
        self.entries
            .iter()
            .find(|entry| entry.kind == Kind::Option && std::ptr::eq(entry.element, option))?;
        scan.select_of(option)?;
        Some(format!("selected: {SELECTED_PREFIX}{}", option.span.start))
    }

    /// Writes one `let` per select value and option value.
    pub fn write_bindings(&self, writer: &mut Writer, source: &str, cx: Cx<'_>) {
        let scan = cx.selects;
        for entry in &self.entries {
            let (reference, function) = match entry.kind {
                Kind::Select => ("&", "select_value"),
                Kind::Option => ("", "option_value"),
            };
            writer.raw(&format!(
                "{BINDING_LINT_ALLOW}let {} = {reference}{PRIVATE_PATH}::{function}(&(",
                binding_name(entry.kind, entry.element)
            ));
            write_value(writer, source, entry.value, cx);
            writer.raw(")); ");
            if let (Kind::Option, Some(select)) = (entry.kind, scan.select_of(entry.element)) {
                writer.raw(&format!(
                    "let {SELECTED_PREFIX}{} = {PRIVATE_PATH}::option_selected({SELECT_PREFIX}{select}, &{}); ",
                    entry.element.span.start,
                    binding_name(Kind::Option, entry.element)
                ));
            }
        }
    }
}

fn binding_name(kind: Kind, element: &JsxElement) -> String {
    let prefix = match kind {
        Kind::Select => SELECT_PREFIX,
        Kind::Option => OPTION_PREFIX,
    };
    format!("{prefix}{}", element.span.start)
}

/// Writes the value of `attribute` as one expression: as written when it is
/// one (`crate::expr_shape`), in braces otherwise (statements in `{ … }`).
fn write_value(writer: &mut Writer, source: &str, attribute: &JsxAttribute, cx: Cx<'_>) {
    match &attribute.value {
        Some(JsxAttributeValue::Expression(island)) => write_island(writer, source, island, cx),
        Some(JsxAttributeValue::Text(text)) => {
            let literal = plain_rust_string_literal(&text.value);
            writer.mapped(&literal, &[text.span], MappingKind::Expression, None);
        }
        // A bare `value` is the boolean `true` (grammar §5.1), which is not a
        // value type, so the facade reports it as such.
        None => {
            writer.mapped(
                "true",
                &[attribute.name.span],
                MappingKind::Expression,
                None,
            );
        }
        Some(JsxAttributeValue::Error(_)) => {
            unreachable!("the scan skips broken attribute values")
        }
    }
}

fn write_island(writer: &mut Writer, source: &str, island: &Island, cx: Cx<'_>) {
    if is_single_expression(island) {
        lower_parts(writer, source, &island.parts, island.span, cx);
    } else {
        lower_island(writer, source, island, cx);
    }
}

fn collect_into<'a>(element: &'a JsxElement, scan: &SelectScan, out: &mut Vec<Entry<'a>>) {
    if matches!(element.open, JsxTag::Incomplete(_)) {
        return;
    }
    if let Some(value) = select_value_attribute(element) {
        out.push(Entry {
            kind: Kind::Select,
            element,
            value,
        });
    } else if scan.select_of(element).is_some() {
        if let Some(value) = option_value_attribute(element) {
            out.push(Entry {
                kind: Kind::Option,
                element,
                value,
            });
        }
    }
    for child in &element.children {
        if let JsxChild::Element(nested) = child {
            collect_into(nested, scan, out);
        }
    }
}
