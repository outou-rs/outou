use outou::prelude::*;

#[component]
fn Multiline() -> Element {
    <div>{r#"first
  second
    third"#}</div>
}
