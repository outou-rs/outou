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

Measured for both layouts; see `docs/ra-spike-results.md` for evidence files. Layout (b) (`src/.generated/` + `#[path]`) satisfies all seven. Layout (a) (`OUT_DIR` + `include!`) fails item 4.

- [x] 1. rust-analyzer loads the Cargo project normally — both layouts
- [x] 2. the generated source is in the crate graph — both layouts (cross-file hover and definition into `src/main.rs`)
- [x] 3. editor changes reach rust-analyzer **without re-running `build.rs`** (decisive) — both layouts; layout (a) verified by unchanged `$OUT_DIR`/build-script-output mtimes and an identical generated-file sha256 before/after an overlay-change probe run while staying in the `gen-outdir` feature state (`spikes/rust-analyzer/results/a-overlay-change-mtimes.txt`)
- [x] 4. completion reflects the latest buffer — **layout (b) only**. Layout (a) returns zero/`null` completions inside the generated `rsx!` macro call and under incomplete input (same probes layout (b) passes); this is why layout (a) is rejected
- [x] 5. hover reflects the latest buffer — both layouts
- [x] 6. definition reflects the latest buffer — both layouts; a use-site definition tracks a `didChange` edit that shifts the declaration by one line (`spikes/rust-analyzer/results/b-definition-latest-buffer-{cold,warm}.json.gz`, `spikes/rust-analyzer/results/a-definition-latest-buffer-{cold,warm}.json.gz`)
- [x] 7. `cargo check` / flycheck diagnostics map back to `App.rsx` — both layouts, via flycheck (native pre-save diagnostics only ever showed syntax errors, not semantic ones, in either layout; noted as a known blocker, not a Strategy-A failure)

### Deliverables

- [x] `docs/ra-spike-results.md` filled in: per-layout results, blockers, latency
- [x] recommendation for ADR 0009 (layout) and, if (b) passes, a note that `OUT_DIR` and `outou-build` are dropped — recommendation: adopt (b), drop `OUT_DIR` generation and `crates/outou-build`
