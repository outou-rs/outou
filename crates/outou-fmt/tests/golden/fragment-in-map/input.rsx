use outou::prelude::*;

#[component]
fn List(items: Vec<String>) -> Element {
    <dl>{items.iter().map(|item| <><dt key={item.clone()}>{item.clone()}</dt><dd>{item.len()}</dd></>)}</dl>
}

#[component]
fn Cond(on: bool) -> Element {
    <div>{if on { <><b>on</b><i>!</i></> } else { <></> }}</div>
}

#[component]
fn Long() -> Element {
    <>
        <section class="a-rather-long-class-name another-class-name" id="some-identifier">content one</section><section class="x">content two</section>
    </>
}
