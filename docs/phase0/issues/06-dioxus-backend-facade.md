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

- [ ] `Backend` trait with `Strict` and `Recovery` modes sharing one AST and one source-map infrastructure
- [ ] children lowered as separate nodes (`"Hello "`, `{name}`), never a format string
- [ ] all runtime references go through `::outou::__private::rsx!` and friends; user crates do not list the backend in `Cargo.toml`
- [ ] `#[component]` kept as a backend-neutral marker and lowered by the backend
- [ ] `outou::prelude` provides `Element` and `component`; hidden names needed by macro expansion are documented in `docs/backend-leakage.md` (row 11) — decide whether codegen emits fully qualified paths instead
- [ ] doc comments and `#[cfg(test)]` preserved verbatim in generated code
- [ ] recovery mode emits analyzable Rust for every `tests/fixtures/incomplete/` case
