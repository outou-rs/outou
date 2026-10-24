use outou::prelude::*;

#[component]
fn Row(label: String) -> Element {
    <li>{label}</li>
}

#[component]
fn App() -> Element {
    let id = 7u32;
    let name = String::from("edit");
    let labels = vec![String::from("a"), String::from("b")];
    <ul key="root">
        <Row key="new" label={name.clone()} />
        <Row key={format!("edit-{id}")} label={name.clone()} />
        <Row key={id} label={name.clone()} />
        <Row key={String::from("x")} label={name} />
        {labels.iter().map(|label| <Row key={label.as_str()} label={label.clone()} />)}
        {[1u8, 2].iter().map(|n| <li key={*n}>{n.to_string()}</li>)}
    </ul>
}
