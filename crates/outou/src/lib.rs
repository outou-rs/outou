//! Outou — Rust with JSX.
//!
//! This is the only crate an Outou application depends on. Bring the public
//! API into scope with:
//!
//! ```
//! use outou::prelude::*;
//! ```
//!
//! Generated code reaches the execution backend through the hidden
//! [`__private`] module. Application code must not use it; its contents are
//! not part of the public API and change without notice.

#![doc(html_root_url = "https://docs.rs/outou/0.0.1")]

/// Everything an Outou application needs in scope.
pub mod prelude {
    pub use crate::__private::{component, Element};

    // The backend's macros expand to unhygienic paths that must resolve in
    // the *user's* scope. These names are therefore re-exported here, hidden
    // from documentation. They are a backend leakage, tracked in
    // `docs/backend-leakage.md` ("Macro expansion names in scope").
    #[doc(hidden)]
    pub use crate::__private::{dioxus_core, dioxus_elements, dioxus_signals, Props};

    // Alpha gate A4 (issue #22): the temporary backend's existing state and
    // launch APIs, re-exported unchanged so an application can be
    // interactive. Not Outou-owned hooks or signals: the alpha decides the
    // runtime, it does not build one. Each name is a backend leakage
    // recorded in ADR 0010 and `docs/backend-leakage.md` rows 31-32.
    pub use crate::__private::{
        use_context, use_context_provider, use_signal, ReadableExt, Signal, WritableExt,
    };

    /// Starts the application in the browser with `app` as the root
    /// component. Needs the `web` feature of `outou`.
    #[cfg(feature = "launch")]
    pub use crate::__private::launch;
}

pub mod jsx {
    //! Types that JSX syntax gives a meaning to: the `key` of a JSX element
    //! (`<li key={id}>`, `<Row key="new" />`), the value of a child
    //! expression (`<p>{count}</p>`) and the `value` of a `<select>` and its
    //! `<option>` elements.

    use dioxus::dioxus_core::{DynamicNode, IntoDynNode};

    mod sealed {
        pub trait Sealed {}

        /// Seals [`super::IntoOptionValue`].
        pub trait SealedOptionValue {}

        /// Seals [`super::IntoChild`]: the marker parameter is as free as
        /// the trait's own, so sealing needs the same parameter.
        pub trait SealedChild<M> {}
    }

    /// Marker for the children the backend already knows how to render.
    /// `A` is the backend's own marker; it is carried only so that the
    /// delegating impl does not overlap the number impls below.
    #[doc(hidden)]
    pub struct Delegated<A>(std::marker::PhantomData<A>);

    /// Marker for the children Outou renders as text: numbers and `char`.
    #[doc(hidden)]
    pub struct Displayed;

