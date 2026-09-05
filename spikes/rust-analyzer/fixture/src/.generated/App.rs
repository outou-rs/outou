// Hand-written "generated" Rust for `../App.rsx` (ADR 0009: fixed path
// `src/.generated/` + `#[path]`). This is the only generated-source layout;
// see docs/adr/0009-generated-source-location.md.

use super::*;

#[component]
pub fn App() -> Element {
    let user = load_user();

    rsx! {
        UserCard {
            user: user
        }
    }
}
