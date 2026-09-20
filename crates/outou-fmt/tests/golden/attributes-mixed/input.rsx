use outou::prelude::*;

#[component]
fn Field(label: String, value: String) -> Element {
        <input   class="field"    value={value.clone()}   disabled  />
}
