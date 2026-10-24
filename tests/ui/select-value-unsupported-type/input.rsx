use outou::prelude::*;

struct Priority(u8);

#[component]
fn App() -> Element {
    let current = Priority(1);
    <select value={current}>
        <option value={true}>flag</option>
        <option value={2.5}>half</option>
    </select>
}
