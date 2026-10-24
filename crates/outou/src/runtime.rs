//! Starting an application.

use crate::__private::Element;

/// Starts the application in the browser with `app` as the root component.
///
/// Needs the `web` feature of `outou`; without it there is no renderer and
/// the call panics.
pub fn launch(app: fn() -> Element) {
    dioxus::launch(app);
}
