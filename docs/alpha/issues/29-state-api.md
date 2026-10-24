---
title: "[A5] State API under Outou names (State<T>, use_state, provide_context, use_context, launch)"
milestone: "Alpha"
gate: A5
droppable: no
labels: [alpha, gate-a5, must-keep, cross-cutting]
---

| | |
|---|---|
| **Gate** | A5 |
| **Droppable** | No |

Decided in ADR 0014. `outou::prelude` re-exports Dioxus's state API under Dioxus names (ledger rows 8, 31, 33; friction F4, F6). Wrap it under Outou names with an API that never lets a guard escape, so the borrow trap of F4 cannot be written. This stays inside the A5 scope: a thin wrapper, not a new runtime, scheduler or update semantics.

- [x] `State<T>`, an Outou-defined newtype over Dioxus's `Signal` (`Copy`/`Clone`/`PartialEq` implemented by hand so it can be a prop). Read with `get() -> T` (`T: Clone`) and `with(|v| ..) -> R`. Write with `set(v)` and `update(|v: &mut T| ..)`, both on `&self`. `read()` and `write()` are not public.
- [x] `use_state`, `provide_context`, `use_context::<T>()`, and an Outou-owned `launch` (the opt-in `launch`/`web` features keep their meaning; `web` stays the only supported public feature). Subscription semantics are Dioxus's, documented.
- [x] Remove `use_signal`, `Signal`, `use_context_provider`, `ReadableExt` and `WritableExt` from the prelude without deprecation (alpha is not public; the only users are `examples/dogfood-board` and `examples/phase0-app`). Migrate both.
- [x] A test that pins the full list of names exported by `outou::prelude`. Backend items are no longer `pub use`d directly (ADR 0010 amendment), apart from the listed exceptions.
- [x] Update the ADR 0010 amendment table for the A4 re-exports. Append ledger rows: rows 8, 31 and 33 resolved or wrapped. Update Friction log F4 and F6.
- [x] Verification: the `dogfood-board` CI job passes (`outou build`, `cargo build`, `cargo test`, `cargo clippy`, wasm32 check); a browser re-walkthrough after migration. (Automated checks pass.)
  - Browser check (Fable, 2026-10-24): full walkthrough at the first #29 implementation (before `Slot`) - search `ken`, status chips with search, New task (empty-title error keeps the typed Assignee, valid save opens the detail), Edit (pre-filled, Save reflected), detail Status change updating list and chip counts, list checkbox toggle, switching the open form to another task, Delete returning the hint; then a smoke run at `997e851` (checkbox toggle and count, detail, search). No console errors. Not run: length limits, New task then Cancel, a live editor (F17).

**Acceptance:** the dogfood-board has no `read()`, `write()` or `let mut` for state; the pinned name list passes; the F4 pattern (`set(T { .. ..read().clone() })`) cannot be written.
