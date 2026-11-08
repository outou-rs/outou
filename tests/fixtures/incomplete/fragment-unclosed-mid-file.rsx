use outou::prelude::*;

#[component]
fn Before() -> Element {
    <>
        <p>before</p>
    </>
}

#[component]
fn App() -> Element {
    let user = load_user();

    <>
        <p>{user}</p>
}

#[component]
fn After() -> Element {
    <p>after</p>
}
