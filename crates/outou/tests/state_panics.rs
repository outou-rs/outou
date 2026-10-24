//! What goes wrong with a `State` at run time (issue #29): the panic says so
//! in Outou's words and points at the line in the user's code, and the table
//! of what may be done inside `with` and `update` is exactly the documented
//! one.

use std::cell::RefCell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;

use outou::__private::dioxus_core::VirtualDom;
use outou::__private::{child, rsx};
use outou::prelude::*;

/// A panic as the test sees it: its message and the file it is reported at.
struct Caught {
    message: String,
    file: String,
}

thread_local! {
    static LAST_FILE: RefCell<Option<String>> = const { RefCell::new(None) };
    static NUMBER: RefCell<Option<State<i32>>> = const { RefCell::new(None) };
    static OTHER: RefCell<Option<State<i32>>> = const { RefCell::new(None) };
}

/// The panic hook is global to the process, so one test at a time installs it.
static HOOK: Mutex<()> = Mutex::new(());

/// Runs `run` and returns the panic it raised, if any.
fn catch(run: impl FnOnce()) -> Option<Caught> {
    let _serial = HOOK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| {
        let file = info.location().map(|l| l.file().to_owned());
        LAST_FILE.with(|cell| *cell.borrow_mut() = file);
    }));
    let result = catch_unwind(AssertUnwindSafe(run));
    std::panic::set_hook(previous);
    let payload = result.err()?;
    let message = payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_default();
    let file = LAST_FILE
        .with(|cell| cell.borrow_mut().take())
        .unwrap_or_default();
    Some(Caught { message, file })
}

#[component]
fn Holder() -> Element {
    let a = use_state(|| 1);
    let b = use_state(|| 2);
    NUMBER.with(|cell| *cell.borrow_mut() = Some(a));
    OTHER.with(|cell| *cell.borrow_mut() = Some(b));
    rsx! { p { {child(a.get().to_string())} } }
}

fn a() -> State<i32> {
    NUMBER.with(|cell| cell.borrow().expect("the component ran"))
}

fn b() -> State<i32> {
    OTHER.with(|cell| cell.borrow().expect("the component ran"))
}

/// A dom whose runtime is current while `run` runs.
fn in_dom(run: impl FnOnce()) -> Option<Caught> {
    let mut dom = VirtualDom::new(Holder);
    dom.rebuild_in_place();
    catch(|| dom.in_runtime(run))
}

fn assert_outou_words(caught: &Caught) {
    for backend in ["AlreadyBorrowed", "Generational", "generational", "dioxus"] {
        assert!(
            !caught.message.contains(backend),
            "backend vocabulary `{backend}` in: {}",
            caught.message
        );
    }
    assert!(caught.message.contains("`State`"), "{}", caught.message);
}

/// A name, whether it panics, and the code to run.
type Case = (&'static str, bool, Box<dyn Fn()>);

/// Every cell of the documented table: what panics inside `with` / `update`
/// of the same state, and what does not.
#[test]
fn the_documented_table_of_reentrancy_holds() {
    let cases: Vec<Case> = vec![
        (
            "with { get }",
            false,
            Box::new(|| {
                a().with(|_| {
                    let _ = a().get();
                })
            }),
        ),
        (
            "with { with }",
            false,
            Box::new(|| a().with(|_| a().with(|_| ()))),
        ),
        ("with { set }", true, Box::new(|| a().with(|_| a().set(3)))),
        (
            "with { update }",
            true,
            Box::new(|| a().with(|_| a().update(|_| ()))),
        ),
        (
            "update { get }",
            true,
            Box::new(|| {
                a().update(|_| {
                    let _ = a().get();
                })
            }),
        ),
        (
            "update { with }",
            true,
            Box::new(|| a().update(|_| a().with(|_| ()))),
        ),
        (
            "update { set }",
            true,
            Box::new(|| a().update(|_| a().set(3))),
        ),
        (
            "update { update }",
            true,
            Box::new(|| a().update(|_| a().update(|_| ()))),
        ),
        (
            "update { Debug }",
            true,
            Box::new(|| {
                a().update(|_| {
                    let _ = format!("{:?}", a());
                })
            }),
        ),
        (
            "update { other get }",
            false,
            Box::new(|| a().update(|v| *v = b().get())),
        ),
        (
            "update { other set }",
            false,
            Box::new(|| a().update(|v| b().set(*v))),
        ),
        (
            "with { other set }",
            false,
            Box::new(|| a().with(|v| b().set(*v))),
        ),
    ];
    for (name, panics, case) in cases {
        let caught = in_dom(case);
        assert_eq!(
            caught.is_some(),
            panics,
            "{name}: {:?}",
            caught.map(|c| c.message)
        );
        if let Some(caught) = caught {
            assert_outou_words(&caught);
            assert!(
                caught.message.contains("already being read or updated"),
                "{name}"
            );
        }
    }
}

#[test]
fn the_f4_shape_inside_update_panics_in_outou_words() {
    let caught = in_dom(|| a().update(|v| *v = a().get() + 1)).expect("panics");
    assert_outou_words(&caught);
}

#[test]
fn a_panic_is_reported_at_the_line_of_the_users_call() {
    let reported = [
        in_dom(|| a().with(|_| a().set(1))).expect("set panics"),
        in_dom(|| {
            a().update(|_| {
                let _ = a().get();
            })
        })
        .expect("get panics"),
        in_dom(|| a().update(|_| a().with(|_| ()))).expect("with panics"),
        in_dom(|| a().update(|_| a().update(|_| ()))).expect("update panics"),
    ];
    for caught in reported {
        assert!(
            caught.file.ends_with("state_panics.rs"),
            "reported at `{}`, not at the user's code",
            caught.file
        );
    }
}
