use outou::prelude::*;

#[component]
fn UserCard(name: String) -> Element {
    <div>{name}</div>
}

#[component]
fn App() -> Element {
    <UserCard />
}
