use outou::prelude::*;

fn load_user() -> String {
    String::new()
}

#[component]
fn App() -> Element {
    let user: u32 = load_user();
    let label = format!("{user}");
    <div>{label}</div>
}
