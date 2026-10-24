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

## Amendments

### Alpha A4 (issue #22): state and launch re-exports

The dogfood application (`examples/dogfood-board`) is interactive, so `outou::prelude` now also re-exports the temporary backend's existing state and launch APIs, unchanged and without any Outou-owned wrapper:

| Re-export | Why the app needs it | Ledger |
|---|---|---|
| `use_signal`, `Signal` | local and shared component state | `docs/backend-leakage.md` row 31 |
| `use_context_provider`, `use_context` | state shared between components without prop drilling | row 31 |
| `ReadableExt`, `WritableExt` | traits that provide `signal.read()` / `signal.set(..)`; they must be in scope | row 31 |
| `launch` (only with the new opt-in `launch` / `web` features of `outou`) | start the application in the browser | row 32 |

This is not Outou-owned hooks or signals: those stay out of scope for the alpha, and the runtime decision (A5) decides whether these names stay, are renamed or are replaced. Generated code is unchanged and still reaches the backend only through `::outou::__private::*`; the new names are used by application code. The `launch` / `web` features are off by default, and `web` is the only supported public one (`launch` is its internal building block: alone it selects no renderer and `launch(..)` panics). The six state re-exports (`use_signal`, `Signal`, `use_context_provider`, `use_context`, `ReadableExt`, `WritableExt`) are **not** feature-gated, so the prelude gains those names for every consumer even with the features off. Existing users are therefore not unaffected in one respect: a user who also glob-imports another module exporting the same name (for example another `Signal`) now gets an ambiguity error at the use site, and the new names appear in completion. Generated code and the facade's own build are unchanged. Gating the state names behind a feature was not chosen because every interactive application needs them and a second feature for them would only complicate the prelude; revisit with the runtime decision (A5).

### Alpha A5 (ADR 0014): `prelude` is Outou's API, `__private` is the backend

The runtime decision ([0014](0014-dioxus-continues-as-backend-not-as-specification.md)) keeps Dioxus as the current backend without making it the specification, and states the boundary as a rule: `outou::prelude` is Outou-owned API and `outou::__private` is the backend. **The prelude does not `pub use` a backend item directly.** The exceptions are the hidden names for macro hygiene (`dioxus_core`, `dioxus_elements`, `dioxus_signals`, `Props`; ledger rows 11 and 16) and these that remain until Outou owns an equivalent:

| Remaining direct export | Note |
|---|---|
| `Element` | Dioxus's own type. Row 4 of the ledger says `Option<VNode>`; that is out of date. In `dioxus-core 0.7.10` (`Cargo.lock`) it is `Result<VNode, RenderError>`. Rows are not rewritten, so this is the correction. |
| Event argument types | Handler arguments (row 9). |
| `VirtualDom` | Needed by tests that render a component without a renderer (row 36). |

**Final form (issue #29).** The state and launch re-exports of the A4 amendment above are gone. The table in the A4 section is the history; this is what `outou::prelude` exports now, pinned by `crates/outou-cli/tests/prelude_names.rs`:

| A4 re-export | A5 result |
|---|---|
| `use_signal` | removed; `use_state(|| init)` returns an Outou-defined `State<T>` |
| `Signal` | removed; `State<T>` is a newtype over it with `Copy` / `Clone` / `PartialEq` / `Debug` written by hand, so it can be a prop |
| `ReadableExt`, `WritableExt` | removed; `State<T>` has `get`, `with`, `set` and `update`, all on `&self`. No `read()` or `write()` exists, so no guard can be held, and `state.set(T { .., ..state.read().clone() })` (F4) cannot be written |
| `use_context_provider` | removed; `provide_context(\|\| value)` |
| `use_context` | kept as a name, now an Outou function: `use_context::<T>()` (`T: Clone`) |
| `launch` | kept as a name, now an Outou function `launch(app: fn() -> Element)` (only with the `launch` / `web` features; `web` is the only supported public one) |

The old names were removed without deprecation: the alpha is not public and the only users were `examples/dogfood-board` and `examples/phase0-app` (the latter used none). The prelude now exports exactly: `Element`, `component`, `State`, `use_state`, `provide_context`, `use_context`, `launch` (with the feature) and the hidden `dioxus_core`, `dioxus_elements`, `dioxus_signals`, `Props`. The remaining direct backend exports are only `component`, `Element` and the four hidden macro-hygiene names; event argument types and `VirtualDom` are allowed by the exception list above but are not exported (a test reaches `VirtualDom` through the hidden `dioxus_core`). Subscription is the backend's: `get` and `with` subscribe the running component, `set` and `update` notify. A `State<T>` is also a JSX child when `T` is (`IntoChild`, ledger row 44).
