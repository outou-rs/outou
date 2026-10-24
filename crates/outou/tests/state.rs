//! The state API (alpha gate A5, issue #29): `State<T>`, `use_state`,
//! `provide_context` and `use_context`, driven headlessly. Components store
//! their `State` in a thread-local so the test can write to it from outside,
//! as an event handler would, and render again.

use std::cell::RefCell;

use outou::__private::dioxus_core::{Mutation, VirtualDom};
use outou::__private::{child, rsx};
use outou::prelude::*;

/// The text a render created or changed, in the order the backend reports it.
fn texts(edits: Vec<Mutation>) -> Vec<String> {
    edits
        .into_iter()
        .filter_map(|edit| match edit {
            Mutation::CreateTextNode { value, .. } | Mutation::SetText { value, .. } => Some(value),
            _ => None,
        })
        .collect()
}

/// Runs `write` in the dom's runtime, as an event handler does, and renders
/// the scopes the write made dirty.
fn write_and_render(dom: &mut VirtualDom, write: impl FnOnce()) -> Vec<String> {
    dom.in_runtime(write);
    texts(dom.render_immediate_to_vec().edits)
}

thread_local! {
    static NUMBER: RefCell<Option<State<i32>>> = const { RefCell::new(None) };
    static COUNT: RefCell<Option<State<usize>>> = const { RefCell::new(None) };
    static NAMES: RefCell<Option<State<Vec<String>>>> = const { RefCell::new(None) };
}

fn number() -> State<i32> {
    NUMBER.with(|cell| cell.borrow().expect("the component ran"))
}

fn names() -> State<Vec<String>> {
    NAMES.with(|cell| cell.borrow().expect("the component ran"))
}

#[component]
fn ReadsWithGet() -> Element {
    let n = use_state(|| 1);
    NUMBER.with(|cell| *cell.borrow_mut() = Some(n));
    rsx! { p { {child(n.get().to_string())} } }
}

#[component]
fn ReadsWithWith() -> Element {
    let n = use_state(|| 1);
    NUMBER.with(|cell| *cell.borrow_mut() = Some(n));
    let text = n.with(|value| format!("n={value}"));
    rsx! { p { {child(text)} } }
}

#[component]
fn NeverReads() -> Element {
    let n = use_state(|| 1);
    NUMBER.with(|cell| *cell.borrow_mut() = Some(n));
    rsx! { p { "static" } }
}

#[component]
fn ListState() -> Element {
    let list = use_state(Vec::<String>::new);
    NAMES.with(|cell| *cell.borrow_mut() = Some(list));
    let joined = list.with(|items| items.join(","));
    rsx! { p { {child(joined)} } }
}

#[component]
fn ChildOfState() -> Element {
    let n = use_state(|| 7usize);
    COUNT.with(|cell| *cell.borrow_mut() = Some(n));
    rsx! { p { {child(n)} } }
}

#[test]
fn get_returns_the_initial_value_and_a_clone() {
    let mut dom = VirtualDom::new(ReadsWithGet);
    assert_eq!(texts(dom.rebuild_to_vec().edits), ["1"]);
    let held = dom.in_runtime(|| number().get());
    assert_eq!(held, 1);
}

#[test]
fn set_then_render_shows_the_new_value() {
    let mut dom = VirtualDom::new(ReadsWithGet);
    dom.rebuild_in_place();
    let after = write_and_render(&mut dom, || number().set(42));
    assert_eq!(after, ["42"]);
}

#[test]
fn with_subscribes_the_component_like_get() {
    let mut dom = VirtualDom::new(ReadsWithWith);
    assert_eq!(texts(dom.rebuild_to_vec().edits), ["n=1"]);
    let after = write_and_render(&mut dom, || number().set(5));
    assert_eq!(after, ["n=5"]);
}

#[test]
fn a_component_that_never_reads_is_not_rendered_again() {
    let mut dom = VirtualDom::new(NeverReads);
    dom.rebuild_in_place();
    let after = write_and_render(&mut dom, || number().set(5));
    assert!(after.is_empty());
}

#[test]
fn update_changes_the_value_in_place_and_triggers_a_render() {
    let mut dom = VirtualDom::new(ListState);
    assert_eq!(texts(dom.rebuild_to_vec().edits), [""]);
    let after = write_and_render(&mut dom, || {
        names().update(|items| items.push("ada".to_owned()));
        names().update(|items| items.push("grace".to_owned()));
    });
    assert!(after.contains(&"ada,grace".to_owned()), "{after:?}");
}

