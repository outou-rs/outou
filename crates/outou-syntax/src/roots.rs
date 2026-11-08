//! The roots of one lowered call, and the `key` rule that follows from them
//! (ADR 0015).
//!
//! A JSX expression is lowered as one call. A fragment adds no node of its
//! own, so a call's roots are the element itself, or, for a fragment, its
//! children with every nested fragment spliced in, whatever their kind:
//! text and expressions are roots like elements.
//!
//! The backend gives the key of a list item to the first root only: a `key`
//! on any later root is not used (`docs/backend-leakage.md` row 63). Outou
//! reports it in its own words and the backend leaves it out of the
//! generated code, so no backend diagnostic reaches the user.

use crate::ast::{unraw_name, File, JsxAttribute, JsxAttributeValue, JsxChild, JsxElement, JsxTag};
use crate::{Diagnostic, Severity};

const KEY: &str = "key";

/// One root of a call.
#[derive(Debug, Clone, Copy)]
pub enum Root<'a> {
    /// An element or component.
    Element(&'a JsxElement),
    /// Text, an expression island or an error node.
    Other,
}

/// The roots of the call that `element` starts, in source order.
pub fn call_roots(element: &JsxElement) -> Vec<Root<'_>> {
    let mut roots = Vec::new();
    if matches!(element.open, JsxTag::Fragment { .. }) {
        splice(&element.children, &mut roots);
    } else {
        roots.push(Root::Element(element));
    }
    roots
}

fn splice<'a>(children: &'a [JsxChild], roots: &mut Vec<Root<'a>>) {
    for child in children {
        match child {
            JsxChild::Element(nested) if matches!(nested.open, JsxTag::Fragment { .. }) => {
                splice(&nested.children, roots);
            }
            JsxChild::Element(nested) => roots.push(Root::Element(nested)),
            JsxChild::Text(_) | JsxChild::Expression(_) | JsxChild::Error(_) => {
                roots.push(Root::Other);
            }
        }
    }
}

/// The usable `key` attribute of `element`, if it has one.
pub fn key_attribute(element: &JsxElement) -> Option<&JsxAttribute> {
    element.attributes.iter().find(|attribute| {
        unraw_name(&attribute.name.name) == KEY
            && !matches!(attribute.value, Some(JsxAttributeValue::Error(_)))
    })
}

/// The `key` attributes of the call that `element` starts that sit on a
/// root other than the first: the ones the backend does not use.
pub fn ignored_keys(element: &JsxElement) -> Vec<&JsxAttribute> {
    ignored_key_owners(element)
        .into_iter()
        .map(|(_, attribute)| attribute)
        .collect()
}

/// [`ignored_keys`], each with the element that carries it.
fn ignored_key_owners(element: &JsxElement) -> Vec<(&JsxElement, &JsxAttribute)> {
    call_roots(element)
        .into_iter()
        .skip(1)
        .filter_map(|root| match root {
            Root::Element(root) => key_attribute(root).map(|attribute| (root, attribute)),
            Root::Other => None,
        })
        .collect()
}

/// Whether `element` has more than one `key`: that is already reported as a
/// duplicate attribute, and a second message about the same key is noise.
fn has_duplicate_key(element: &JsxElement) -> bool {
    element
        .attributes
        .iter()
        .filter(|attribute| unraw_name(&attribute.name.name) == KEY)
        .count()
        > 1
}

/// A warning for each `key` that is not on the first root of a call.
pub fn check(file: &File) -> Vec<Diagnostic> {
    file.jsx_calls()
        .into_iter()
        .flat_map(ignored_key_owners)
        .filter(|(owner, _)| !has_duplicate_key(owner))
        .map(|(_, attribute)| Diagnostic {
            span: attribute.span,
            message: "this `key` has no effect: in a fragment only the first element can carry the `key` of the item; move the `key` to the first element, or wrap the items in an element and give that element the `key`".to_string(),
            severity: Severity::Warning,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn warnings(source: &str) -> Vec<String> {
        check(&crate::parse(source).file)
            .into_iter()
            .map(|d| d.message)
            .collect()
    }

    #[test]
    fn a_key_on_the_first_root_is_used_and_on_a_later_root_is_not() {
        let ok = "fn f() { <><li key=\"a\"/><li/></> }";
        assert!(warnings(ok).is_empty());
        let later = "fn f() { <><li/><li key=\"b\"/></> }";
        assert_eq!(warnings(later).len(), 1);
    }

    #[test]
    fn text_before_an_element_makes_it_a_later_root() {
        assert_eq!(warnings("fn f() { <>\"t\"<li key=\"b\"/></> }").len(), 1);
    }

    #[test]
    fn nested_fragments_are_spliced_and_an_empty_one_has_no_root() {
        assert!(warnings("fn f() { <><></><li key=\"a\"/></> }").is_empty());
        assert_eq!(
            warnings("fn f() { <><li/><><b key=\"a\"/></></> }").len(),
            1,
            "the nested root is the second root of the call"
        );
    }

    #[test]
    fn keys_below_the_roots_are_not_roots() {
        let source = "fn f() { <><ul><li key=\"a\"/><li key=\"b\"/></ul><p/></> }";
        assert!(warnings(source).is_empty());
    }

    #[test]
    fn a_fragment_child_of_an_element_is_not_a_call_of_its_own() {
        let source = "fn f() { <ul><><li key=\"a\"/><li key=\"b\"/></></ul> }";
        assert!(warnings(source).is_empty());
    }

    #[test]
    fn a_map_closure_fragment_is_its_own_call() {
        let source = "fn f() { <ul>{xs.map(|x| <><li/><li key={x}/></>)}</ul> }";
        assert_eq!(warnings(source).len(), 1);
    }

    #[test]
    fn a_duplicated_key_is_reported_once_as_a_duplicate_attribute() {
        let source = "fn f() { <><i/><i key=\"a\" key=\"b\"/></> }";
        assert!(warnings(source).is_empty());
        let messages: Vec<String> = crate::parse(source)
            .diagnostics
            .into_iter()
            .map(|d| d.message)
            .collect();
        assert_eq!(messages, ["duplicate attribute `key` on this tag"]);
    }

    #[test]
    fn a_raw_key_counts() {
        assert_eq!(warnings("fn f() { <><i/><i r#key=\"x\"/></> }").len(), 1);
    }
}
