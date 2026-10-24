use outou::prelude::*;

// Accepted before issue #26 (any `Display` type); a JSX `key` is now a
// string or an integer (ADR 0014).
#[component]
fn App() -> Element {
    <ul>
        <li key={'a'}>char</li>
        <li key={1.5f64}>float</li>
        <li key={true}>flag</li>
    </ul>
}
