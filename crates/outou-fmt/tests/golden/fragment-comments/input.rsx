use outou::prelude::*;

#[component]
fn Island() -> Element {
    <>{/* kept */}</>
}

#[component]
fn Tags() -> Element {
    <div>< /* in */ >x</ /* out */ ></div>
}

#[component]
fn Between() -> Element {
    <>
        {/* first */}
        <p/>
        plain text
    </>
}