    /// A value that can be a JSX child: `<p>{value}</p>`.
    ///
    /// Implemented for text (`&str`, `String`, formatted strings), for
    /// integers, floating point numbers and `char`, and for references to
    /// them (`&usize`, what `v.iter()` hands out), all rendered as their
    /// `Display` text (so `{3.5}` is the text `3.5`), for an `Element`, for
    /// an `Option` of text or of an `Element` (`None` renders nothing) and
    /// for an iterator of elements.
    ///
    /// Not children, with the way to write them:
    ///
    /// - an `Option` of a number or `char`: `{maybe.map(|n| n.to_string())}`;
    /// - a `Vec<Element>`: `{items.into_iter()}` (or `.iter()`), because a
    ///   `Vec` is not an iterator;
    /// - an iterator of text: map each item to an element first;
    /// - `&String`, `&&str`, `Arc<str>`, `Rc<str>`, `Cow<str>`, `Box<str>`:
    ///   `{s.as_str()}` or `{s.to_string()}`;
    /// - a `Signal<usize>`: read it first (the state API is issue #29).
    ///
    /// TODO(alpha): the `&String`/`&&str`/smart-pointer strings, the `Option`
    /// of a number and a `Signal` of a displayable value are gaps to close
    /// or to keep deliberately, see `docs/grammar.md` (Child types).
    ///
    /// **`bool` is not a child.** Rust has no `cond && <x />` that keeps its
    /// type, so a condition is written with `if` or with `Option`:
    ///
    /// ```text
    /// {if ready { <b>done</b> } else { <i>wait</i> }}
    /// {ready.then(|| <b>done</b>)}
    /// ```
    ///
    /// The trait is sealed; it cannot be implemented outside Outou. A child
    /// of any other type is reported at the child expression in the `.rsx`
    /// file:
    ///
    /// ```text
    /// error[E0277]: `bool` cannot be rendered as a JSX child
    /// ```
    ///
    /// The type parameter `M` only keeps the implementations apart; it is
    /// always inferred and never written.
    #[diagnostic::on_unimplemented(
        message = "`{Self}` cannot be rendered as a JSX child",
        label = "this is not text, a number, an element, an `Option` of text or an element, or an iterator of elements",
        note = "a JSX child can be text (`&str`, `String`), a number, a `char`, an element, an `Option` of text or of an element, or an iterator of elements",
        note = "`bool` is not a child; write a conditional child with `if` or `Option`, for example `{{if ready {{ <b>done</b> }} else {{ <i>wait</i> }}}}` or `{{ready.then(|| <b>done</b>)}}`",
        note = "convert any other value to text first, for example with `.to_string()`; an `Option` of a number needs `.map(|n| n.to_string())`"
    )]
    pub trait IntoChild<M>: sealed::SealedChild<M> {
        /// The child as a backend node. Generated code only; not public API.
        #[doc(hidden)]
        fn into_child(self) -> DynamicNode;
    }

    impl<T: IntoDynNode<A>, A> sealed::SealedChild<Delegated<A>> for T {}
    impl<T: IntoDynNode<A>, A> IntoChild<Delegated<A>> for T {
        fn into_child(self) -> DynamicNode {
            self.into_dyn_node()
        }
    }

    macro_rules! displayed_children {
        ($($displayed:ty),* $(,)?) => {
            $(
                impl sealed::SealedChild<Displayed> for $displayed {}
                impl IntoChild<Displayed> for $displayed {
                    fn into_child(self) -> DynamicNode {
                        self.to_string().into_dyn_node()
                    }
                }
                // `v.iter().map(|n| <li>{n}</li>)` hands out `&usize`.
                impl sealed::SealedChild<Displayed> for &$displayed {}
                impl IntoChild<Displayed> for &$displayed {
                    fn into_child(self) -> DynamicNode {
                        self.to_string().into_dyn_node()
                    }
                }
            )*
        };
    }

    displayed_children!(
        u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, f32, f64, char,
    );

    /// A value that can be used as a JSX `key`: a string or an integer.
    ///
    /// Implemented for `str`, `String`, `Cow<'_, str>`, `Box<str>`,
    /// `Rc<str>`, `Arc<str>`, every integer type, every `NonZero*` type, and
    /// `&T` / `&mut T` of any of them. This is narrower than before issue
    /// #26, when any `Display` type was accepted: `char`, `bool`, floating
    /// point numbers and a custom `Display` type are no longer keys.
    ///
    /// The trait is sealed; it cannot be implemented outside Outou. A key
    /// expression is checked against it, so a value of any other type is
    /// reported at the `key` in the `.rsx` file:
    ///
    /// ```text
    /// error[E0277]: `Money` cannot be used as a JSX `key`
    /// ```
    #[diagnostic::on_unimplemented(
        message = "`{Self}` cannot be used as a JSX `key`",
        label = "this is not a string or an integer",
        note = "a JSX `key` must be a string (`&str`, `String`) or an integer type",
        note = "convert the value first, for example with `.to_string()`"
    )]
    pub trait IntoKey: sealed::Sealed {
        /// The key as a string. Generated code only; not public API.
        #[doc(hidden)]
        fn to_key(&self) -> String;
    }

    impl sealed::Sealed for str {}
    impl IntoKey for str {
        fn to_key(&self) -> String {
            self.to_owned()
        }
    }

    impl sealed::Sealed for String {}
    impl IntoKey for String {
        fn to_key(&self) -> String {
            self.clone()
        }
    }

    impl<T: IntoKey + ?Sized> sealed::Sealed for &T {}
    impl<T: IntoKey + ?Sized> IntoKey for &T {
        fn to_key(&self) -> String {
            (**self).to_key()
        }
    }

    impl<T: IntoKey + ?Sized> sealed::Sealed for &mut T {}
    impl<T: IntoKey + ?Sized> IntoKey for &mut T {
        fn to_key(&self) -> String {
            (**self).to_key()
        }
    }

    macro_rules! string_pointer_keys {
        ($($pointer:ty),* $(,)?) => {
            $(
                impl sealed::Sealed for $pointer {}
                impl IntoKey for $pointer {
                    fn to_key(&self) -> String {
                        str::to_key(self)
                    }
                }
            )*
        };
    }

    string_pointer_keys!(
        std::borrow::Cow<'_, str>,
        Box<str>,
        std::rc::Rc<str>,
        std::sync::Arc<str>,
    );

    macro_rules! integer_keys {
        ($($integer:ty),* $(,)?) => {
            $(
                impl sealed::Sealed for $integer {}
                impl IntoKey for $integer {
                    fn to_key(&self) -> String {
                        self.to_string()
                    }
                }
            )*
        };
    }

    integer_keys!(u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize);

    macro_rules! non_zero_keys {
        ($($non_zero:ty),* $(,)?) => {
            $(
                impl sealed::Sealed for $non_zero {}
                impl IntoKey for $non_zero {
                    fn to_key(&self) -> String {
                        self.get().to_string()
                    }
                }
            )*
        };
    }

    non_zero_keys!(
        std::num::NonZeroU8,
        std::num::NonZeroU16,
        std::num::NonZeroU32,
        std::num::NonZeroU64,
        std::num::NonZeroU128,
        std::num::NonZeroUsize,
        std::num::NonZeroI8,
        std::num::NonZeroI16,
        std::num::NonZeroI32,
        std::num::NonZeroI64,
        std::num::NonZeroI128,
        std::num::NonZeroIsize,
    );

    /// A value that can be the `value` of a `<select>` or of an `<option>`:
    /// a string, an integer or a `char`.
    ///
    /// `<select value={V}>` selects the `<option>` whose `value` is equal to
    /// `V`. Both are turned into text first and compared as text, as the
    /// browser does: `2` and `"2"` are equal, `"a"` and `"A"` are not.
    ///
    /// Implemented for `str`, `String`, `Cow<'_, str>`, `Box<str>`,
    /// `Rc<str>`, `Arc<str>`, every integer type, every `NonZero*` type,
    /// `char`, and `&T` / `&mut T` of any of them. `bool`, floating point
    /// numbers, `Option` and a custom type are not values: convert them with
    /// `.to_string()` (or `.label()` and the like) first.
    ///
    /// The trait is sealed; it cannot be implemented outside Outou. A value
    /// of any other type is reported at the expression in the `.rsx` file:
    ///
    /// ```text
    /// error[E0277]: `Priority` cannot be used as the value of a `<select>` or `<option>`
    /// ```
    #[diagnostic::on_unimplemented(
        message = "`{Self}` cannot be used as the value of a `<select>` or `<option>`",
        label = "this is not a string, an integer or a `char`",
        note = "the `value` of a `<select>` and of an `<option>` must be a string (`&str`, `String`), an integer type or a `char`",
        note = "convert the value first, for example with `.to_string()`"
    )]
    pub trait IntoOptionValue: sealed::SealedOptionValue {
        /// The value as text. Generated code only; not public API.
        #[doc(hidden)]
        fn to_option_value(&self) -> String;
    }

    impl sealed::SealedOptionValue for str {}
    impl IntoOptionValue for str {
        fn to_option_value(&self) -> String {
            self.to_owned()
        }
    }

    impl sealed::SealedOptionValue for String {}
    impl IntoOptionValue for String {
        fn to_option_value(&self) -> String {
            self.clone()
        }
    }

    impl<T: IntoOptionValue + ?Sized> sealed::SealedOptionValue for &T {}
    impl<T: IntoOptionValue + ?Sized> IntoOptionValue for &T {
        fn to_option_value(&self) -> String {
            (**self).to_option_value()
        }
    }

    impl<T: IntoOptionValue + ?Sized> sealed::SealedOptionValue for &mut T {}
    impl<T: IntoOptionValue + ?Sized> IntoOptionValue for &mut T {
        fn to_option_value(&self) -> String {
            (**self).to_option_value()
        }
    }

    macro_rules! displayed_option_values {
        ($($displayed:ty),* $(,)?) => {
            $(
                impl sealed::SealedOptionValue for $displayed {}
                impl IntoOptionValue for $displayed {
                    fn to_option_value(&self) -> String {
                        self.to_string()
                    }
                }
            )*
        };
    }

    displayed_option_values!(
        u8,
        u16,
        u32,
        u64,
        u128,
        usize,
        i8,
        i16,
        i32,
        i64,
        i128,
        isize,
        char,
        std::borrow::Cow<'_, str>,
        Box<str>,
        std::rc::Rc<str>,
        std::sync::Arc<str>,
        std::num::NonZeroU8,
        std::num::NonZeroU16,
        std::num::NonZeroU32,
        std::num::NonZeroU64,
        std::num::NonZeroU128,
        std::num::NonZeroUsize,
        std::num::NonZeroI8,
        std::num::NonZeroI16,
        std::num::NonZeroI32,
        std::num::NonZeroI64,
        std::num::NonZeroI128,
        std::num::NonZeroIsize,
    );
}

