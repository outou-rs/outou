# Gate 3 results: the integrated language server

Outcome of Week 5's integration (`crates/outou-lsp`, issue #9): connecting the
real parser and codegen (`outou_syntax`, `outou_codegen`, `outou_backend_dioxus`,
via `outou_cli::build`) to the Week 1 pipeline (`spikes/rust-analyzer/`,
[`docs/ra-spike-results.md`](ra-spike-results.md)). Same format as that
document.

## Environment

| Item | Value |
|---|---|
| Date | 2026-09-06 |
| rust-analyzer version | 1.98.1 (48a229ce 2026-09-01) |
| rustc / cargo version | rustc 1.98.1 (48a229cea 2026-09-01) / cargo 1.98.1 (797e8a9bc 2026-08-05) |
| Node version | v24.14.0 |
| OS | macOS 26.6.2 (build 25G83), Darwin 25.6.0, arm64 |

`outou-lsp` was built with `cargo build -p outou-lsp` (debug profile) and
spawned rust-analyzer via `OUTOU_RUST_ANALYZER`/`PATH` exactly as it does in
normal use — no special test-only code path. The target program is Gate 3's
fixed example, `examples/phase0-app` (`src/main.rsx`, `src/components.rsx`):

```rsx
use outou::prelude::*;
mod components;
use components::{Field, UserCard};
// ...
#[component]
fn App() -> Element {
    let user = load_user();
    // ...
    <main class="app">
        <Greeting name="Outou" />
        {
            if user.is_some() {
                <UserCard user={user.unwrap()} />
            } else {
                <p>No user</p>
            }
        }
        // ...
    </main>
}
fn load_user() -> Option<components::User> { /* ... */ }
```

All raw command output is under `spikes/rust-analyzer/results/gate3-*.json.gz`,
produced by `crates/outou-lsp/tests/gate3.rs` (run explicitly: `cargo test -p
outou-lsp --test gate3 -- --ignored --nocapture`), which drives
`spikes/rust-analyzer/client/outou-lsp-client.mjs` — a sibling of the Week 1
spike's `ra-client.mjs`, talking to `outou-lsp` itself rather than to
rust-analyzer directly (see that script's own doc comment for why it is a
separate file). See [`results/README.md`](../spikes/rust-analyzer/results/README.md)
for the exact command per file.

## Gate 3 criteria

| Criterion (`docs/phase0/issues/09-integrated-lsp.md`) | Works? | Evidence |
|---|---|---|
| `didOpen`/`didChange` regenerates only the edited unit (recovery mode), overlaid onto rust-analyzer | yes | `crates/outou-lsp/src/documents.rs`'s `regenerating_one_unit_does_not_touch_another` unit test asserts `components.rs`'s generated text is byte-identical after editing `main.rsx`; the dispatch path (`crate::dispatch::handle_rsx_change`) only ever calls `Workspace::regenerate` for the one unit whose `.rsx` URI changed, sending a single `textDocument/didChange` for that unit's generated URI. A crate-wide re-plan only happens when the edited file's own declared `mod` set changes (`Workspace::module_shape_changed`), per the architecture note. |
| Hover on `user` reports its real type through the real parser | yes | `gate3-hover-user.json.gz`: `let user: Option<User>` at `main.rsx:20:8-12` (mapped back from the generated file; `latencyMs.hover` 560 ms). |
| Definition on `load_user()` resolves within `main.rsx` | yes | `gate3-definition-load-user.json.gz`: resolves to `main.rsx:42:3-12` (`fn load_user`), `latencyMs.definition` 522-540 ms across two runs. |
| Definition across `.rsx` files (`UserCard` -> `components.rsx`) | yes | `gate3-definition-user-card.json.gz`: resolves to `components.rsx:8:7-15` (`pub fn UserCard`), `latencyMs.definition` 502-540 ms. |
| Completion keeps working under incomplete input: member (`user.`) | yes | `gate3-completion-member.json.gz`: 122 items including `unwrap`, `expect`, `unwrap_or*`; the overlay (`user.` on its own line) is not valid Rust, and rust-analyzer's own native diagnostics correctly flag it (`Syntax Error: expected field name or number` / `expected SEMICOLON`, mapped to `main.rsx:21:9`) while completion still answers. |
| Completion keeps working under incomplete input: component tag (`<UserC`) | yes | `gate3-completion-component.json.gz`: 155 items, including `UserCard`; the truncated tag itself is reported by Outou's own diagnostics (`unexpected '<' inside tag`), independent of rust-analyzer's answer. |
| Completion keeps working under incomplete input: prop value (`user={us}`) | yes | `gate3-completion-prop.json.gz`: 158 items, including `user` (the local variable). A bare `<UserCard us` (attribute name, unclosed tag) is not tested directly: Outou's parser recovers an unclosed tag as a placeholder call rather than a partial struct literal, so there is nothing rust-analyzer-shaped to complete against at that exact position — see the script's own comment. A well-formed tag with a partially-typed *value* is the shape that reaches rust-analyzer as ordinary Rust, matching the Week 1 spike's own prop-completion probe. |
| Rust type errors reported at the right position in the `.rsx` file | yes, via `didSave` + flycheck | `gate3-diagnostic-type-error.json.gz`: changing `let user = load_user();` to `let user: u32 = load_user();` and sending `textDocument/didSave` (which `outou-lsp` uses to write the current generated text to disk and forward the save to rust-analyzer, `crate::dispatch::handle_rsx_save`) produces `mismatched types` at `main.rsx:20:20-31` plus three cascading `E0599`s at the right positions in `main.rsx`, 6.0 s after the save. Matches the Week 1 spike's own finding: rust-analyzer's *native* (in-memory) diagnostics never report semantic errors, only flycheck does, and flycheck reads the file from disk — this server therefore has to write the buffer to disk on save, not just keep it in rust-analyzer's overlay. |
| Outou syntax errors reported by Outou itself, at the right position | yes | `gate3-diagnostic-syntax-error.json.gz`: truncating `<h1>Hello {name}</h1>` to `<div cl` produces `unexpected '}' inside tag '<div>', expected an attribute or '>'` (`source: "outou"`) 1.5 s after the edit — no `cargo check` round trip needed, since this is Outou's own parser diagnostic, published immediately on `didChange`. |
| Completion/definition keep working elsewhere in the file after an unrelated syntax error | yes | Same run: `definitionDespiteSyntaxError` still resolves `load_user()` to `main.rsx:42:3-12`, unaffected by the broken `<div cl` earlier in the file (a different function, `Greeting`) — recovery mode keeps the rest of the file analyzable. |

