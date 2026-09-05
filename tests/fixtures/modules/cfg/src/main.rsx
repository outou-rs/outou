use outou::prelude::*;

#[cfg(feature = "x")]
mod optional;

#[ cfg(feature = "y") ]
mod spaced;

#[component]
fn App() -> Element {
    <div>{"cfg fixture"}</div>
}
