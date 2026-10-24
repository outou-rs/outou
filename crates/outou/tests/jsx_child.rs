//! The accepted JSX child types (alpha gate A5, issue #27). The shapes
//! mirror what generated code does: `{::outou::__private::child(EXPR)}`
//! inside an element's body. Each test renders headlessly and reads the
//! text nodes the backend would create.

use outou::__private::dioxus_core::{Mutation, VirtualDom};
use outou::__private::{child, rsx};
use outou::prelude::*;

/// The text nodes of a first render, sorted: the backend creates them in
/// reverse document order, which is not what these tests are about.
fn texts_of(app: fn() -> Element) -> Vec<String> {
    let mut dom = VirtualDom::new(app);
    let mut texts: Vec<String> = dom
        .rebuild_to_vec()
        .edits
        .into_iter()
        .filter_map(|edit| match edit {
            Mutation::CreateTextNode { value, .. } => Some(value),
            _ => None,
        })
        .collect();
    texts.sort();
    texts
}

fn strings(values: &[&str]) -> Vec<String> {
    let mut strings: Vec<String> = values.iter().map(|value| (*value).to_owned()).collect();
    strings.sort();
    strings
}

#[component]
fn Row(label: String) -> Element {
    rsx! { li { {child(label)}, } }
}

#[test]
fn integers_and_floats_render_as_their_display_text() {
    fn app() -> Element {
        let count: usize = 2;
        let ratio = 3.5f64;
        rsx! { p { {child(count)}, {child(ratio)}, {child(-7i64)}, {child(0.25f32)}, } }
    }
    assert_eq!(texts_of(app), strings(&["2", "3.5", "-7", "0.25"]));
}

#[test]
fn an_unsuffixed_literal_and_a_char_render_as_text() {
    fn app() -> Element {
        rsx! { p { {child(42)}, {child(1.5)}, {child('x')}, } }
    }
    assert_eq!(texts_of(app), strings(&["42", "1.5", "x"]));
}

#[test]
fn a_reference_to_a_number_or_char_renders_as_text() {
    fn app() -> Element {
        let numbers = [1usize, 2];
        let ratio = 0.5f32;
        let letter = 'z';
        let (ratio, letter) = (&ratio, &letter);
        rsx! {
            p {
                {child(ratio)}, {child(letter)},
                {child(numbers.iter().map(|n| rsx! { b { {child(n)}, } }))},
            }
        }
    }
    assert_eq!(texts_of(app), strings(&["0.5", "z", "1", "2"]));
}

#[test]
fn strings_and_string_slices_render_unchanged() {
    fn app() -> Element {
        let owned = String::from("owned");
        rsx! { p { {child(owned)}, {child("literal")}, {child(format!("f{}", 1))}, } }
    }
    assert_eq!(texts_of(app), strings(&["owned", "literal", "f1"]));
}

#[test]
fn an_option_of_text_renders_its_content_or_nothing() {
    fn app() -> Element {
        let some = Some(String::from("here"));
        let none: Option<String> = None;
        rsx! { p { {child(some)}, {child(none)}, } }
    }
    assert_eq!(texts_of(app), strings(&["here"]));
}

#[test]
fn an_element_an_option_of_one_and_an_if_expression_render() {
    fn app() -> Element {
        let shown = true;
        let (plain, opt, yes, no) = ("plain", "opt", "yes", "no");
        let present: Option<Element> = Some(rsx! { b { {child(opt)}, } });
        let absent: Option<Element> = None;
        rsx! {
            div {
                {child(rsx! { i { {child(plain)}, } })},
                {child(present)},
                {child(absent)},
                {child(if shown { rsx! { em { {child(yes)}, } } } else { rsx! {} })},
                {child(if !shown { rsx! { em { {child(no)}, } } } else { rsx! {} })},
            }
        }
    }
    assert_eq!(texts_of(app), strings(&["plain", "opt", "yes"]));
}

#[test]
fn an_iterator_of_keyed_elements_renders_every_item() {
    fn app() -> Element {
        let labels = [String::from("a"), String::from("b")];
        rsx! {
            ul {
                {child(labels.iter().enumerate().map(|(index, label)| {
                    let key = outou::__private::key(&index);
                    rsx! { Row { key: "{key}", label: label.clone() } }
                }))},
            }
        }
    }
    assert_eq!(texts_of(app), strings(&["a", "b"]));
}

#[test]
fn the_unit_value_renders_nothing() {
    fn app() -> Element {
        rsx! { p { {child(())}, } }
    }
    assert!(texts_of(app).is_empty());
}
