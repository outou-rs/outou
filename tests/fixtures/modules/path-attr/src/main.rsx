use outou::prelude::*;

#[path = "elsewhere/thing.rsx"]
mod thing;

#[path = "somewhere/other.rs"]
mod other;

#[component]
fn App() -> Element {
    <div>{"path-attr fixture"}</div>
}
