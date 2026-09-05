# 0010. The `outou` facade crate is the backend boundary

Status: Accepted

## Context

Generated code has to call into the backend (`rsx!`, `component`, `Element`). If generated code wrote `::dioxus::…`, every user crate would need Dioxus in its `Cargo.toml`, and `use dioxus::prelude::*` would leak into user code and documentation. The backend would become public API by accident.

## Decision

There is one public crate, `outou`. Users write `use outou::prelude::*;` and depend on `outou` only. Generated code references the backend through a hidden module, `::outou::__private::*`, which re-exports `rsx!`, `component`, `Element` and whatever else the lowering needs. A user crate's `Cargo.toml` never lists the backend.

The facade is the leakage boundary: if the runtime changes, `outou::prelude` keeps its public API as far as possible and only generated code and `__private` move.

## Consequences

- `#[component]` is required on Outou components in Phase 0; it is a backend-neutral marker in the AST and lowered by the backend.
- The backend's macros expand to unhygienic names, so the prelude also has to bring some backend names into scope (hidden from docs). This is recorded in the leakage ledger.
- Backend vocabulary appearing in user-facing diagnostics is a Phase 0 failure, because it means the boundary leaked.

## Alternatives considered

- **Generate `::dioxus::…` directly.** Simplest, and makes the backend public API.
- **Re-export the whole backend prelude publicly.** Same problem with a nicer name.
