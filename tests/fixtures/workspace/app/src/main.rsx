use outou::prelude::*;

use ui_kit::{Button, Card};

#[component]
fn App() -> Element {
    <div class="app">
        <Card title="Hello" />
        <Button label="Click" />
    </div>
}

fn main() {
    let _ = App;
}