/// Backend re-exports used by generated code. **Not public API.**
#[doc(hidden)]
pub mod __private {
    pub use dioxus::dioxus_core;
    pub use dioxus::prelude::{
        component, dioxus_elements, dioxus_signals, rsx, use_context, use_context_provider,
        use_signal, Element, Props, ReadableExt, Signal, WritableExt,
    };

    #[cfg(feature = "launch")]
    pub use dioxus::launch;

    /// Turns the value of a JSX `key` into the string the backend takes.
    /// Generated code binds each key with this before the element's own
    /// call, so a key of an unsupported type is reported here, in Outou's
    /// words (`crate::jsx::IntoKey`). The value is converted on the spot, so
    /// the original is not borrowed by the element that follows.
    #[doc(hidden)]
    pub fn key<K: crate::jsx::IntoKey + ?Sized>(key: &K) -> String {
        key.to_key()
    }

    /// Turns the `value` of a `<select value={V}>` into text. Generated code
    /// binds it once, before the element's call, and every `<option>` of the
    /// select compares against that one value (`option_selected`). A value of
    /// an unsupported type is reported here, in Outou's words
    /// (`crate::jsx::IntoOptionValue`).
    #[doc(hidden)]
    pub fn select_value<V: crate::jsx::IntoOptionValue + ?Sized>(value: &V) -> String {
        value.to_option_value()
    }

