// Hand-written "generated" Rust for `fixture/src/App.rsx`.
// The real compiler would emit `::outou::__private::rsx!`; the spike has no
// framework code, so it uses the backend's macro directly.

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
