use outou::prelude::*;

#[component]
fn Multiline(show: bool) -> Element {
    <div>
        {
            if show {
                <span title="aa
  bb" />
            } else {
                <span title="x" />
            }
        }
    </div>
}
