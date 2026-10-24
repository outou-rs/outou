//! The values of `<select value={V}>` and `<option value={..}>` (issue #28).
//! The shapes mirror what generated code does: borrow the expression,
//! convert it on the spot, compare as text.

use outou::__private::{option_selected, option_value, select_value};

#[test]
fn strings_become_their_own_text() {
    assert_eq!(select_value(&("High")), "High");
    assert_eq!(select_value(&String::from("x")), "x");
    assert_eq!(option_value(&format!("p-{}", 7)), "p-7");
    assert_eq!(option_value(&"borrowed".to_owned().as_str()), "borrowed");
}

#[test]
fn integers_and_chars_become_their_display_text() {
    assert_eq!(select_value(&(7u32)), "7");
    assert_eq!(option_value(&(-3i64)), "-3");
    assert_eq!(select_value(&('c')), "c");
    assert_eq!(option_value(&std::num::NonZeroU8::new(9).unwrap()), "9");
}

#[test]
fn smart_pointers_to_strings_are_values() {
    use std::borrow::Cow;
    use std::rc::Rc;
    use std::sync::Arc;

    assert_eq!(select_value(&Cow::Borrowed("b")), "b");
    assert_eq!(option_value(&Box::<str>::from("box")), "box");
    assert_eq!(option_value(&Rc::<str>::from("rc")), "rc");
    assert_eq!(select_value(&Arc::<str>::from("arc")), "arc");
}

#[test]
fn an_option_is_selected_when_its_text_equals_the_selects() {
    let selected = select_value(&("Medium"));
    assert!(option_selected(&selected, &option_value(&("Medium"))));
    assert!(!option_selected(&selected, &option_value(&("High"))));
}

#[test]
fn comparison_is_on_text_so_a_number_matches_its_decimal_form() {
    let selected = select_value(&(2u8));
    assert!(option_selected(&selected, &option_value(&("2"))));
    assert!(option_selected(&selected, &option_value(&(2i64))));
    assert!(!option_selected(&selected, &option_value(&(3u8))));
}

#[test]
fn comparison_is_exact_not_trimmed_or_case_folded() {
    let selected = select_value(&("a"));
    assert!(!option_selected(&selected, &option_value(&("A"))));
    assert!(!option_selected(&selected, &option_value(&(" a"))));
}
