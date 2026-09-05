//! Spike entry point. Everything that `App.rsx` refers to is defined here as
//! plain Rust, so the spike exercises cross-file definition/hover as well.

use dioxus::prelude::*;

/// A user record. `App.rsx` calls `load_user()` and hovers over the result.
#[derive(Debug, Clone, PartialEq)]
pub struct User {
    pub name: String,
    pub age: u32,
}

/// Definition target for "go to definition" from `App.rsx`.
pub fn load_user() -> User {
    User {
        name: "Outou".to_string(),
        age: 1,
    }
}

/// Component target for completion of `<UserCard` and its props.
#[component]
pub fn UserCard(user: User) -> Element {
    rsx! {
        div { class: "user-card", "{user.name} ({user.age})" }
    }
}

// Generated Rust at a fixed path, an ordinary source file for rust-analyzer
// (ADR 0009).
#[path = ".generated/App.rs"]
mod app;

fn main() {
    let _ = app::App;
    println!("ra-spike-fixture: this binary only exists to be analyzed");
}
