---
title: "[Week 5 / Gate 3] Integrated language server"
milestone: "Phase 0"
week: "Week 5"
gate: "Gate 3"
droppable: no
labels: [phase0, gate-3, must-keep]
---

| | |
|---|---|
| **Gate** | 3 |
| **Droppable** | No |
| **Crate** | `outou-lsp` |

Connect the real parser and codegen to the Week 1 pipeline.

- [ ] `didOpen` / `didChange` → regenerate (recovery mode) → overlay to rust-analyzer; single-file edits never regenerate the whole crate
- [ ] completion, hover, definition forwarded with positions mapped both ways through the registry
- [ ] rust-analyzer and flycheck diagnostics mapped back to `.rsx`; Outou syntax diagnostics published alongside
- [ ] completion keeps working while typing (`<UserCard us`, `<div class=`, `user.`)
- [ ] definition across several `.rsx` files
- [ ] latency measured against the budget in `docs/phase0.md`

**Gate 3:** through the real parser, completion / hover / definition / diagnostics do not work for `let user = load_user(); <UserCard user={user} />` → STOP.
