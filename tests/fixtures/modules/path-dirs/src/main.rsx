use outou::prelude::*;

mod inline_host {
    #[path = "inner.rsx"]
    mod inner;
}

#[path = "thread_files"]
mod thread {
    #[path = "tls.rsx"]
    mod local_data;
    mod extra;
}

#[path = "somewhere/other.rs"]
mod other;

#[path = "deep/loaded.rs"]
mod loaded;

mod plain;

mod mod_style;

#[component]
fn App() -> Element {
    <p>{"path-dirs fixture"}</p>
}
