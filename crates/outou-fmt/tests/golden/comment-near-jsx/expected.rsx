use outou::prelude::*;

// A plain Rust comment right before a component.
#[component]
fn Noted() -> Element {
    <div>
        {/* keep this note */}
        <span>{1}</span>
    </div>
}
