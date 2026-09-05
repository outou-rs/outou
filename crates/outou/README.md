# outou

Public facade crate. Applications depend on this crate only and write `use outou::prelude::*;`.
The hidden `__private` module re-exports the execution backend for generated code; it is not public API.

Phase 0: Week 4 (Gate 2).

## `__private`

- `component`, `Element`, and the backend macro-hygiene names (`dioxus_core`, `dioxus_elements`, `dioxus_signals`, `Props`) documented as leakage in `docs/backend-leakage.md` (row 11; row 16 records why generated code cannot avoid needing them in scope).
- `recovery_element() -> Element` and `recovery<T>() -> T`: recovery-mode placeholders emitted by `outou-backend-dioxus` for a construct it could not reconstruct. Both `unreachable!()` if ever actually called — recovery-mode generated Rust is for rust-analyzer analysis only and is never fed to `rustc` through `cargo build`. See `crates/outou-backend-dioxus/README.md`'s "Recovery-mode placeholders" section for exactly when each is used.

This crate is the only one in the workspace allowed to depend on `dioxus` (`AGENTS.md`).