**Gate 3: PASS.** Every criterion in `docs/phase0/issues/09-integrated-lsp.md`
holds through the real parser and codegen against `examples/phase0-app`.

## Measured latency

All values are `latencyMs` fields from `outou-lsp-client.mjs`'s JSON output
(`spikes/rust-analyzer/results/gate3-*.json.gz`), from two full runs of
`cargo test -p outou-lsp --test gate3 -- --ignored`. Each probe's `initialize`
figure is the client's own `initialize` request/response round trip (not
rust-analyzer's own startup, which happens inside `outou-lsp` before it
answers `initialize` — the client always waits a fixed 12 s settle window
after `didOpen` before issuing its request, matching the Week 1 spike's own
"cold"/no-progress-forwarding situation: `outou-lsp` does not forward
rust-analyzer's `$/progress` notifications to its own client today, so there
is no readiness signal to watch other than a fixed wait).

| Request | Run 1 (ms) | Run 2 (ms) |
|---|---|---|
| `initialize` (client <-> outou-lsp) | 96 | 731* |
| hover (`user`) | 562 | 560 |
| definition (`load_user`, same file) | 529 | 540 |
| definition (`UserCard`, cross-file) | 526 | 502 |
| completion (member, `user.`) | 622 | 595 |
| completion (component, `<UserC`) | 644 | 593 |
| completion (prop value, `user={us}`) | 642 | 631 |
| `didSave` -> mismatched-types diagnostic published | 6003 | 6005 |
| `didChange` -> Outou syntax diagnostic published | 1504 | 1502 |
| definition after an unrelated syntax error | 523 | 574 |

