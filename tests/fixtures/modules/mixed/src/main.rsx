use outou::prelude::*;

mod components;

use components::{button::Button, user::UserCard};

#[component]
fn App() -> Element {
    <main>
        <UserCard name="Outou" />
        <Button label="Go" />
    </main>
}
