use outou::prelude::*;

#[component]
pub fn Button(label: String) -> Element {
    ::outou::__private::rsx! { button { {label} } }
}
