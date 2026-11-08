use outou::prelude::*;

#[component]
fn Rows() -> Element {
    <><tr><td>a</td></tr><tr><td>b</td></tr></>
}

#[component]
fn Single() -> Element {
    <>   <p>only</p>   </>
}

#[component]
fn Empty() -> Element {
    <>   </>
}

#[component]
fn Nested() -> Element {
    <ul><><li>a</li><li>b</li></><li>c</li><><></></></ul>
}
