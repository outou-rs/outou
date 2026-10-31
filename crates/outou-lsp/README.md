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
| `window/workDoneProgress/create` | Forwarded to the editor (under a fresh id) only if the editor advertised `window.workDoneProgress`, with rust-analyzer's token replaced by an opaque `outou/progress/<n>` token (remembered both ways); otherwise answered locally with `null` and that token's progress is dropped. |
| `$/progress` (a notification, not a request) | A token rust-analyzer created is renamed to its `outou/progress/<n>` token (forgotten at `end`); its title (rewritten to `Outou: indexing`, `Outou: checking`, ...) and message (reduced to its `done/total` counter) are rewritten, for `begin` / `report` / `end` payloads only. If the editor did not advertise `window.workDoneProgress`, that progress is dropped. A `$/progress` for any other token (one the editor allocated, e.g. a `workDoneToken`) is forwarded untouched. |
| `window/workDoneProgress/cancel` (editor to rust-analyzer) | The `outou/progress/<n>` token is mapped back to rust-analyzer's own; an unknown token is forwarded as is. |
| `partialResultToken` in a forwarded request | Removed before the request reaches rust-analyzer (partial results are not reverse-mapped); rust-analyzer answers in full. `workDoneToken` is kept. |
| `experimental/serverStatus` (a notification) | Never forwarded. `outou-lsp` declares `experimental.serverStatusNotification` to rust-analyzer in its own `initialize`; a `health` of `warning` or `error` becomes one Outou-worded `window/showMessage` per state (repeats are silent, only an explicit `ok` re-arms, a missing or unknown `health` is ignored), with the upstream message appended only if it passes the backend-vocabulary scan (case-insensitive, `-` as `_`, punctuation ignored). |
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

### Formatting

`textDocument/formatting` is answered locally by `outou-fmt`, the same pipeline as `outou fmt`. The request's `options` (`tabSize`, `insertSpaces`, ...) are deliberately ignored: `rustfmt.toml` is the single source of truth for layout, so an editor setting can never silently override the project's configuration, and the result is identical to what `outou fmt` produces (a test pins that the output does not depend on `options`). The request runs on the server loop but within a 10 second budget covering every `rustfmt` process the file needs; past it the running `rustfmt` is killed and the request fails with a `RequestFailed` error, "Outou formatting timed out". Other failures (a syntax error, a missing `rustfmt`) still answer `null` and log to stderr.

### Semantic tokens, rename, references

`textDocument/semanticTokens/full` merges two sources: rust-analyzer's
own tokens for the generated file (decoded, mapped back through the
source map, split per line where a mapped range crosses a line break) and
Outou's own AST-derived tokens (component name, HTML element name — both
opening and closing tag — attribute name, event attribute, JSX text),
which always win an overlapping position. The advertised legend is
rust-analyzer's own (read from its `initialize` response) when it is
attached, or the standard LSP legend in degraded mode / when no
rust-analyzer is attached at all — in either case, degraded-mode requests
still get an answer built from Outou's own tokens alone, never `null`.
See `docs/adr/0012-semantic-tokens-legend-and-overlay.md` and
`src/semantic_tokens/`.

`textDocument/rename`/`prepareRename` and `textDocument/references` are
forwarded at the mapped generated position like hover/definition
(including from a cursor on a *closing* tag name, which needs no special
case: it maps through the same identifier mapping as the opening tag).
Rename's `WorkspaceEdit` (`changes` and `documentChanges`, the latter
versioned against the `.rsx` document, never the generated file's) is
translated back through the same many-to-many source map that gives an
element's identifier both its opening and closing tag as sources — so
renaming a component updates both tags with no separate synthesis step —
and the whole rename is refused, as an Outou-vocabulary error, if any part
of it would otherwise have to be silently dropped. References uses the
same translation but drops an unmappable location instead of refusing,
since it is read-only. See
`docs/adr/0013-rename-translation-and-refusal.md`, `src/rename.rs` and
`src/references.rs`.

## Layout

