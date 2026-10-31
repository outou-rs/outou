use outou::prelude::*;

struct Price(u32);

#[component]
fn App() -> Element {
    let price = Price(3);
    let open = true;
    <ul>
        <li>{price}</li>
        <li>{open}</li>
        <li>{open && 1 > 0}</li>
        <li>{Some(open)}</li>
    </ul>
}
