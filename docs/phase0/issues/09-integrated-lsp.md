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
- [x] completion, hover, definition forwarded with positions mapped both ways through the registry — **except** a JSX tag-name or attribute-name position, which is answered locally from Outou's own parse tree (`crates/outou-lsp/src/complete.rs`) and never forwarded at all: rust-analyzer only ever sees the *expanded* Rust, where that distinction does not exist (see `docs/gate3-results.md`, M3)
- [x] rust-analyzer and flycheck diagnostics mapped back to `.rsx`; Outou syntax diagnostics published alongside; an `ERROR`-severity diagnostic with no direct source span is still published, at the nearest mapped position, never dropped or downgraded
- [x] completion keeps working while typing (`<UserC`, `<UserCard us`, `<div class=`, `user.`) — see `docs/gate3-results.md` for the exact shape each probe used. A first review of this gate found the original "PASS" evidence had not actually verified several of these shapes correctly (a tag-name position reached rust-analyzer as generic Rust-identifier completion; an attribute-value position returned a wrongly-positioned edit that would corrupt the buffer); both are now fixed and re-verified, not merely re-documented — see `docs/gate3-results.md`'s corrected write-up.
- [x] definition across several `.rsx` files
- [x] latency measured against the budget in `docs/phase0.md`
- [x] every payload reaching the editor verified free of backend vocabulary (`docs/gate3-results.md`'s `assert_no_leakage`, applied recursively to every probe's full result — the first review found two live leaks, hover and completion, that the original evidence collection had not been checking for at all)

**Gate 3: PASS**, re-verified twice: once after a review found the original sign-off premature, and again after a second review found three more HIGH defects (H1-H3 — completion/hover leaking at a *closing* tag's own name, and an ordinary user type coincidentally named `…Props` losing its hover) plus a repeat of the latency-table transcription defect below. See `docs/gate3-results.md`'s own note at the top for both. Through the real parser, completion / hover / definition / diagnostics all work for `let user = load_user(); <UserCard user={user} />`, including cross-file definition and completion under incomplete input, for both the opening and closing side of a JSX element. Full writeup, evidence files and known limitations: [`docs/gate3-results.md`](../../gate3-results.md).

### Latency

**Generated, not transcribed** (issue #9 Gate 3 review, M4/MEDIUM-12, and its
repeat in the second review): this table and `docs/gate3-results.md`'s copy
of it are both printed verbatim by
[`spikes/rust-analyzer/client/gate3-latency-table.mjs`](../../../spikes/rust-analyzer/client/gate3-latency-table.mjs)
from one `cargo test -p outou-lsp --test gate3 -- --ignored --nocapture`
run's artifacts — re-run the script after any gate3 run and re-paste its
output into both documents rather than hand-editing either one. See
`docs/gate3-results.md`'s "Measured latency" for the full table (every row,
including the three new H1-H3 probes); this is the same run, trimmed to the
rows this issue originally tracked:

<!-- BEGIN GENERATED (subset): node spikes/rust-analyzer/client/gate3-latency-table.mjs -->

| Request | Latency (ms) |
|---|---|
| hover (`user`) | 13122 |
| definition (`load_user`, same file) | 5465 |
| definition (`UserCard`, cross-file) | 9709 |
| completion (member, `user.`) | 9911 |
| completion (tag, component) | 4 (answered locally; never forwarded) |
| completion (attribute name) | 4 (answered locally; never forwarded) |
| completion (prop value, `user={us}`) | 26246 |
| `didSave` -> mismatched-types diagnostic | 8929 |
| `didChange` -> Outou syntax diagnostic | 101 |

<!-- END GENERATED -->

These are real elapsed times to a *correct* answer (this pass's test client retries until the answer is useful, not merely non-`null`), not sleep durations and not the first-answer-however-wrong figures an earlier revision reported.

### Implementation notes

- `crates/outou-lsp` is a proxy over `lsp_server`/`lsp_types`: one thread
  reads the editor (via `lsp_server::Connection`), one reads a spawned
  rust-analyzer child (`crates/outou-lsp/src/ra.rs`, reusing
  `lsp_server::Message`'s own framing for that side too), correlated by a
  request-id map (`crates/outou-lsp/src/dispatch/{mod,requests,responses}.rs`).
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
  from disk. `outou-lsp` therefore re-plans and writes generated Rust to
  disk on save — through the same transactional Strict path `outou build`
  itself uses (`outou_cli::build::emit::emit`), never Recovery-mode
  placeholder text — and forwards the save to rust-analyzer
  (`crate::dispatch::handle_rsx_save`). Startup does the same for any
  *missing* generated file.
- `outou-lsp` strips `linkSupport` from the capabilities it forwards to
  rust-analyzer, so `textDocument/definition` responses are always plain
  `Location`/`Location[]`, never `LocationLink[]` — a deliberate
  simplification of the response-mapping code, not a capability the editor
  loses (an editor without `linkSupport` gets the same information from
  plain locations). It also forces `general.positionEncodings:
  ["utf-16"]` and refuses to use rust-analyzer at all if it answers
  otherwise, rather than trust whatever the editor and rust-analyzer
  happen to negotiate between themselves.
- `outou-cli::build::plan`/`emit::generate_unit` are reused directly rather
  than duplicated ("there is one compiler"): `outou-lsp` depends on
  `outou-cli`'s library, not just its binary. Planning now also accepts an
  in-memory overlay (`outou_modules::resolve_with_overlay`) so a `mod`
  typed into an unsaved buffer enters the graph immediately, not only
  after a save.
- A JSX tag-name or attribute-name completion position is answered from
  Outou's own parse tree (`crates/outou-lsp/src/complete.rs`) rather than
  ever being forwarded to rust-analyzer: rust-analyzer only ever sees the
  *expanded* Rust, where a tag name and an ordinary struct-literal
  identifier are indistinguishable, so no amount of response mapping can
  make that position answer correctly. Every other payload rust-analyzer
  does answer is sanitized before reaching the editor
  (`crate::response`, `crate::translate`) — diagnostic `data` cleared,
  `relatedInformation` reverse-mapped, completion items filtered by
  marker and by whether their edit range actually contains the cursor,
  hover contents stripped of backend path prefixes.
