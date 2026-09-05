use outou::prelude::*;

#[component]
fn Before() -> Element {
    <div>before</div>
}

#[component]
fn App(items: Vec<i32>) -> Element {
    <ul>
        {items.iter().map(|x| <li>{x.clone()}</li>)
    </ul>
}

#[component]
fn After() -> Element {
    <p>after</p>
}
