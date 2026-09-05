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
}

/// Backend re-exports used by generated code. **Not public API.**
#[doc(hidden)]
pub mod __private {
    pub use dioxus::dioxus_core;
    pub use dioxus::prelude::{component, dioxus_elements, dioxus_signals, rsx, Element, Props};
}
