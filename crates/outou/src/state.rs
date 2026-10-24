//! Component state under Outou's names (alpha gate A5, issue #29, ADR 0014).
//!
//! [`State<T>`] is a handle to a value that lives as long as the component
//! that created it. It is `Copy`, so an event handler takes it with `move`
//! and the component keeps using it, and it can be passed to a child
//! component as a prop.
//!
//! The API never hands out a guard. Every read and write is a method that
//! takes `&self` and finishes inside the call, so a borrow of the value
//! cannot be held across a later write. `read()` and `write()` do not exist
//! on purpose: with a guard in hand, `state.set(T { f, ..state.read().clone() })`
//! borrows the state twice and fails to compile.
//!
//! | To | Write |
//! |---|---|
//! | read a clone | `state.get()` (`T: Clone`) |
//! | read without cloning | `state.with(\|value\| value.len())` |
//! | replace the value | `state.set(next)` |
//! | change it in place | `state.update(\|value\| value.push(x))` |
//! | change one field | `state.update(\|draft\| draft.title = title)` or `state.set(Draft { title, ..state.get() })` |
//!
//! # Re-rendering
//!
//! `get` and `with` *subscribe* the code that calls them: when they run while
//! a component renders, that component renders again after the next `set` or
//! `update` of the same state. Called from an event handler, they only read.
//! `set` and `update` notify the subscribers, whether or not the value
//! changed. `set` does not compare the old and the new value. This is the
//! semantics of the current backend; Outou does not define its own.
//!
//! Hooks (`use_state`, `provide_context`, `use_context`) must be called in the
//! same order on every render of a component, never inside a condition, a
//! loop or after an early return.
//!
//! # Reading and writing the same state twice
//!
//! The value is borrowed for the whole of a `with` or `update` call, so what
//! may be done inside the closure to *the same state* is limited, and the
//! rest is a run-time panic (it cannot be a compile error):
//!
//! | Inside the closure of | `get` / `with` / `{:?}` | `set` / `update` |
//! |---|---|---|
//! | `state.with(..)` | allowed | panics |
//! | `state.update(..)` | panics | panics |
//!
//! Another state may always be read or written inside the closure. The
//! panic says that the state is already being read or updated and is
//! reported at the line of the call in your code. Read first with `get`,
//! then write: `state.set(state.get() + 1)` is fine, and so is
//! `state.update(|value| *value += 1)`.
//!
//! In particular a change of one field that also reads the state does not
//! work inside `update`: `draft.update(|d| *d = Draft { title, ..draft.get() })`
//! panics when it runs. Write `draft.set(Draft { title, ..draft.get() })`
//! (the read ends before the write starts) or `draft.update(|d| d.title = title)`.
//!
//! # Writing while rendering
//!
//! Do not `set` or `update` a state in the body of a component that reads
//! the same state (with `get`, `with` or `{state}`): the write marks that
//! component to render again, the render writes again, and the rendering
//! never settles. Write from event handlers, effects and other code that
//! does not run on every render.

use std::fmt;

use dioxus::prelude::{
    use_context as backend_use_context, use_context_provider, use_signal, ReadableExt, Signal,
    WritableExt,
};
use dioxus::signals::{BorrowError, BorrowMutError};

/// Panics because the state is already borrowed by an enclosing `with` or
/// `update`. `#[track_caller]` makes the panic point at the user's call.
#[track_caller]
fn already_in_use() -> ! {
    panic!(
        "this `State` is already being read or updated by an enclosing `with` or `update` \
         closure; do not read or write the same state inside its own `with` or `update` \
         (read it with `get` first, then write)"
    )
}

/// Panics because the component that owns the state has been removed.
#[track_caller]
fn owner_gone() -> ! {
    panic!("this `State` was used after the component that created it was removed")
}

#[track_caller]
fn read_failed(error: &BorrowError) -> ! {
    match error {
        BorrowError::Dropped(_) => owner_gone(),
        _ => already_in_use(),
    }
}

#[track_caller]
fn write_failed(error: &BorrowMutError) -> ! {
    match error {
        BorrowMutError::Dropped(_) => owner_gone(),
        _ => already_in_use(),
    }
}

/// Where the value lives. A type of its own, not an alias, so that the
/// backend's type name does not appear in the signature an editor shows for
/// [`State`] (rust-analyzer prints the field of a tuple struct and expands an
/// alias).
struct Slot<T: 'static>(Signal<T>);

impl<T: 'static> Clone for Slot<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> Copy for Slot<T> {}

/// A value owned by a component that re-renders the subscribers when it
/// changes. See the [module documentation](self).
pub struct State<T: 'static>(Slot<T>);

impl<T: 'static> State<T> {
    /// A clone of the current value. Subscribes the running component.
    #[track_caller]
    pub fn get(&self) -> T
    where
        T: Clone,
    {
        self.with(T::clone)
    }

    /// Runs `read` with a reference to the current value and returns its
    /// result, without cloning the value. Subscribes the running component.
    ///
    /// The reference cannot leave the closure. Do not write to this same
    /// state inside it: that panics.
    #[track_caller]
    pub fn with<R>(&self, read: impl FnOnce(&T) -> R) -> R {
        match self.0 .0.try_read() {
            Ok(guard) => read(&guard),
            Err(error) => read_failed(&error),
        }
    }

    /// Replaces the value and notifies the subscribers.
    #[track_caller]
    pub fn set(&self, value: T) {
        self.update(|current| *current = value);
    }

    /// Changes the value in place with `change` and notifies the
    /// subscribers once the closure has returned.
    ///
    /// Do not read or write this same state inside the closure: that
    /// panics.
    #[track_caller]
    pub fn update(&self, change: impl FnOnce(&mut T)) {
        let mut signal = self.0 .0;
        let result = signal.try_write();
        match result {
            Ok(mut guard) => change(&mut guard),
            Err(error) => write_failed(&error),
        }
    }
}

impl<T: 'static> Clone for State<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> Copy for State<T> {}

/// Two handles are equal when they are the same state, not when their values
/// are equal. This is what lets a `State` be a component prop: a child is
/// rendered again only when it is given a different state or its other props
/// changed.
impl<T: 'static> PartialEq for State<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 .0 == other.0 .0
    }
}

impl<T: 'static> Eq for State<T> {}

/// Prints the current value without subscribing the running component.
impl<T: fmt::Debug + 'static> fmt::Debug for State<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 .0.try_peek() {
            Ok(value) => formatter.debug_tuple("State").field(&*value).finish(),
            Err(error) => read_failed(&error),
        }
    }
}

/// Creates a state, once per component instance: `init` runs on the first
/// render only, later renders return the same state.
///
/// A hook: call it in the same order on every render.
#[track_caller]
#[must_use]
pub fn use_state<T: 'static>(init: impl FnOnce() -> T) -> State<T> {
    State(Slot(use_signal(init)))
}

/// Makes `value()` available to every descendant of this component through
/// [`use_context`], and returns it. `value` runs on the first render only.
///
/// Context is looked up by type: provide one value per type, and wrap a
/// type that would otherwise be ambiguous (two `State<i32>`) in a struct.
/// A hook: call it in the same order on every render.
#[track_caller]
pub fn provide_context<T: Clone + 'static>(value: impl FnOnce() -> T) -> T {
    use_context_provider(value)
}

/// The value of type `T` provided by the nearest ancestor with
/// [`provide_context`]. Panics if no ancestor provided one. A hook: call it
/// in the same order on every render.
#[track_caller]
#[must_use]
pub fn use_context<T: Clone + 'static>() -> T {
    backend_use_context::<T>()
}
