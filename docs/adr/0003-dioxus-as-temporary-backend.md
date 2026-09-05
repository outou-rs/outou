# 0003. Dioxus is the temporary execution backend

Status: Accepted

## Context

Earlier plans included an Outou runtime: VDOM, hooks, renderer. None of that is where the uncertainty lies. The uncertain and valuable part is whether JSX can live in Rust with Rust's tooling intact. Building a runtime first would spend the time box on the known part.

## Decision

Phase 0 builds no runtime. Outou JSX is lowered to Dioxus `rsx!` and executed by Dioxus. Dioxus is a backend behind a trait, not a public specification, and the Outou AST is never converted into a Dioxus AST.

Every constraint Dioxus imposes on Outou code is written down in the backend leakage ledger (`docs/backend-leakage.md`) and classified. After the front end is proven, a runtime decision is made from that ledger: keep Dioxus, or build a native runtime.

## Consequences

- Props are `Clone + PartialEq + 'static`, `Element` is Dioxus's, borrowed props do not work. These are recorded as leakage, not accepted as Outou semantics.
- Backend diagnostics must be translated; backend vocabulary reaching users is a Phase 0 failure.
- The backend is one implementation of a `Backend` trait; a second backend must be possible without touching the front end.

## Alternatives considered

- **Native runtime first.** Spends the time box on the part that is not in doubt.
- **Yew or Leptos as backend.** Dioxus's `rsx!` is closest in shape to the lowered form Outou needs (component calls with named props and child lists), and its `#[component]` matches Outou's marker.