| File | Contents |
|---|---|
| `src/main.rs` | Arg parsing (`--version`), resolves the rust-analyzer binary, fails fast if it cannot be found. |
| `src/server.rs` | The main loop: `initialize`, crate planning, rust-analyzer setup (capability forcing/validation), the client/rust-analyzer select loop. |
| `src/dispatch/mod.rs` | Shared `State`/`Pending`/`PendingKind` bookkeeping (cancel/epoch tracking) the three sibling modules below mutate. |
| `src/dispatch/requests.rs` | Requests the editor sends this server: position mapping, the local completion/hover split, `$/cancelRequest` rewriting. |
| `src/dispatch/responses.rs` | Everything coming back from rust-analyzer: its responses (mapped back), its own requests to the editor, its notifications. |
| `src/dispatch/notifications.rs` | `.rsx` `didOpen`/`didChange`/`didSave`/`didClose` handling, single-unit regeneration and crate-wide re-planning. |
| `src/dispatch/watched_files.rs` | `workspace/didChangeWatchedFiles`: registers `**/*.rsx` and `**/Cargo.toml` watchers when the client supports dynamic registration; closed `.rsx` files changed, created or deleted outside the editor are re-read (open buffers win), a manifest change or a created/deleted file re-plans. |
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
| `src/uri.rs` | Conversions between `outou_sourcemap::Uri` and `lsp_types::Uri`, and `normalize_file_uri` (the one spelling every internal key uses). |
| `src/editor_uris.rs` | Normalizes `.rsx` URIs from the editor on the way in (tracking the set of spellings a document is open under) and restores the editor's spelling on the way out (diagnostics, definition, references, rename); the identity when the editor already uses real paths. |
| `src/semantic_tokens/mod.rs` | `textDocument/semanticTokens/full` orchestration: decode, map, merge, re-encode. |
| `src/semantic_tokens/legend.rs` | The legend advertised/translated against (rust-analyzer's own, or the standard LSP list in degraded mode). |
| `src/semantic_tokens/token.rs` | Pure delta-encode/decode, per-line splitting and overlay-merge transforms, independently unit-tested. |
| `src/semantic_tokens/outou_tokens.rs` | The pure AST walk producing Outou's own component/element/attribute/event/text tokens. |
| `src/rename.rs` | `textDocument/rename`/`prepareRename`: `WorkspaceEdit` translation, refusal policy. |
| `src/references.rs` | `textDocument/references`: `Location[]` translation (drops rather than refuses on an unmappable entry). |
| `tests/watched_files.rs` | Drives the real binary (no rust-analyzer) through registration and external create/change/delete of closed `.rsx` files. |
| `tests/gate3.rs` | End-to-end Gate 3 test, driving `spikes/rust-analyzer/client/outou-lsp-client.mjs` against a fresh temporary copy of `examples/phase0-app` per probe (never the repository tree). Ignored by default; run explicitly: `cargo test -p outou-lsp --test gate3 -- --ignored --nocapture`. |

## Known limitations

See [`docs/gate3-results.md`](../../docs/gate3-results.md)'s own section
for the full hover/completion/definition/diagnostics list with evidence.
In short: rust-analyzer's `$/progress` (and so the readiness signal it carries) only
reaches editors that advertise `window.workDoneProgress` support,
semantic diagnostics need a save (a rust-analyzer limitation, not this
server's), a transformed (non-verbatim) source mapping returns `null`
rather than a guess. Two items from that review are deliberate: a
diagnostic whose generated span has several `.rsx` sources is shown at the
first source only (ADR 0007's reverse-mapping rule asks for one location),
and the Windows/UNC file URI limitation is `TODO(alpha) (#31)` at
`crate::uri::to_path`.

Semantic tokens/rename/references (issue #14 review):

- **A keyword-named prop (`type`, lowered to the raw identifier `r#type`)
  has no rename/references answer at all** — refused (rename) or dropped
  (references) exactly like synthesized code, since its mapping is
  length-mismatched (`docs/backend-leakage.md` row 29, ADR 0013). No
  text-only fallback is implemented for this narrower shape.
  `TODO(alpha) (#31)`.
- **`prepareRename` on a closing tag's own name answers with the closing
  tag's range.** The request's `.rsx` cursor picks, among the sources of the
  generated name (`crate::rename::translate_prepare_rename_response`), the
  one that contains it; the rename itself still edits both tags.
- **`.rsx` URIs are normalized on disk, not per RFC 3986.** Every
  internal key is a lexical string, so the editor's URI is canonicalized at
  the boundary (`crate::uri::normalize_file_uri`: percent-decoding,
  symbolic links, `..`, the on-disk spelling on a case-insensitive file
  system) and the editor's own spelling is restored in
  `publishDiagnostics`, definition, references and rename results
  (`crate::editor_uris`). This was reproduced and fixed against a real
  editor that opened the workspace through a symbolic link (and on macOS
  `/tmp` and `/var` are such links): completion, hover and definition
  answered `null`, `didSave` never reached rust-analyzer, and its
  diagnostics landed on a different resource than the open tab. Still
  open, `TODO(alpha) (#31)`: an equivalent `file://localhost/` authority,
  Windows drive letters and UNC paths, and non-`file:` schemes are passed
  through unchanged.
  The same file may be open under several spellings (real path and link):
  diagnostics go to every one of them, a response uses the spelling of
  its request, and the document is reloaded from disk only when the last
  spelling closes. When the spelling in use changes (a file that had
  diagnostics at its real path is then opened through the link, or a
  spelling closes), the spelling that had diagnostics gets one empty
  `publishDiagnostics`, so no stale error is left behind.
- **rust-analyzer's standard error is dropped by default.** It carries
  rust-analyzer's own log lines and the full text of every `didSave`, which
  is the generated Rust and its `.generated` path, and the editor would show
  all of it. Start `outou-lsp` with `OUTOU_LSP_RA_LOG=1` (any value other
  than empty or `0`) to pass it through to this server's stderr unchanged;
  that is a developer opt-in and nothing is filtered or rewritten. If
  rust-analyzer exits, the server's own stderr says so and names the
  variable. `outou-lsp`'s own messages on stderr are unaffected. The VS Code
  extension shows the server's stderr at `[info]`.
- **PascalCase components do not trigger `non_snake_case`.** Codegen writes
  `#[allow(non_snake_case)] ` before a PascalCase `#[component]` function,
  because rust-analyzer's own check sees the item before the macro expands.
- **A `.rsx` file the plan does not reach** (no `mod` declaration leads to
  it) is checked for syntax only and never sent to rust-analyzer; the
  server says so once per file on stderr.
- **`textDocument/semanticTokens/full` re-parses the whole `.rsx` file
  on every request** (both for Outou's own overlay tokens and, when a
  live rust-analyzer is attached, to build the source map lookup),
  matching this crate's existing style elsewhere (`crate::complete`,
  `crate::response`'s `known_component_names`) rather than caching a
  parse across requests.
- **No `textDocument/semanticTokens/range` or `/full/delta`** — only the
  whole-document, non-incremental form is advertised or implemented.
