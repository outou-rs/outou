use outou::prelude::*;

#[component]
fn App() -> Element {
    <select value="a">
        {["b"].into_iter().map(|__outou_select_9| <option value="a">A</option>)}
    </select>
}
