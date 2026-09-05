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

- [x] `didOpen` / `didChange` → regenerate (recovery mode) → overlay to rust-analyzer; single-file edits never regenerate the whole crate
- [x] completion, hover, definition forwarded with positions mapped both ways through the registry
- [x] rust-analyzer and flycheck diagnostics mapped back to `.rsx`; Outou syntax diagnostics published alongside
- [x] completion keeps working while typing (`<UserCard us`, `<div class=`, `user.`) — see `docs/gate3-results.md` for the exact shape each probe used and why a bare unclosed `<UserCard us` does not reach rust-analyzer as a props-builder context (recorded as a known limitation, not a Phase 0 STOP)
- [x] definition across several `.rsx` files
- [x] latency measured against the budget in `docs/phase0.md`

**Gate 3: PASS.** Through the real parser, completion / hover / definition / diagnostics all work for `let user = load_user(); <UserCard user={user} />`, including cross-file definition and completion under incomplete input. Full writeup, evidence files and known limitations: [`docs/gate3-results.md`](../../gate3-results.md).

### Latency (from `docs/gate3-results.md`, two runs)

| Request | Run 1 (ms) | Run 2 (ms) |
|---|---|---|
| hover (`user`) | 562 | 560 |
| definition (same file) | 529 | 540 |
| definition (cross-file) | 526 | 502 |
| completion (member) | 622 | 595 |
| completion (component) | 644 | 593 |
| completion (prop value) | 642 | 631 |
| `didSave` → type-mismatch diagnostic | 6003 | 6005 |
| `didChange` → Outou syntax diagnostic | 1504 | 1502 |

### Implementation notes

- `crates/outou-lsp` is a proxy over `lsp_server`/`lsp_types`: one thread
  reads the editor (via `lsp_server::Connection`), one reads a spawned
  rust-analyzer child (`crates/outou-lsp/src/ra.rs`, reusing
  `lsp_server::Message`'s own framing for that side too), correlated by a
  request-id map (`crates/outou-lsp/src/dispatch.rs`).
- Position/location mapping (`crates/outou-lsp/src/mapping.rs`) does not use
  `outou_sourcemap::SourceMap::to_generated`/`map_range` directly for
  single-position queries: those are designed for the many-to-many
  diagnostic/reference case and return a whole matching mapping's span,
  which is frequently much larger than one token (`Writer::verbatim` maps
  an entire spliced Rust block as one mapping). Positions are instead
  translated *proportionally* within the most specific (smallest) matching
  mapping — see that module's doc comments for the two bugs this caught
  during implementation (jumping to a mapping's start regardless of
  cursor position; picking the wrong of several overlapping source spans).
- Semantic (type-mismatch) diagnostics need `textDocument/didSave`, not
  just `didChange`: rust-analyzer's native diagnostics never report them
  (Week 1 spike finding), only flycheck does, and flycheck reads the file
  from disk. `outou-lsp` therefore writes the current generated text to
  disk on save and forwards the save to rust-analyzer
  (`crate::dispatch::handle_rsx_save`).
- `outou-lsp` strips `linkSupport` from the capabilities it forwards to
  rust-analyzer, so `textDocument/definition` responses are always plain
  `Location`/`Location[]`, never `LocationLink[]` — a deliberate
  simplification of the response-mapping code, not a capability the editor
  loses (an editor without `linkSupport` gets the same information from
  plain locations).
- `outou-cli::build::plan`/`emit::generate_unit` are reused directly rather
  than duplicated ("there is one compiler"): `outou-lsp` depends on
  `outou-cli`'s library, not just its binary.
