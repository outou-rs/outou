use outou::prelude::*;

mod components;

use components::UserCard;

#[component]
fn Greeting(name: String) -> Element {
    <h1>Hello {name}</h1>
}

#[component]
fn App() -> Element {
    let user = load_user();

    <main class="app">
        <Greeting name="Outou" />

        {
            if user.is_some() {
                <UserCard user={user.unwrap()} />
            } else {
                <p>No user</p>
            }
        }
    </main>
}

fn load_user() -> Option<components::User> {
    Some(components::User { name: "Outou".to_string() })
}

fn main() {
    let _ = App;
}
