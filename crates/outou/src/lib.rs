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
    //! (`<li key={id}>`, `<Row key="new" />`).

    mod sealed {
        pub trait Sealed {}
    }

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