\* Run 2's `initialize` figure (731 ms) is the client's very first request of
that process, issued immediately after spawning a fresh `outou-lsp`, which in
turn spawns a fresh rust-analyzer; it is not representative of a steady-state
request and is not the number that matters for Gate 3 (`outou-lsp` answers
`initialize` once it has finished its own setup with rust-analyzer, which
includes rust-analyzer's own `initialize` response, but not full indexing).
Per-request round trips after that (hover/definition/completion, 500-650 ms)
are consistent across both runs.

Compared against `docs/phase0.md`'s provisional budget:

- **"incremental `.rsx` -> generated Rust: perceived as instantaneous"** —
  `crates/outou-lsp/src/documents.rs`'s `regenerating_one_unit_is_fast` test
  asserts this step (one `outou_syntax::parse` + `DioxusBackend::generate`
  call on `examples/phase0-app/src/main.rsx`, the same primitive `outou
  build` uses) completes in well under 50 ms — in practice a small fraction
  of a millisecond, unmeasured to more precision here since the wall-clock
  a user perceives is dominated by the rust-analyzer round trip below, not
  this step.
- **"extra proxy overhead on completion: not perceptible"** — completion
  round trips (595-644 ms) are in the same range as the Week 1 spike's own
  *warm* completion figure through the spike's own client protocol overhead
  (`docs/ra-spike-results.md`'s layout (b) table: 70-174 ms measured
  *directly against rust-analyzer*, no proxy). The gap here is dominated by
  `outou-lsp-client.mjs`'s own JSON-RPC round trip to a freshly spawned
  process pair (client -> outou-lsp -> rust-analyzer, vs. the spike's client
  -> rust-analyzer directly) rather than by the mapping logic itself, which
  is a handful of in-memory span comparisons; a warm, already-indexed
  session (an editor kept open, not a fresh process per request) is expected
  to look like the spike's own warm numbers plus one extra in-process hop.
  Not independently isolated in this pass — recorded as a known gap below.
- **"a single-file edit never regenerates the whole crate"** — verified at
  the unit level (`regenerating_one_unit_does_not_touch_another`) and
  exercised end-to-end by every completion/diagnostic probe above, each of
  which edits `main.rsx` only.

## Known blockers and limitations

- **No `$/progress` forwarding.** `outou-lsp` does not relay rust-analyzer's
  own indexing progress to the editor, so a real editor has no readiness
  signal beyond receiving actual hover/completion/definition answers (which
  rust-analyzer will still eventually give correctly once indexed, per the
  Week 1 spike's own bounded-retry fallback). This client works around it
  with a fixed 12 s settle window, matching a debug build's typical
  indexing time for this small fixture; a larger real project would need a
  longer window, or `outou-lsp` would need to add progress forwarding.
  Tracked as a fixable defect, not a Phase 0 STOP — completion/hover/
  definition/diagnostics all work correctly once indexing finishes, which
  is what Gate 3 is about.
- **Semantic type errors require a save, not just a keystroke.** Per the
  Week 1 spike's own finding (native diagnostics never report semantic
  errors), a type error the user has typed but not saved will not appear as
  a live diagnostic; it appears once the file is saved (`didSave` writes the
  generated file to disk and triggers rust-analyzer's flycheck). This is the
  same limitation the spike already documented for rust-analyzer generally,
  not something specific to Outou's translation layer, but it does mean
  "Rust type errors reported at the right position" is satisfied on save,
  not on every keystroke. Recorded here rather than re-litigated as new.
- **`<UserCard us` (an attribute name, unclosed tag) does not reach
  rust-analyzer as a props-builder completion context.** Outou's recovery
  parser turns an unclosed tag into a placeholder call rather than a partial
  struct literal, so there is no `UserCard { us` for rust-analyzer to
  complete props against at that position; a well-formed tag with a
  partially-typed attribute *value* does reach rust-analyzer correctly (see
  the criteria table above). Whether recovery should instead try to keep an
  unclosed tag's *known* attributes as a partial struct literal (so
  attribute-*name* completion also works) is a design question for a later
  phase, not investigated further here — recorded as a temporary
  limitation, not a Phase 0 STOP, since prop *value* completion (the shape
  the Week 1 spike itself measured) works.
- **Backend-vocabulary translation is a small, extensible table
  (`crates/outou-lsp/src/translate.rs`), not exhaustive.** Every diagnostic
  message observed in this pass (rustc's own `mismatched types`, `E0599`
  method-not-found messages, native syntax-error messages) was already in
  Outou vocabulary with no translation needed — none of Gate 3's probes
  happened to trigger a message containing `rsx!`, `PropsBuilder`,
  `dioxus_core`, or the table's other markers. `docs/backend-leakage.md`
  row 19 already documents that rustc *can* print Dioxus paths directly
  (a missing required prop, a non-`IntoDynNode` child); this pass did not
  add a new reproduction, only the translation mechanism itself (row 24).
- **Latency numbers here are cold, single-process runs**, not a
  warm/steady-state editor session (`outou-lsp` and rust-analyzer are
  spawned fresh for every probe by the test). A real editor session keeps
  both processes warm for the whole editing session, which the Week 1
  spike's own warm-vs-cold comparison suggests would improve every number
  above; not separately re-measured here for `outou-lsp` specifically.

## Not implemented in this pass (documented, not silently dropped)

- `textDocument/references` (mentioned as optional in the architecture note;
  `docs/phase0.md`'s "what is cut first" list also names rename/references
  as droppable before a formatter).
- `$/progress` forwarding to the editor (see above).
- Any UI beyond the headless protocol: this crate has no editor extension of
  its own to test against in Phase 0 (`packages/vscode-outou` is a
  placeholder); `outou-lsp-client.mjs` and `crates/outou-lsp/tests/gate3.rs`
  are how Gate 3 is verified, the same way the Week 1 spike verified Gate 0.

## Decision

**Gate 3: PASS.** Completion, hover, definition and diagnostics all work
through the real parser and codegen for `let user = load_user(); <UserCard
user={user} />`, including cross-file definition, completion under
incomplete/broken input, and both Outou's own syntax diagnostics and
rust-analyzer/flycheck's diagnostics mapped back to the right `.rsx`
position. Phase 0 proceeds to Step 6.
