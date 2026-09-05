use outou::prelude::*;

mod components;

use components::{Field, UserCard};

#[component]
fn Greeting(name: String) -> Element {
    <h1>Hello {name}</h1>
}

#[component]
fn TagList(tags: Vec<String>) -> Element {
    <ul class="tags">
        {tags.iter().map(|tag| <li key={tag.clone()}>{tag.clone()}</li>)}
    </ul>
}

#[component]
fn App() -> Element {
    let user = load_user();
    let tags = vec!["rust".to_string(), "jsx".to_string(), "phase0".to_string()];

    <main class="app">
        <Greeting name="Outou" />

        {
            if user.is_some() {
                <UserCard user={user.unwrap()} />
            } else {
                <p>No user</p>
            }
        }

        <TagList tags={tags} />

        <form>
            <Field label="Name" type="text" />
        </form>
    </main>
}

fn load_user() -> Option<components::User> {
    Some(components::User { name: "Outou".to_string() })
}

fn main() {
    let _ = App;
    let _ = components::initial(&components::User {
        name: "Outou".to_string(),
    });
}
