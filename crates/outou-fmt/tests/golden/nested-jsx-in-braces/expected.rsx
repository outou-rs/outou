use outou::prelude::*;

#[component]
fn Conditional(show: bool) -> Element {
    <div>
        {
            if show {
                <span>{1}</span>
            } else {
                <span>{2}</span>
            }
        }
    </div>
}
