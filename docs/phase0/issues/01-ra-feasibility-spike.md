---
title: "[Week 1 / Gate 0] rust-analyzer feasibility spike"
milestone: "Phase 0"
week: "Week 1"
gate: "Gate 0"
droppable: no
labels: [phase0, gate-0, must-keep, spike]
---

| | |
|---|---|
| **Gate** | 0 (kill check: a STOP here ends Phase 0) |
| **Droppable** | No |
| **Where** | `spikes/rust-analyzer/` |

No parser. Hand-written `App.rsx`, hand-written generated Rust (`virtual/App.rs`), a hand-written many-to-many `source-map.json` and a Cargo fixture. Drive rust-analyzer with the headless JSON-RPC client in `client/` and check that completion, hover, definition and flycheck diagnostics round-trip.

Try **Strategy A** in both layouts:

- (a) `OUT_DIR` overlay: `build.rs` → `OUT_DIR/outou/App.rs` → `include!`. Get the hashed path from `cargo check --message-format=json` (`build-script-executed`).
- (b) fixed path overlay: `src/.generated/App.rs` referenced with `#[path]`.

### Checklist (all seven required for Strategy A)

- [ ] 1. rust-analyzer loads the Cargo project normally
- [ ] 2. the generated source is in the crate graph
- [ ] 3. editor changes reach rust-analyzer **without re-running `build.rs`** (decisive)
- [ ] 4. completion reflects the latest buffer
- [ ] 5. hover reflects the latest buffer
- [ ] 6. definition reflects the latest buffer
- [ ] 7. `cargo check` / flycheck diagnostics map back to `App.rsx`

### Deliverables

- [ ] `docs/ra-spike-results.md` filled in: per-layout results, blockers, latency
- [ ] recommendation for ADR 0009 (layout) and, if (b) passes, a note that `OUT_DIR` and `outou-build` are dropped
