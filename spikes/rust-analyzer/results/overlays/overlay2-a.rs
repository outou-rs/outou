// Hand-written "generated" Rust for `fixture/src/App.rsx`.
// The real compiler would emit `::outou::__private::rsx!`; the spike has no
// framework code, so it uses the backend's macro directly.

#[component]
pub fn App() -> Element {
    let user = load_user().name;

    rsx! {
        UserCard {
            user: user
        }
    }
}
