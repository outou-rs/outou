//! The accepted `key` types (issue #26). The shapes mirror what generated
//! code does: borrow the key expression, convert it on the spot.

use outou::__private::key;

#[test]
fn strings_become_their_own_text() {
    assert_eq!(key(&("new")), "new");
    assert_eq!(key(&String::from("x")), "x");
    assert_eq!(key(&format!("edit-{}", 7)), "edit-7");
    assert_eq!(key(&"borrowed".to_owned().as_str()), "borrowed");
}

#[test]
fn integers_become_their_decimal_text() {
    assert_eq!(key(&(7u32)), "7");
    assert_eq!(key(&(-3i64)), "-3");
    assert_eq!(key(&(0usize)), "0");
    assert_eq!(key(&(u128::MAX)), u128::MAX.to_string());
}

#[test]
fn the_original_is_only_borrowed() {
    let id = String::from("a");
    assert_eq!(key(&(id)), "a");
    // Still usable, e.g. moved into a prop after the key was taken.
    let moved = id;
    assert_eq!(moved, "a");
}

#[test]
fn smart_pointers_to_strings_and_mutable_references_are_keys() {
    use std::borrow::Cow;
    use std::rc::Rc;
    use std::sync::Arc;

    assert_eq!(key(&Cow::Borrowed("b")), "b");
    assert_eq!(key(&Cow::<str>::Owned("o".to_owned())), "o");
    assert_eq!(key(&Box::<str>::from("box")), "box");
    assert_eq!(key(&Rc::<str>::from("rc")), "rc");
    assert_eq!(key(&Arc::<str>::from("arc")), "arc");

    let mut text = String::from("m");
    let mut number = 5u8;
    assert_eq!(key(&(&mut text)), "m");
    assert_eq!(key(&(&mut number)), "5");
    assert_eq!(key(&(&mut text.as_mut_str())), "m");
}

#[test]
fn non_zero_integers_become_their_decimal_text() {
    use std::num::{NonZeroI128, NonZeroI8, NonZeroIsize, NonZeroU128, NonZeroU32, NonZeroUsize};

    assert_eq!(key(&NonZeroU32::new(9).unwrap()), "9");
    assert_eq!(key(&NonZeroI8::new(-4).unwrap()), "-4");
    assert_eq!(key(&NonZeroUsize::new(1).unwrap()), "1");
    assert_eq!(key(&NonZeroIsize::new(-1).unwrap()), "-1");
    assert_eq!(
        key(&NonZeroU128::new(u128::MAX).unwrap()),
        u128::MAX.to_string()
    );
    assert_eq!(
        key(&NonZeroI128::new(i128::MIN).unwrap()),
        i128::MIN.to_string()
    );
}
