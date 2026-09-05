---
title: "[Week 4] Dioxus backend codegen, #[component] lowering and the outou facade"
milestone: "Phase 0"
week: "Week 4"
gate: "—"
droppable: no
labels: [phase0, must-keep]
---

| | |
|---|---|
| **Gate** | none (required by Gate 2) |
| **Droppable** | No |
| **Crates** | `outou-codegen`, `outou-backend-dioxus`, `outou` |

- [x] `Backend` trait with `Strict` and `Recovery` modes sharing one AST and one source-map infrastructure
- [x] children lowered as separate nodes (`"Hello "`, `{name}`), never a format string
- [x] all runtime references go through `::outou::__private::rsx!` and friends; user crates do not list the backend in `Cargo.toml`
- [x] `#[component]` kept as a backend-neutral marker and lowered by the backend
- [x] `outou::prelude` provides `Element` and `component`; hidden names needed by macro expansion are documented in `docs/backend-leakage.md` (row 11) — decide whether codegen emits fully qualified paths instead (decided, row 16: no, the vendored `dioxus-rsx`/`dioxus-core-macro` 0.7.10 expansions use bare, unqualified paths)
- [x] doc comments and `#[cfg(test)]` preserved verbatim in generated code
- [x] recovery mode emits analyzable Rust for every `tests/fixtures/incomplete/` case