#[component]
fn TwoStates() -> Element {
    let source = use_state(|| 2);
    let target = use_state(|| 40);
    // Another state may be read and written inside `update`.
    target.update(|value| *value += source.get());
    source.update(|value| *value += target.with(|other| *other));
    assert_eq!(target.get(), 42);
    assert_eq!(source.get(), 44);
    rsx! { p { "ok" } }
}

#[test]
fn update_does_not_panic_when_it_touches_another_state() {
    let mut dom = VirtualDom::new(TwoStates);
    dom.rebuild_in_place();
}

#[test]
fn update_returns_nothing_and_with_returns_the_closure_result() {
    let mut dom = VirtualDom::new(ReadsWithGet);
    dom.rebuild_in_place();
    dom.in_runtime(|| {
        let length = number().with(|value| value.to_string().len());
        assert_eq!(length, 1);
        number().update(|value| *value = 100);
        assert_eq!(number().with(|value| *value), 100);
    });
}

// Calls the `Clone` impl through a generic bound, so clippy's `clone_on_copy`
// (which fires on a direct clone of a `Copy` type) does not apply.
fn cloned<T: Clone>(value: &T) -> T {
    value.clone()
}

#[component]
fn Identity() -> Element {
    let first = use_state(|| 1);
    let copy = first;
    assert!(first == copy);
    assert!(first == cloned(&first));
    assert!(first != use_state(|| 1));
    rsx! { p { "ok" } }
}

#[test]
fn a_state_is_copy_clone_and_compares_by_identity() {
    fn assert_copy<T: Copy + Clone + PartialEq + 'static>() {}
    assert_copy::<State<String>>();
    assert_copy::<State<Vec<u8>>>();
    VirtualDom::new(Identity).rebuild_in_place();
}

#[test]
fn debug_prints_the_value_without_subscribing() {
    let mut dom = VirtualDom::new(NeverReads);
    dom.rebuild_in_place();
    dom.in_runtime(|| {
        assert_eq!(format!("{:?}", number()), "State(1)");
    });
}

#[component]
fn Shown(value: State<i32>) -> Element {
    rsx! { p { {child(value.get().to_string())} } }
}

#[component]
fn PassesState() -> Element {
    let n = use_state(|| 3);
    NUMBER.with(|cell| *cell.borrow_mut() = Some(n));
    rsx! { Shown { value: n } }
}

#[test]
fn a_state_can_be_a_component_prop() {
    let mut dom = VirtualDom::new(PassesState);
    assert_eq!(texts(dom.rebuild_to_vec().edits), ["3"]);
    let after = write_and_render(&mut dom, || number().set(9));
    assert_eq!(after, ["9"]);
}

#[component]
fn ReadsContext() -> Element {
    let shared = use_context::<State<i32>>();
    rsx! { p { {child(shared.get().to_string())} } }
}

#[component]
fn ProvidesContext() -> Element {
    let n = use_state(|| 11);
    NUMBER.with(|cell| *cell.borrow_mut() = Some(n));
    let provided = provide_context(|| n);
    assert!(provided == n);
    rsx! { ReadsContext {} }
}

#[test]
fn context_carries_a_state_to_a_descendant_and_it_stays_live() {
    let mut dom = VirtualDom::new(ProvidesContext);
    assert_eq!(texts(dom.rebuild_to_vec().edits), ["11"]);
    let after = write_and_render(&mut dom, || number().set(12));
    assert_eq!(after, ["12"]);
}

#[test]
fn a_state_of_a_displayable_value_is_a_child() {
    let mut dom = VirtualDom::new(ChildOfState);
    assert_eq!(texts(dom.rebuild_to_vec().edits), ["7"]);
    // `{count}` subscribes the component that renders it.
    let after = write_and_render(&mut dom, || {
        COUNT.with(|cell| cell.borrow().expect("the component ran").set(8));
    });
    assert_eq!(after, ["8"]);
}

#[derive(Clone, Debug, PartialEq)]
struct Draft {
    title: String,
    notes: String,
}

#[component]
fn F4() -> Element {
    let draft = use_state(|| Draft {
        title: "a".into(),
        notes: "n".into(),
    });
    draft.update(|d| d.title = "b".into());
    draft.set(Draft {
        notes: "m".into(),
        ..draft.get()
    });
    assert_eq!(
        draft.get(),
        Draft {
            title: "b".into(),
            notes: "m".into()
        }
    );
    rsx! { p { "ok" } }
}

#[test]
fn the_f4_pattern_is_replaced_by_update_and_by_get() {
    VirtualDom::new(F4).rebuild_in_place();
}