    /// Turns the `value` of an `<option>` that sits inside a
    /// `<select value={..}>` into text. Generated code binds it once and uses
    /// the binding both as the option's `value` and in `option_selected`.
    #[doc(hidden)]
    pub fn option_value<V: crate::jsx::IntoOptionValue + ?Sized>(value: &V) -> String {
        value.to_option_value()
    }

    /// Whether an `<option>` is the one the enclosing `<select value>`
    /// names: exact text equality, as the browser compares them.
    #[doc(hidden)]
    pub fn option_selected(select_value: &str, option_value: &str) -> bool {
        select_value == option_value
    }

    /// Turns a JSX child expression into the node the backend renders.
    /// Generated code wraps every `{ … }` child in this call, so a child of
    /// an unsupported type (a `bool`, a custom struct) is reported here, in
    /// Outou's words (`crate::jsx::IntoChild`), and a number is rendered as
    /// text.
    #[doc(hidden)]
    pub fn child<M>(child: impl crate::jsx::IntoChild<M>) -> dioxus::dioxus_core::DynamicNode {
        child.into_child()
    }

    /// Recovery-mode placeholder for a JSX element the parser could not
    /// recover a name for (grammar §9 — `outou-backend-dioxus`'s
    /// `is_broken`). Only ever emitted in `outou-codegen`'s recovery
    /// mode, which is for rust-analyzer only and never reaches `rustc`
    /// through `cargo build` (strict-mode generation is refused whenever
    /// a broken element could occur). Calling it is a bug: it exists
    /// purely so the surrounding, unrelated code stays type-checkable
    /// while the user is still typing.
    #[doc(hidden)]
    pub fn recovery_element() -> Element {
        recovery()
    }

    /// Recovery-mode placeholder for a broken Rust expression (a
    /// statement, tail, or island `outou-backend-dioxus` could not
    /// reconstruct). Its type is inferred from context, so it fits
    /// wherever the broken expression stood — see the same never-executed
    /// contract as [`recovery_element`].
    #[doc(hidden)]
    pub fn recovery<T>() -> T {
        unreachable!(
            "Outou recovery placeholder: recovery-mode generated code is for rust-analyzer \
             analysis only and must never run"
        )
    }
}
