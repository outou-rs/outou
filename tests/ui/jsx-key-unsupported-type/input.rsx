use outou::prelude::*;

struct Money(u32);

#[component]
fn App() -> Element {
    let price = Money(3);
    <ul>
        <li key={price}>price</li>
        <li key={true}>flag</li>
    </ul>
}
