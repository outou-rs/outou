# outou-lsp

Language server for `.rsx` files (language id `outou-rsx`). Proxies
rust-analyzer and maps positions/locations through `outou-sourcemap`'s
`Registry` in both directions, using the exact same parser and codegen
`outou build` uses ("there is one compiler", `AGENTS.md`) — only the mode
differs (`Mode::Recovery` for the editor overlay; on-disk generated files
are always written in `Mode::Strict`, the same mode `outou build` uses,
never a Recovery placeholder).

Phase 0: Week 5 (Gate 3). Full write-up, evidence and known limitations:
[`docs/gate3-results.md`](../../docs/gate3-results.md).

## Two processes, not one

This server does not run rust-analyzer's own analysis itself: it spawns a
**second, separate rust-analyzer process** per workspace and proxies to
it. An editor that also runs its own rust-analyzer for ordinary `.rs`
files (which most do) therefore ends up with two rust-analyzer processes
against the same crate — this server's own, and the editor's. That is the
documented Phase 0 arrangement (issue #9's architecture note; the Week 1
spike ran the same way), not an accident: `outou-lsp` needs full control
over what text rust-analyzer sees for a generated file (the editor
overlay), and driving the editor's own rust-analyzer for that would need
a different protocol entirely (custom notifications an ordinary
rust-analyzer does not understand).

## Running it

```bash
cargo build -p outou-lsp
target/debug/outou-lsp
```

An editor connects over stdio and sends the usual LSP `initialize`
handshake with `workspaceFolders` pointing at the crate root. On startup
this server:

1. Resolves the crate's `.rsx` root (`src/main.rsx`/`lib.rsx`); if none
   exists, it runs in a **degraded mode** that only publishes Outou syntax
   diagnostics (no rust-analyzer is spawned).
2. Otherwise, plans the whole crate (`outou_cli::build::plan`) and
   generates every `.rsx` unit in `Mode::Recovery` for the in-memory
   editor overlay.
3. If any unit's generated file is missing on disk, writes **every**
   unit once through the same transactional Strict path `outou build`
   itself uses (`outou_cli::build::emit::emit`) — never Recovery-mode
   placeholder text, and never written at all if Strict generation fails
   (a real syntax error before `outou build` has ever run). An
   already-complete set of generated files (however stale) is left
   alone.
4. Spawns rust-analyzer (`OUTOU_RUST_ANALYZER` env var, else the first
   `rust-analyzer` on `PATH`; the process fails fast with an
   Outou-vocabulary message if neither resolves, before ever starting the
   LSP handshake) with the same root, forwarding the editor's own
   capabilities (minus `linkSupport`, so `textDocument/definition`
   responses are always plain `Location`s; `general.positionEncodings`
   is forced to `["utf-16"]` regardless of what the editor sent, and this
   server refuses to use rust-analyzer at all if it does not agree) and
   `initializationOptions`, plus `checkOnSave: true` and
   `cargo.buildScripts.enable: true`. `didOpen`s every generated unit's
   current text as rust-analyzer's overlay (Strategy A, ADR 0005).

After that:

- `textDocument/{didOpen,didChange}` for a `.rsx` file regenerates only
  that unit (in `Mode::Recovery`, for the overlay) and forwards a
  `didChange` to rust-analyzer for its generated file, unless the edited
  file's own set of `mod` declarations changed, in which case the whole
  crate is re-planned — using every currently open buffer's own text, not
  what is on disk, so an unsaved `mod` declaration is seen immediately —
  and every generated unit is resynced (`didOpen` for a newly-planned
  one, `didChange` — with a version that keeps counting up, never resets
  — for one that already existed, `didClose` for one that no longer
  exists).
- `textDocument/completion` first classifies the `.rsx` cursor against
  Outou's own parse tree (`src/complete.rs`): a JSX tag name (element or
  component, opening **or closing**) or attribute name is answered
  **locally**, from the plan's `#[component]` functions and a static
  HTML element list — never forwarded to rust-analyzer, which only ever
  sees the *expanded* Rust and cannot tell a tag-name position from an
  ordinary identifier, and would in any case reverse-map a closing tag's
  own name back to its opening tag's position (issue #9 Gate 3 review,
  H1). Every other position (`user.`, a prop *value*) is forwarded and
  mapped as below.
