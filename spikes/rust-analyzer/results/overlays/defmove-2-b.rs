// Hand-written "generated" Rust for `../App.rsx` (variant (b): fixed path).
// Keep in sync with `../../../virtual/App.rs`; only the `use` line differs
// because this file is a module rather than an `include!`.

use super::*;

#[component]
pub fn App() -> Element {
    // shifted by didChange
    let user = load_user();
    let _probe = user.age;

    rsx! {
        UserCard {
            user: user
        }
    }
}
