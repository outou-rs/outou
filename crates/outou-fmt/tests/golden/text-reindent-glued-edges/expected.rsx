use outou::prelude::*;

#[component]
fn App() -> Element {
    <div>
        <p>
            glued first
            glued last
        </p>
        <p>
            <b>x</b>after b
            more<i>y</i>after i
            end
        </p>
        <p>
            <b>x</b> first space kept
            last space kept </p>
    </div>
}
