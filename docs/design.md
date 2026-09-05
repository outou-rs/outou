# Design

## The bet

Outou introduces JSX into Rust as a first-class expression, in its own file type:

```rsx
use outou::prelude::*;

#[component]
fn App() -> Element {
    let user = load_user();

    <main>
        <UserCard user={user} />
    </main>
}
```

The element is not inside a macro. It is an expression of the language, in a `.rsx` file, next to ordinary Rust.

Dioxus, Yew and others already offer HTML-like DSLs inside macros. What they cannot offer, and what Outou is betting on, is this: **JSX outside a macro DSL, without losing Rust's developer experience.** Concretely, while editing a `.rsx` file you keep:

- go to definition on `load_user()`
- the type of `user` on hover
- completion after `user.`
- completion of component names and their props
- Rust type errors at the right position in the `.rsx` file
- JSX syntax errors phrased by Outou, not by a backend
- `cargo build`, and nothing else, to build

and all of this keeps working while the file is half-typed (`<UserCard us`, `<div class=`).

The novelty is therefore not a VDOM, hooks or a renderer. It is the front end and the tooling: grammar, parser with error recovery, module resolution, Cargo integration, source maps, rust-analyzer integration, diagnostics and formatting. That is what Phase 0 validates, and nothing else.

The one question Phase 0 answers: **can JSX become part of Rust without making Rust worse?** If yes, Outou continues. If no, the standalone `.rsx` format is abandoned.

## Non-goals of Phase 0

Phase 0 builds none of the following:

- a VDOM, hooks, signals or a renderer of its own
- React interop
- SSR, a router, HMR, a native renderer
- a production CSS solution
- React drop-in compatibility

Showing "Hello Outou" in a browser is explicitly *not* a success criterion.

## Why Dioxus is the temporary backend

Outou does not implement a runtime in Phase 0. It compiles Outou JSX to Dioxus `rsx!` invocations and lets Dioxus execute them:

```text
Outou JSX → Outou AST → Dioxus backend → rsx! → Rust
```

Dioxus is not part of Outou's public specification. The Outou AST is never turned into a Dioxus AST; the backend is one lowering among possible others, and every place Dioxus shows through is recorded in [backend-leakage.md](backend-leakage.md). After the front end is proven, a runtime decision is made with that ledger as input: keep Dioxus, or move to a native runtime.

## The facade boundary

Applications depend on one crate and import one prelude:

```rust
use outou::prelude::*;
```

They never write `use dioxus::prelude::*` and never list Dioxus in their `Cargo.toml`. Generated code reaches the backend through a hidden path, `::outou::__private::*`, which the `outou` crate re-exports. That crate is the leakage boundary: if the runtime changes later, `outou::prelude` stays as compatible as possible and generated code is the only thing that moves.

The consequence that follows from this: if a user ever sees backend vocabulary such as `rsx! macro`, `PropsBuilder`, `dioxus_rsx` or `GeneratedNode` in a diagnostic, Phase 0 has failed at that point. Rust's own type errors are fine; backend-specific errors are translated into Outou's vocabulary or hidden.

## If the bet fails

If the standalone `.rsx` format cannot be made to work, the same grammar and parser fall back to a procedural macro in ordinary `.rs` files:

```rust
fn App() -> Element {
    outou::jsx! {
        <main>
            <h1>Hello Outou</h1>
        </main>
    }
}
```

"Rust with JSX" survives that fallback. The main differentiator, JSX as a first-class Rust expression, does not. In that case the project is compared again with existing macro DSLs and continued only if it still has a reason to exist.

## Future

React interop remains an important goal for a 1.0 but is not started until the front end, the tooling and the runtime decision are settled, in that order.
