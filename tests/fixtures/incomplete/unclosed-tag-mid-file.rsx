use outou::prelude::*;

#[component]
fn Before() -> Element {
    <div>before</div>
}

#[component]
fn App() -> Element {
    let user = load_user();

    <div cl
}

#[component]
fn After() -> Element {
    <p>after</p>
}
