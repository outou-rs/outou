# outou

Public facade crate. Applications depend on this crate only and write `use outou::prelude::*;`.
The hidden `__private` module re-exports the execution backend for generated code; it is not public API.

Phase 0: Week 4 (Gate 2).

## `__private`

- `component`, `Element`, and the backend macro-hygiene names (`dioxus_core`, `dioxus_elements`, `dioxus_signals`, `Props`) documented as leakage in `docs/backend-leakage.md` (row 11; row 16 records why generated code cannot avoid needing them in scope).
- `recovery_element() -> Element` and `recovery<T>() -> T`: recovery-mode placeholders emitted by `outou-backend-dioxus` for a construct it could not reconstruct. Both `unreachable!()` if ever actually called — recovery-mode generated Rust is for rust-analyzer analysis only and is never fed to `rustc` through `cargo build`. See `crates/outou-backend-dioxus/README.md`'s "Recovery-mode placeholders" section for exactly when each is used.

## Public API besides the prelude

- `outou::jsx::IntoKey`: the sealed trait behind a JSX `key` (strings and integers; narrower than before #26, when any `Display` type was accepted: `char`, `bool`, floating point numbers and custom `Display` types are no longer keys). A key of any other type is a compile error in Outou's words, at the key in the `.rsx` file (issue #26, ADR 0014). `__private::key` is the generated-code entry point that binds each key.
- `outou::jsx::IntoChild<M>`: the sealed trait behind a JSX child expression (`<p>{expr}</p>`), issue #27 and ADR 0014. Text, elements, `Option`s of text or of an element and iterators of elements are children as before; integers, floating point numbers and `char`, and references to them (`&usize`), now render as their `Display` text (`{count}` is `2`, without `.to_string()`). **`bool` is not a child** (use `if` or `Option`), nor is any other type, among them an `Option` of a number (`.map(|n| n.to_string())`), a `Vec<Element>` (`.into_iter()`), `&String` and `Arc<str>`/`Rc<str>`/`Cow<str>`/`Box<str>` (`TODO(alpha)`). A `State<T>` is a child when `T` is (`{count}` with `count: State<usize>`, issue #29), and subscribes the component that renders it: the error is Outou's, at the child in the `.rsx` file (``` `bool` cannot be rendered as a JSX child ```). `M` is a marker type that keeps the implementations apart; it is always inferred. `__private::child` is the generated-code entry point that wraps every child.
- `outou::jsx::IntoOptionValue`: the sealed trait behind the value of a `<select value={V}>` and of the `<option value={..}>` elements inside it (issue #28, ADR 0014): a string, an integer or a `char` (and references to them), turned into text and compared as exact text, so that the option whose value equals the select's is selected on mount, as in React. Anything else (`bool`, a float, an `Option`, a custom type) is a compile error in Outou's words, at the expression in the `.rsx` file (``` `Priority` cannot be used as the value of a `<select>` or `<option>` ```). `__private::select_value`, `option_value` and `option_selected` are the generated-code entry points. Limits (`<select multiple>`, options reached through variables or components): `docs/grammar.md` §5.1.

## Opt-in features

- `web`: the only supported public feature. It exposes `outou::prelude::launch` and selects the browser renderer. Used by `examples/dogfood-board`; off by default (ADR 0010 amendment, `docs/backend-leakage.md` row 32).
- `launch` is an internal building block of `web` and is not supported on its own: it makes `outou::prelude::launch` exist but selects no renderer, so calling it panics at run time. Applications enable `web`, never `launch`.

## State API (issue #29, ADR 0014)

The prelude's state API is Outou's own, a thin wrapper over the backend's signals that never exposes a guard:

- `State<T>`: a handle to a value owned by a component. `Copy`, `Clone`, `PartialEq` (by identity) and `Debug` (when `T: Debug`), so it can be moved into event handlers and passed as a component prop. `get() -> T` (`T: Clone`), `with(|v| ..) -> R`, `set(v)` and `update(|v: &mut T| ..)` all take `&self`; no `let mut` is needed. There is no `read()` or `write()`, so `state.set(T { f, ..state.read().clone() })` cannot be written; write `state.update(|t| t.f = f)` or `state.set(T { f, ..state.get() })`.
- `use_state(|| init)`, `provide_context(|| value)` and `use_context::<T>()`: hooks, called in the same order on every render.
- `launch(App)` (with the `web` feature): starts the application in the browser.

**Subscription.** `get` and `with` subscribe the code that is running: called while a component renders, they make it render again after the next `set` or `update` of that state; called in an event handler they only read. `set` and `update` notify the subscribers even when the value is unchanged. This is the backend's semantics, not an Outou-defined one. **The same state twice.** The value is borrowed for the whole of a `with` or `update` call, so inside the closure of `state.with(..)` reading the same state (`get`, `with`) is fine and writing it (`set`, `update`) panics at run time, and inside the closure of `state.update(..)` both reading (`get`, `with`, `{:?}`) and writing the same state panic. Another state is always fine. The panic is in Outou's words and is reported at the line of your call. Read first, then write: `draft.set(Draft { title, ..draft.get() })` and `draft.update(|d| d.title = title)` work, `draft.update(|d| *d = Draft { title, ..draft.get() })` panics when it runs. **Writing while rendering.** Do not `set` or `update` a state in the body of a component that reads that same state: the write marks the component dirty, it renders again, writes again, and never settles; write from event handlers. The full list of the prelude's names is pinned by `crates/outou-cli/tests/prelude_names.rs`.

This crate is the only one in the workspace allowed to depend on `dioxus` (`AGENTS.md`).
