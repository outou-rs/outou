use outou::prelude::*;

#[component]
pub fn UserCard(name: String) -> Element {
    <div class="user">{name}</div>
}
