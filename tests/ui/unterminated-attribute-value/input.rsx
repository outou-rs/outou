use outou::prelude::*;

#[component]
fn App() -> Element {
    let user = load_user();

    <User name={
