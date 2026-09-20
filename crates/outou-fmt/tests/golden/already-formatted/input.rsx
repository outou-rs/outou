use outou::prelude::*;

#[component]
fn Card(title: String) -> Element {
    <div class="card">{title}</div>
}