- `textDocument/hover` uses the same tag-name classification: a tag name
  is answered `null` **locally** too (issue #9 Gate 3 review, H2) —
  neither an HTML element's nor a user component's generated occurrence
  has anything useful to show once sanitized, and the closing tag's own
  occurrence has the same reverse-mapping ambiguity as completion's. Every
  other hover, and every `textDocument/definition`, maps the position to
  the generated file, forwards the request, and maps the response back,
  sanitizing it on the way (see "What gets sanitized" below).
- `textDocument/publishDiagnostics` from rust-analyzer is mapped back,
  merged with Outou's own syntax diagnostics, and translated out of
  backend vocabulary; an unmapped diagnostic (synthesized code, no direct
  `.rsx` span) is still published, at the nearest mapped position, if its
  severity is `ERROR` — never silently dropped or downgraded.
- `textDocument/didSave` re-plans the crate from the now-on-disk sources
  and writes every unit through the same transactional Strict path as
  startup, then forwards the save to rust-analyzer for the saved file's
  generated unit, so its `checkOnSave` flycheck — the only mechanism that
  reports semantic (type-mismatch) errors, per the Week 1 spike — runs
  against a build that actually compiles. If Strict generation fails
  anywhere in the plan, nothing is written and the save is not forwarded;
  for the saved file's own syntax error, Outou's own syntax diagnostic
  (published by the ordinary `didChange` path) already explains why, but
  if the plan was blocked by some *other* `.rsx` file's syntax error, this
  server also sends a `window/showMessage` (Warning) naming that file, so
  the block is visible even to a user who isn't looking at it.
- `$/cancelRequest` is rewritten through this server's own pending-request
  map before being forwarded, so it cancels the right rust-analyzer
  request even though the editor's and this server's request-id spaces
  are independent counters that can otherwise collide.

### What is and is not forwarded to rust-analyzer

Not "everything else is forwarded transparently" — several
rust-analyzer -> client requests are answered by this server itself, and
the ones that are forwarded get a freshly allocated id (tracked
internally) rather than rust-analyzer's own:

| Method | What happens |
|---|---|
| `workspace/configuration` | Answered locally: an array of `null`s, one per requested item — never a bare `null`, which is not a valid result shape for this request. |
| `client/registerCapability` | Forwarded to the editor (under a fresh id) only if the editor's own `initialize` capabilities advertised `dynamicRegistration: true` somewhere; otherwise answered locally with `null`. |
| `window/workDoneProgress/create` | Forwarded to the editor (under a fresh id) only if the editor advertised `window.workDoneProgress`; otherwise answered locally with `null`. |
| `$/progress` (a notification, not a request) | Forwarded to the editor verbatim, under its own token, only if the editor advertised `window.workDoneProgress` (the same condition that gates forwarding `window/workDoneProgress/create`); otherwise dropped. |
| Every other rust-analyzer -> client request | Answered locally with `null`. |
| Every client -> server request/notification not named above | Forwarded to rust-analyzer verbatim (after any position mapping this document already described). |

### What gets sanitized

Every payload built from rust-analyzer's answer against *generated* Rust
is sanitized before it reaches the editor, not just position-mapped
(`AGENTS.md`: backend vocabulary in user-facing output is a Phase 0
failure):

- A diagnostic's `data` field is always cleared, never used to stash the
  original (possibly backend-vocabulary) message.
- `relatedInformation` locations are recursively mapped back to `.rsx`
  coordinates (an entry with no source is dropped, not shown pointing at
  `src/.generated/…`), and their messages translated.
- A completion item is dropped outright if its `label`/`detail`/
  `documentation`/`filterText`/`insertText`/`labelDetails.detail`/
  `.description` names a backend marker (an extended list: `dioxus_*`,
  `PropsBuilder`, `VNode`, `RenderError`, `__template`, `_completions`, a
  `^__`-prefixed or `…Props`-suffixed name, `Usage in rsx`,
  `ChildComponent`, …), if it is a typed-builder internal
  (`build`/`into`/`try_into`) exposed by a `PropsBuilder` chain, or if its
  primary edit's range does not contain the request's cursor — checked
  **twice**: once before mapping (in generated coordinates, against the
  generated cursor the request was sent for) and again after mapping the
  edit back to `.rsx` coordinates (against the original `.rsx` cursor,
  issue #9 Gate 3 review, H1) — a mapping that reverse-maps to the wrong
  `.rsx` location entirely (a multi-source mapping always resolving to
  its first source: a closing tag's own generated occurrence resolving to
  its opening tag) passes the first check but fails the second.
  `additionalTextEdits` is stripped from every surviving item rather than
  mapped (an unmapped one would apply at the wrong, generated-coordinate
  position), and so is `documentation`.
- A JSX tag name (element or component, opening **or closing**) is never
  even sent to rust-analyzer for hover: it is classified locally, exactly
  like completion, and answered `null` directly
  (`crate::complete::is_tag_name_position`). Every hover that *is*
  forwarded has `::outou::__private::…`/`dioxus_*::…` path prefixes
  stripped, and any line that still names a backend marker — or names a
  real component's own `Props`/`PropsBuilder` type, anchored to the
  plan's actual `#[component]` functions rather than the bare
  `…Props`-suffix shape guess (issue #9 Gate 3 review, H3) — is dropped;
  a hover with nothing left, or with only markdown layout (an empty
  fence, a bare heading) surviving, is suppressed entirely rather than
  shown starting mid-sentence or empty-looking.

## Layout

| File | Contents |
|---|---|
| `src/main.rs` | Arg parsing (`--version`), resolves the rust-analyzer binary, fails fast if it cannot be found. |
| `src/server.rs` | The main loop: `initialize`, crate planning, rust-analyzer setup (capability forcing/validation), the client/rust-analyzer select loop. |
| `src/dispatch/mod.rs` | Shared `State`/`Pending`/`PendingKind` bookkeeping (cancel/epoch tracking) the three sibling modules below mutate. |
| `src/dispatch/requests.rs` | Requests the editor sends this server: position mapping, the local completion/hover split, `$/cancelRequest` rewriting. |
| `src/dispatch/responses.rs` | Everything coming back from rust-analyzer: its responses (mapped back), its own requests to the editor, its notifications. |
| `src/dispatch/notifications.rs` | `.rsx` `didOpen`/`didChange`/`didSave`/`didClose` handling, single-unit regeneration and crate-wide re-planning. |
| `src/complete.rs` | Outou-native tag-name/attribute-name completion (and the hover-side tag-name classifier), answered without ever asking rust-analyzer. |
| `src/ra.rs` | Spawns rust-analyzer and speaks JSON-RPC to it (reuses `lsp_server::Message`'s own framing for that side too); bounded waits for its `initialize`/`shutdown` responses. |
| `src/documents/mod.rs` | Re-exports; module doc for the split below. |
| `src/documents/units.rs` | The per-document (`RsxDocument`) and per-generated-unit (`GeneratedUnit`) types. |
| `src/documents/workspace.rs` | `Workspace`: the plan, the registry, loading, single-unit regeneration and crate-wide re-planning. |
| `src/plan.rs` | Thin wrapper over `outou_cli::build::plan` (degraded-mode detection, module-declaration-change detection, the planning overlay). |
| `src/mapping.rs` | Position/range translation between `.rsx` and generated Rust; refuses to translate a transformed (non-exact-length) mapping rather than guess. |
| `src/response.rs` | Rewrites and sanitizes hover/definition/completion response payloads through `mapping.rs`. |
| `src/diagnostics.rs` | Merges Outou syntax diagnostics with mapped rust-analyzer/flycheck diagnostics; never drops or downgrades an unmapped `ERROR`. |
| `src/translate.rs` | Backend-vocabulary translation table and marker list, shared by diagnostics, completion and hover (`docs/backend-leakage.md` rows 24-26). |
| `src/uri.rs` | Conversions between `outou_sourcemap::Uri` and `lsp_types::Uri`. |
| `tests/gate3.rs` | End-to-end Gate 3 test, driving `spikes/rust-analyzer/client/outou-lsp-client.mjs` against a fresh temporary copy of `examples/phase0-app` per probe (never the repository tree). Ignored by default; run explicitly: `cargo test -p outou-lsp --test gate3 -- --ignored --nocapture`. |

## Known limitations

See [`docs/gate3-results.md`](../../docs/gate3-results.md)'s own section
for the full list with evidence. In short: `$/progress` (and so the
readiness signal it carries) only reaches editors that advertise
`window.workDoneProgress` support, semantic diagnostics need a save (a
rust-analyzer limitation, not this server's), a transformed
(non-verbatim) source mapping returns `null` rather than a
guess, and a handful of SKIP items recorded as `TODO(phase0)` at their
own call sites (Windows/UNC file URIs, a multi-source diagnostic always
using the first source, `completionItem/resolve` not being advertised).
