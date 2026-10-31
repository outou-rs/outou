//! Lowering of the `key` attribute (alpha gate A5, issue #26).
//!
//! The backend only accepts a key as a format-string literal
//! (`key: "{value}"`). Splicing the user's expression into that literal
//! broke a static string (`key="new"`) and any expression containing a
//! `}` (`key={format!("edit-{id}")}`), and made the language server refuse
//! to rename inside a key (the literal and the expression differ in
//! length). Instead every key of one `rsx!` call is evaluated first and
//! bound to a synthesized identifier:
//!
//! ```text
//! { let __outou_key_0 = ::outou::__private::key(&(EXPR));
//!   ::outou::__private::rsx! { … key: "{__outou_key_0}" … } }
//! ```
//!
//! Only the synthesized identifier ever reaches the format string. The
//! user's expression is copied verbatim into the `let`, so it keeps its
//! source mapping (hover, rename, references) and is type-checked against
//! the facade's `IntoKey`, which owns the diagnostic for an unsupported
//! key type. A static string takes the same path.
//!
//! One `rsx!` call corresponds to one block: the element in expression
//! position plus every element written directly as a child of it. An
//! island (`{ … }`) holding JSX is a separate `rsx!` call and so is a
//! separate block with its own numbering, which may shadow an outer name
//! only inside that island's code, where the outer binding is never used.
//!
//! Evaluation order: all keys of a call are evaluated once, in source
//! order, before any other attribute or child of that call
//! (`docs/grammar.md`, "The `key` attribute").

use outou_codegen::Writer;
use outou_sourcemap::MappingKind;
use outou_syntax::ast::{
    unraw_name, JsxAttribute, JsxAttributeValue, JsxChild, JsxElement, JsxTag,
};

use crate::escape::plain_rust_string_literal;
use crate::expr::lower_parts;
use crate::Cx;
use crate::PRIVATE_PATH;

/// The attribute name that is lowered as a key.
const KEY_ATTRIBUTE: &str = "key";

/// Prefix of every synthesized key binding. Names starting with `__` are
/// filtered out of completion by the language server.
const BINDING_PREFIX: &str = outou_syntax::reserved::KEY_PREFIX;

/// Allows the one lint the generated `key(&(EXPR))` can trip: an `EXPR` that
/// is itself a dereference (`key={*n}`) becomes `&(*n)`, which Clippy's
/// `borrow_deref_ref` flags in the user's `outou cargo clippy`. The allow is
/// on the generated `let` only, and so also covers a reborrow the user writes
/// inside that expression.
pub(crate) const BINDING_LINT_ALLOW: &str = "#[allow(clippy::borrow_deref_ref)] ";

/// The `key` attributes of one `rsx!` call, in source order.
pub struct KeyBindings<'a> {
    attributes: Vec<&'a JsxAttribute>,
}

impl<'a> KeyBindings<'a> {
    /// Collects the keys of the `rsx!` call that `element` starts: its own
    /// and those of elements written directly as its children. Does not
    /// look inside islands, which are separate calls.
    pub fn collect(element: &'a JsxElement) -> Self {
        let mut attributes = Vec::new();
        collect_into(element, &mut attributes);
        Self { attributes }
    }

    /// Whether the call has no key at all (and so needs no block).
    pub fn is_empty(&self) -> bool {
        self.attributes.is_empty()
    }

    /// The synthesized identifier bound to `attribute`'s value, or `None`
    /// when `attribute` is not a key of this call.
    pub fn identifier(&self, attribute: &JsxAttribute) -> Option<String> {
        self.attributes
            .iter()
            .position(|candidate| std::ptr::eq(*candidate, attribute))
            .map(binding_name)
    }

    /// Writes one `let` per key.
    pub fn write_bindings(&self, writer: &mut Writer, source: &str, cx: Cx<'_>) {
        for (index, attribute) in self.attributes.iter().enumerate() {
            writer.raw(&format!(
                "{BINDING_LINT_ALLOW}let {} = {PRIVATE_PATH}::key(&(",
                binding_name(index)
            ));
            match &attribute.value {
                Some(JsxAttributeValue::Expression(island)) => {
                    lower_parts(writer, source, &island.parts, island.span, cx);
                }
                Some(JsxAttributeValue::Text(text)) => {
                    let literal = plain_rust_string_literal(&text.value);
                    writer.mapped(&literal, &[text.span], MappingKind::Expression, None);
                }
                // A bare `key` is the boolean `true`, as for any other
                // attribute (grammar §5.1); it is not a valid key type, so
                // the facade reports it as one.
                None => {
                    writer.mapped(
                        "true",
                        &[attribute.name.span],
                        MappingKind::Expression,
                        None,
                    );
                }
                Some(JsxAttributeValue::Error(_)) => {
                    unreachable!("collect_into skips broken attribute values")
                }
            }
            writer.raw(")); ");
        }
    }
}

fn binding_name(index: usize) -> String {
    format!("{BINDING_PREFIX}{index}")
}

fn collect_into<'a>(element: &'a JsxElement, out: &mut Vec<&'a JsxAttribute>) {
    if matches!(element.open, JsxTag::Incomplete(_)) {
        return;
    }
    for attribute in &element.attributes {
        let is_key = unraw_name(&attribute.name.name) == KEY_ATTRIBUTE
            && !matches!(attribute.value, Some(JsxAttributeValue::Error(_)));
        if is_key {
            out.push(attribute);
        }
    }
    for child in &element.children {
        if let JsxChild::Element(nested) = child {
            collect_into(nested, out);
        }
    }
}
