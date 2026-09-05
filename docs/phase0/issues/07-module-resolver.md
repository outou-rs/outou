---
title: "[Week 4] Module resolver for mixed .rs / .rsx crates"
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
| **Crate** | `outou-modules` |

- [ ] candidates `foo.rsx`, `foo.rs`, `foo/mod.rsx`, `foo/mod.rs`; more than one existing candidate → `ambiguous Outou module `foo`` (`tests/fixtures/modules/ambiguous/`)
- [ ] `#[path = "…"]` honored; extension decides `.rsx` vs plain Rust
- [ ] `#[cfg(…)]` not evaluated: all modules generated, attribute preserved on the generated declaration
- [ ] explicit generated module graph (no reliance on rustc's implicit `.rs` lookup for generated files)
- [ ] `tests/fixtures/modules/mixed/` resolves: `.rsx → .rsx`, `.rsx → .rs`, `.rs → .rsx`, nested
