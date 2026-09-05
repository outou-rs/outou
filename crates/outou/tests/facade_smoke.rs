//! Compile-time check that generated code can reach the backend through
//! `::outou::__private` while the user crate only imports `outou::prelude`.
//! Nothing here mentions the backend crate by name.

use outou::prelude::*;

#[component]
fn Greeting(name: String) -> Element {
    ::outou::__private::rsx! {
        h1 { "Hello " {name} }
    }
}

#[component]
fn App() -> Element {
    ::outou::__private::rsx! {
        main { class: "app", Greeting { name: "Outou" } }
    }
}

#[test]
fn facade_exposes_backend_only_through_private_path() {
    let _ = App;
    let _ = Greeting;
}
