use outou::prelude::*;

#[component]
fn App() -> Element {
    <p>
        lead
        word   <b>x</b>   keep leading;
        keep trailing   <i>y</i>
    </p>
}
