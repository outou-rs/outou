# Backend leakage ledger

Outou uses Dioxus as a temporary execution backend. Every constraint that Dioxus imposes on Outou code is recorded here and classified so that the runtime decision after Phase 0 is made from a list, not from a feeling.

Classifications:

- **Outou intrinsic** — the behavior is part of Outou's own semantics and would exist with any backend.
- **Dioxus leakage** — a Dioxus detail visible to Outou users. Must not become permanent specification.
- **Temporary limitation** — something Outou intends to support but cannot with this backend.
- **Accepted permanent behavior** — a Dioxus behavior Outou has decided to keep on its own merits.

| # | Constraint | Origin | Classification | Notes |
|---|---|---|---|---|
| 1 | Props must be `Clone` | Dioxus `#[component]` / `Props` derive | Dioxus leakage | Not an Outou language rule. Revisit at the runtime decision. |
| 2 | Props must be `'static` | Dioxus component model | Dioxus leakage | Follows from #1 and #9. |
| 3 | Props must be `PartialEq`, and equality drives re-rendering | Dioxus memoization | Dioxus leakage | Outou has not defined its own update semantics yet. |
| 4 | `Element = Option<VNode>` | Dioxus core | Dioxus leakage | Outou exposes the name `Element` only. Whether it stays an `Option` is a runtime decision. |
| 5 | The type of `children` is Dioxus's `Element` | Dioxus | Dioxus leakage | Same as #4. |
| 6 | `#[component]` is lowered to a Dioxus component definition (a props struct plus a function) | Outou marker, Dioxus lowering | Outou intrinsic (marker) / Dioxus leakage (lowering) | The attribute itself is backend-neutral and required in Phase 0. Only the generated shape is Dioxus. |
| 7 | `rsx!` is re-exported through `outou::__private` and appears in generated code | Codegen | Temporary limitation | Hidden path; never written by users. Appearing in a user-facing diagnostic is a Phase 0 failure. |
| 8 | Signals, hooks and their rules (`use_*` ordering, `Signal<T>` API) are Dioxus's | Dioxus | Dioxus leakage | Outou defines no hook or signal of its own in Phase 0. |
| 9 | Event model: handler types, event names and the `on*` attribute convention | Dioxus HTML crate | Dioxus leakage | Outou JSX event syntax is Outou's; the handler types are not. |
| 10 | `&str` (borrowed) props are not supported: `fn Greeting(name: &str) -> Element` does not compile | Dioxus `'static` props | Temporary limitation | Use `String` in Phase 0. Borrowed props are an Outou goal. |
| 11 | Macro expansion names in scope: the backend's macros expand to unhygienic paths, so `outou::prelude` must also bring `dioxus_core`, `dioxus_elements`, `dioxus_signals` and `Props` into scope (hidden from docs) | Dioxus macro hygiene | Dioxus leakage | Discovered while building the facade. These names are visible to completion in user code. Codegen could instead emit fully qualified paths; TODO(phase0) decide at Gate 2. |
| 12 | Element and attribute vocabulary (`div`, `class`, …) is validated by the Dioxus HTML crate | Dioxus HTML crate | Temporary limitation | An unknown attribute produces a Dioxus error today. Outou must translate it (see [design.md](design.md)). |

Rows are appended, never rewritten, as new leakage is found. The runtime decision after Phase 0 counts how many rows make Outou semantics unnatural and weighs that against interop freedom, renderer control, performance, bundle size, API stability and upgrade cost.
