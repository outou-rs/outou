# Gate 3 results: the integrated language server

Outcome of Week 5's integration (`crates/outou-lsp`, issue #9): connecting the
real parser and codegen (`outou_syntax`, `outou_codegen`, `outou_backend_dioxus`,
via `outou_cli::build`) to the Week 1 pipeline (`spikes/rust-analyzer/`,
[`docs/ra-spike-results.md`](ra-spike-results.md)). Same format as that
document.

**This revision corrects the previous version of this document**, which
declared **Gate 3: PASS** without having been reviewed. A review (issue #9
fix list) found the original evidence collection ran every probe directly
against `examples/phase0-app` in the repository tree using fixed sleeps
instead of event-timed waits, asserted presence rather than correctness
(no range checks, no leakage checks), and — more seriously — missed nine
real defects, three of them severe enough to block sign-off on their own:
`didSave`/startup could replace the crate's real `[[bin]]` target with
Recovery-mode placeholder text (M1); a `mod` typed into an unsaved buffer
never entered the module graph (M2); and completion at a tag-name or
attribute-name position returned generic Rust-identifier noise
(`__TEMPLATE_ROOTS`, `PropsBuilder` internals, raw `dioxus_*` module
names) rather than anything Outou-shaped (M3). All of that document's own
claims below have been re-verified from a **fresh, live re-run** after
fixing every MUST item the review found; see `docs/backend-leakage.md`
rows 25-26 for the two defects that document had not itself been
tracking.

## Environment

| Item | Value |
|---|---|
| Date | 2026-09-06 (re-verification) |
| rust-analyzer version | 1.98.1 (48a229ce 2026-09-01) |
| rustc / cargo version | rustc 1.98.1 (48a229cea 2026-09-01) / cargo 1.98.1 (797e8a9bc 2026-08-05) |
| Node version | v24.14.0 |
| OS | macOS 26.6.2 (build 25G83), Darwin 25.6.0, arm64 |

`outou-lsp` was built with `cargo build -p outou-lsp` (debug profile) and
spawned rust-analyzer via `OUTOU_RUST_ANALYZER`/`PATH` exactly as it does in
normal use — no special test-only code path. The target program is Gate 3's
fixed example, `examples/phase0-app` (`src/main.rsx`, `src/components.rsx`),
unchanged from the previous revision of this document.

**Every probe now runs against a fresh temporary copy of `examples/phase0-app`
(`crates/outou-lsp/tests/gate3.rs::fresh_copy`), never the repository tree
itself** — several probes deliberately save broken content or pre-corrupt the
source before startup, and the previous revision's own probes ran directly
against the tracked example, which is why one of the defects below
(M1/CRITICAL-1) was reachable there at all. `Cargo.toml`'s `outou` path
dependency is rewritten to an absolute path per copy; a `CARGO_TARGET_DIR`
shared across every probe's copy (outside the repository tree) keeps
`cargo check` (rust-analyzer's flycheck) from recompiling `dioxus` and the
rest of the dependency graph from scratch on every single probe.

All raw command output is under `spikes/rust-analyzer/results/gate3-*.json.gz`
(replacing the previous revision's files; `gate3-completion-component.json.gz`
and `gate3-completion-prop.json.gz` are removed — the probes they were named
for no longer exist, superseded by M3's tag/attribute-name probes below),
produced by `crates/outou-lsp/tests/gate3.rs` (run explicitly: `cargo test -p
outou-lsp --test gate3 -- --ignored --nocapture`), which drives
`spikes/rust-analyzer/client/outou-lsp-client.mjs` — a sibling of the Week 1
spike's `ra-client.mjs`, talking to `outou-lsp` itself rather than to
rust-analyzer directly (see that script's own doc comment for why it is a
separate file). The client now waits for a *useful* answer (bounded retry,
not a fixed sleep) rather than accepting the first non-`null` one — the
previous revision's fixed 12s settle window plus a bare `!== null` check
occasionally accepted a still-indexing placeholder (`{unknown}`, an empty
`[]`) as if it were the real answer; this revision's `latencyMs` are real
elapsed times to a *correct* answer, not sleep durations. Run once, per the
review's instruction (`cargo test -p outou-lsp --test gate3 -- --ignored
--nocapture`, `finished in 55.11s` for all 17 probes with a warm shared
target directory).

## Gate 3 criteria

| Criterion (`docs/phase0.md`, `docs/phase0/issues/09-integrated-lsp.md`) | Works? | Evidence |
|---|---|---|
| `didOpen`/`didChange` regenerates only the edited unit (recovery mode), overlaid onto rust-analyzer | yes | `crates/outou-lsp/src/documents.rs`'s `regenerating_one_unit_does_not_touch_another` unit test asserts `components.rs`'s generated text is byte-identical after editing `main.rsx`. A crate-wide re-plan happens only when the edited file's own declared-module *shape* changes (`Workspace::module_shape_changed`, now comparing full module descriptors — name, `#[path]`, inline/file form — not just names, M2), and now sees every currently open buffer's own text, not disk (`Workspace::build_overlay`; unit test `replan_sees_a_module_declared_only_in_the_open_buffer`, and `outou-modules`' own `resolve_with_overlay_sees_a_mod_declaration_only_present_in_the_overlay`). |
| The generated `[[bin]]`/`[lib]` target on disk is never Recovery-mode placeholder text | yes (fixed; was **no**) | **M1/CRITICAL-1, confirmed and fixed.** Before the fix, saving a half-typed `<UserCard us` replaced `crate-root.rs` — the crate's real `[[bin]]` target — with a placeholder call, and the sibling `.rs.map.json` was never rewritten at all. `gate3-save-with-syntax-error.json.gz`: saving `<div cl` (a syntax error) now leaves `src/.generated/crate-root.rs` **byte-identical** before and after (checked by the Rust test against the temp copy directly) and publishes the Outou syntax diagnostic (`severity 1`, `source: "outou"`) instead of writing anything. `gate3-startup-broken-source.json.gz`: starting `outou-lsp` against an already-broken `main.rsx` with no `.generated/` directory creates **no file at all** (checked directly) and still publishes the syntax diagnostic. Both startup and save now go through the same transactional Strict path `outou build` itself uses (`outou_cli::build::emit::emit`); Recovery mode is used only for the in-memory editor overlay. |
| A `mod` declared only in an unsaved buffer enters the plan | yes (fixed; was **no**) | **M2/HIGH-2, confirmed and fixed.** Before the fix, planning always re-read every file from disk, so typing `mod newmod;` (unsaved) and saving wrote a generated root with the new `mod` line but no matching `#[path]` — non-compiling Rust. Fixed via `outou_modules::resolve_with_overlay`/`outou_cli::build::plan::plan_with_overlay`, threaded through `crate::plan::resolve_with_overlay` and `Workspace::build_overlay` (every currently open document's text, plus the just-edited one). Verified at the `Workspace` API level (`documents::tests::replan_sees_a_module_declared_only_in_the_open_buffer`) rather than by a new live gate3 probe — the scenario needs an extra fixture file (`src/newmod.rsx`) and is exercised more deterministically this way than through another rust-analyzer spawn; recorded here as a scoped substitution, not a skipped test. |
| Hover on `user` reports its real type through the real parser | yes | `gate3-hover-user.json.gz`: `let user: Option<User>` at `main.rsx:20:8-12` (range asserted, not just presence), `latencyMs.hover` 5702 ms (a real wait for full indexing — see the latency section for why this is now slower, not worse, than the previous revision's number). |
| Hover position mapping is correct across non-ASCII text | yes (new criterion, M7) | **M7/HIGH-6.** `gate3-hover-nonascii.json.gz`: with `let _u = "あああ"; let user = load_user();` inserted, hover on `user` still resolves `let user: Option<User>` at the *correct* column (24-28, shifted exactly by the inserted prefix's own UTF-16 length) — proof end to end that this server's forced `general.positionEncodings: ["utf-16"]` (and its refusal to use rust-analyzer if it answers otherwise) actually holds, not just that this server's own math assumes it. Before the fix, driving the same rust-analyzer directly with `["utf-8", "utf-16"]` negotiated `utf-8` and produced a confidently *wrong* (not null) answer for exactly this shape of input. |
| Hovering an HTML element never leaks backend vocabulary | yes (fixed; was **no**) | **M4/HIGH-9(d).** `gate3-hover-element-tag.json.gz`: hovering `<main class="app">` returns `hover: null` — before the fix this returned `dioxus_html::elements\n\npub mod main\n\n\nBuild a <main> element.\n\nUsage in rsx# use dioxus::prelude::*; …` verbatim, the exact string `AGENTS.md` forbids. `crate::response::sanitize_hover` strips `::outou::__private::`/`dioxus_*::` path prefixes and drops any line still naming a marker; here nothing of the hover survived, so it is suppressed entirely rather than shown starting mid-sentence — an honest `null`, not a leak. |
| Definition on `load_user()` resolves within `main.rsx` | yes | `gate3-definition-load-user.json.gz`: resolves to `main.rsx:42:3-12` (`fn load_user`, range asserted). |
| Definition across `.rsx` files (`UserCard` -> `components.rsx`) | yes | `gate3-definition-user-card.json.gz`: resolves to `components.rsx:8:7-15` (`pub fn UserCard`, range asserted). |
| Completion keeps working under incomplete input: member (`user.`) | yes | `gate3-completion-member.json.gz`: 122 items including `unwrap`, `expect`, `unwrap_or*`, `is_some_and`; native diagnostics correctly flag the overlay as invalid Rust on its own (`Syntax Error: expected field name or number`) while completion still answers; `assert_no_leakage` passes on the full result. |
| Completion at a JSX tag-name position is Outou-shaped, not generic Rust identifiers | yes (fixed; was **no**) | **M3/HIGH-7/HIGH-10, the most substantial fix in this revision.** The previous document's own "PASS" for `completion-component` was hovering over the wrong fact: `<UserC` reached rust-analyzer as an *ordinary struct-literal identifier* position, returning 155 items (`__TEMPLATE_ROOTS`, `GreetingPropsBuilder_Error_Missing_required_field_name`, `App_completions::`, …) with `UserCard` merely present among them — not a props/tag-name-shaped answer at all. Fixed by *not forwarding this position to rust-analyzer*: `crates/outou-lsp/src/complete.rs` classifies the `.rsx` cursor against Outou's own recovery parse tree and answers locally. `gate3-completion-tag-component.json.gz`: exactly one item, `UserCard`, `textEdit.range` `{25,9}..{25,14}` (the partial identifier's own span — asserted exactly, not just presence); `gate3-completion-tag-element.json.gz`: exactly one item, `div`, range `{25,9}..{25,11}`. Neither result is forwarded to rust-analyzer at all (`latencyMs.completion`: 3ms, both — this is in-process, not a round trip). |
| Completion at a JSX attribute-name position on a known component answers with real prop names | yes (fixed; was previously documented as impossible) | **M3, and a correction to a specific false claim.** The previous revision stated: "Outou's recovery parser turns an unclosed tag into a placeholder call rather than a partial struct literal, so there is no `UserCard { us` for rust-analyzer to complete props against." **That claim was false** — recovery does produce `UserCard { us: true, }`, and rust-analyzer's builder-method completion on it did work, live-verified during the review — but the *payload* was `build`/`into(as Into)`/`try_into(as TryInto)` (typed-builder internals) and a raw `UserCardPropsBuilder<...>` detail string, not a clean prop-name answer. Fixed the right way regardless: `<UserCard us` is now classified as an attribute-name position and answered **locally**, never forwarded. `gate3-completion-prop-name.json.gz`: exactly one item, `user`, range `{28,26}..{28,28}` (exact). |
| Completion at an HTML attribute-value position never corrupts the buffer | yes (fixed; was **no**, and was the worst defect found) | **M3/M4/HIGH-8.** `<div class=` previously returned `dioxus_core::`/`dioxus_elements::`/`dioxus_signals::` as completion *labels*, with a **zero-width edit at the wrong column** (character 8, ten columns from the actual cursor at 19) — accepting any item would have corrupted the buffer. `gate3-completion-attr-value.json.gz`: `completion: null`. `crate::response::map_completion_response` now drops any item whose mapped primary edit range does not contain the request's cursor, generically fixing this class of defect regardless of position; here nothing survived the filter. |
| Completion at a prop *value* position (`user={us}`) | yes | `gate3-completion-prop-value.json.gz` (renamed from the previous revision's `completion-prop`, same shape): 129 items including `user`, `self::`, `crate::`, `components::User`, `tags` — ordinary Rust completion in scope, `assert_no_leakage` passes. |
| Rust type errors reported at the right position in the `.rsx` file | yes, via `didSave` + flycheck | `gate3-diagnostic-type-error.json.gz`: `let user: u32 = load_user();` + `didSave` produces `mismatched types` (severity 1) plus a cascading `E0599` (`is_some`), both at the right positions, `latencyMs.didSaveToDiagnostics` 5173 ms with a warm shared build cache (see latency section: the previous revision's fixed `6003 ms` figure was a sleep duration, not a measurement). |
| A missing required prop (a compile error synthesized entirely inside generated code, no direct `.rsx` span) is never dropped or downgraded | yes (fixed; was **no**, silently downgraded) | **M5, confirmed and fixed — more serious than the review's own description.** Before the fix: a hard `E0061` compile error (`argument #1 of type UserCardPropsBuilder_Error_Missing_required_field_user is missing`) had no direct source span (it points inside the synthesized `rsx!` expansion), so the mapper reported it as `severity: 4` (**HINT**) — a compile error shown to the user as a barely-visible hint. `gate3-diagnostic-missing-prop.json.gz`: `<UserCard />` (missing `user`) now publishes `severity: 1` (ERROR), message `this component is missing a required property` (cleanly translated, no `PropsBuilder` text), at `main.rsx:28:16-28` — `crate::mapping::nearest_source_position` supplies an approximate but present position for any `ERROR`-severity diagnostic with no direct mapping, rather than dropping it. `relatedInformation` is correctly reverse-mapped to `components.rsx:7:0-12`. |
| Outou syntax errors reported by Outou itself, at the right position | yes | `gate3-diagnostic-syntax-error.json.gz`: truncating `<h1>Hello {name}</h1>` to `<div cl` produces `unexpected '}' inside tag '<div>', expected an attribute or '>'` (`source: "outou"`), event-timed at 102 ms (`didChangeToDiagnostics`) — no `cargo check` round trip needed. |
| Completion/definition keep working elsewhere in the file after an unrelated syntax error | yes | Same run: `definitionDespiteSyntaxError` still resolves `load_user()` to `main.rsx:42:3-12`, unaffected by the broken `<div cl` earlier in the file (a different function, `Greeting`). |
| A stale rust-analyzer diagnostic does not survive an edit that fixes it | yes (fixed; was **no**) | **M6/HIGH-4, confirmed and fixed.** Before the fix, reverting a type-error buffer via `didChange` (no save) kept republishing the same stale `mismatched types` diagnostics for 3+ seconds, clearing only on an unrelated later edit — `Workspace::regenerate` never cleared the cached rust-analyzer diagnostics it re-merges on every publish. `gate3-stale-diagnostics-cleared.json.gz`: after the type error is confirmed published (`typeErrorIntroduced`, `E0308`, 4266 ms) and the buffer is reverted, `diagnosticsAfterRevert` is an **empty array** — zero stale diagnostics, immediately, not eventually. |
| Every payload sent to the editor is free of backend vocabulary | yes (fixed; was **no**, in several places the previous revision's own probes never looked at) | **M4/HIGH-9, the second-most substantial fix.** `crates/outou-lsp/tests/gate3.rs::assert_no_leakage` now recursively scans **every** field of **every** probe's full JSON result (not just the field the probe happens to be about) for `PropsBuilder`, `dioxus`, `.generated`, `VNode`, `RenderError`, `__private`, `__template`, `rsx!` — this is what would have caught the hover and completion leaks above, and did not exist before. See `docs/backend-leakage.md` rows 25-26 for the full list of what was leaking and how each channel (diagnostic `data`, `relatedInformation`, completion item fields, hover contents) is now sanitized. |

**Gate 3: PASS**, now honestly. Every criterion in
`docs/phase0/issues/09-integrated-lsp.md` holds through the real parser and
codegen against `examples/phase0-app`, re-verified live after fixing every
MUST item the review found (M1-M9) and the SHOULD items in scope for this
pass (S1, S2, S3, S5, S7). See "Not applied as reviewed" below for what was
intentionally left for a later pass, and why.

## Measured latency

All values are `latencyMs` fields from `outou-lsp-client.mjs`'s JSON output
(`spikes/rust-analyzer/results/gate3-*.json.gz`, the artifacts this table is
drawn from), from **one** full run of `cargo test -p outou-lsp --test gate3 --
--ignored --nocapture` (finished in 55.11s total for all 17 probes) — the
review's own MEDIUM-12 finding was that the previous revision's "two runs"
table did not actually correspond to two runs of the retained artifacts, and
its `didSaveToDiagnostics`/`didChangeToDiagnostics` figures were `setTimeout`
sleep durations, not measurements. This revision reports one honest run
rather than a fabricated second one; S8 (re-measuring proxy overhead against
a truly warm, long-lived session) remains open, see below. (Two earlier runs
during this pass surfaced a genuine flake in the `stale-diagnostics-cleared`
probe itself — rust-analyzer's flycheck can publish more than once for a
single save, and reverting right after the *first* matching diagnostic could
race a second, late publish for the pre-revert content; fixed by waiting for
diagnostics to go quiet (`waitForQuiescence`) before reverting. This is a
test-harness fix, not a change to `crate::documents::Workspace::regenerate`'s
own cache-clearing, which was correct in every run.)

| Request | This run (ms) | What changed vs. the previous revision |
|---|---|---|
| `initialize` (client <-> outou-lsp) | 38-911 (per-probe; the high end only on the first probe of the run, cold) | Same shape as before: this is `outou-lsp`'s own setup, not full rust-analyzer indexing. |
| hover (`user`) | 5702 | Was `560` — but that number came from a client that accepted the *first* non-`null` answer, which could be (and, once actually checked, was: see `hover-user`/`hover-nonascii` above) a still-indexing placeholder. This run's client retries until the answer is actually correct (`isUsefulHover`); the honest number is higher, not lower, once indexing time is not silently absorbed into "it answered `null`-but-not-actually-null so we stopped." |
| hover (non-ASCII prefix) | 4380 | New probe (M7). |
| hover (element tag, sanitized to `null`) | 1 | In-process; no rust-analyzer round trip needed once `null` is the answer. |
| definition (`load_user`, same file) | 823 | Was `529-540`, same caveat as hover above (this run's number is a real wait for a *correct*, non-empty answer). |
| definition (`UserCard`, cross-file) | 4306 | Same caveat. |
| completion (member, `user.`) | 4046 | Was `595-622`; same caveat. |
| completion (tag, component) | 3 | New shape (M3): answered locally, never forwarded to rust-analyzer. Was `593-644` when this used to be a full rust-analyzer round trip for the *wrong* answer. |
| completion (tag, element) | 3 | New probe (M3), local. |
| completion (attribute name) | 3 | New probe (M3), local. |
| completion (attribute value, sanitized to empty) | 2 | New probe (M4/HIGH-8); in-process filtering, no meaningful round trip left once every item is dropped. |
| completion (prop value, `user={us}`) | 4166 | Renamed from "prop value"; was `631-642`, same indexing-wait caveat. |
| `didSave` -> mismatched-types diagnostic published | 5173 | Was `6003-6005` — a sleep duration, not a measurement (MEDIUM-12); this is now the real elapsed time to the *first matching* diagnostic, with a warm shared `CARGO_TARGET_DIR`. |
| `didChange` -> Outou syntax diagnostic published | 102 | Was `1502-1504`, likewise a sleep duration before; this is the real elapsed time. |
| definition after an unrelated syntax error | 479 | Same indexing-wait caveat as the other definition rows; low here because this probe's crate was already warm by the time it ran. |
| `didSave` -> missing-required-prop diagnostic (M5) | 5780 | New probe. |
| type error introduced -> published, before the M6 revert (M6) | 4266 | New probe. |
| `didSave` -> Outou diagnostic, syntax error (M1 save probe) | 102 | New probe; no flycheck involved (a syntax error is Outou's own, published on `didChange` already). |
| startup -> Outou diagnostic, broken source at launch (M1) | 102 | New probe. |

Compared against `docs/phase0.md`'s provisional budget:

- **"incremental `.rsx` -> generated Rust: perceived as instantaneous"** —
  unchanged from the previous revision: `regenerating_one_unit_is_fast`
  asserts well under 50 ms (in practice a small fraction of a millisecond).
- **"extra proxy overhead on completion: not perceptible"** — for the three
  positions M3 now answers locally (tag name, tag name on an element,
  attribute name), the overhead is a few milliseconds of in-process
  classification, not a proxied round trip at all — strictly better than
  "not perceptible": there is no rust-analyzer round trip to perceive.
  For positions still forwarded (member, prop value), this pass's numbers
  (4-4.8s) are **not** a regression in the underlying proxy cost; they
  reflect this run's client correctly waiting for rust-analyzer to finish
  indexing a cold crate rather than accepting its first (wrong) answer.
  **S8 (re-measuring warm, steady-state overhead against a long-lived
  session) is still open** — recorded as a known gap below, not silently
  dropped.
- **"a single-file edit never regenerates the whole crate"** — unchanged:
  verified at the unit level and exercised by every probe above.

## Known blockers and limitations

- **No `$/progress` forwarding (S6, open).** `outou-lsp` still does not relay
  rust-analyzer's own indexing progress to the editor. This pass's test
  client compensates with bounded request retries (`requestUntilReady`)
  instead of a fixed sleep, which is more honest about latency but does not
  fix the underlying gap for a real editor, which still has no readiness
  signal beyond the answers themselves.
- **Semantic type errors require a save, not just a keystroke.** Unchanged
  from the previous revision — a rust-analyzer limitation (native
  diagnostics never report semantic errors, per the Week 1 spike), not this
  server's own.
- **A transformed (non-verbatim) source mapping returns `null`, not a guess
  (S1, fixed).** `key={tag.clone()}` lowers to `key: "{tag.clone()}"`
  (quotes added, different length); before this pass, hovering `tag` inside
  that attribute value returned a proportionally-scaled but meaningless
  position (`extern crate std`). `crate::mapping` now refuses to translate
  any mapping whose source and generated spans differ in length, in both
  directions; verified by unit test
  (`a_length_mismatched_mapping_maps_{forward_to_nothing,backward_to_unmapped}`),
  not a new gate3 probe (the reproduction needs a hand-built mapping to
  force the exact byte-length mismatch; doing that live would require a
  scripted codegen edge case, not a probe against the fixed target
  program).
- **`$/cancelRequest` correctness (S2, fixed for the specific defect found;
  RA-originated request forwarding is new but not independently
  gate3-tested).** Before this pass, a cancel used the editor's own request
  id verbatim, which — whenever the editor's and this server's independent
  id counters diverged — cancelled nothing (the full result was still
  delivered) or the wrong rust-analyzer request. Fixed by rewriting the id
  through this server's pending-request map
  (`crate::dispatch::resolve_cancel_target`, unit-tested). Also new in this
  pass: `workspace/configuration` answers an array of `null`s instead of a
  bare `null`, and `client/registerCapability`/
  `window/workDoneProgress/create` are forwarded to the editor (under a
  freshly allocated id) when it advertised support, instead of always being
  swallowed locally — unit-tested (`dispatch_client_response_*`,
  `handle_ra_response_*`), not exercised by a live gate3 probe, since
  reaching this path needs a client that both advertises the relevant
  capability and answers the forwarded request, which
  `outou-lsp-client.mjs`'s minimal capabilities do not do.
- **Monotonic `rust-analyzer` document versions and stale-overlay cleanup
  (S3, fixed).** A re-plan used to hard-reset every unit's version to `1`
  (versions could go backwards after enough edits) and never told
  rust-analyzer a removed unit's overlay was gone. Both are fixed
  (`replan_and_resync` now tracks each unit's previous version and sends
  `didClose` for anything no longer in the plan) and covered by unit tests
  for the epoch/cancellation half of the same fix
  (`handle_ra_response_cancels_a_response_from_a_stale_epoch`); the
  version-monotonicity and `didClose` behavior itself is not independently
  gate3-tested (it needs a multi-edit sequence long enough to distinguish
  "reset to 1" from "kept counting up", which the fixed target program's
  probes do not naturally exercise).
- **Startup/shutdown deadlines (S5, fixed).** `RaClient::wait_for_response`
  had no timeout at all; a hung (not exited) rust-analyzer wedged this
  server forever during `initialize` or `shutdown`. Now bounded
  (`HANDSHAKE_TIMEOUT`, 30s). Every pending client request is also now
  failed with an ordinary error the moment rust-analyzer's message channel
  closes, instead of hanging the editor forever
  (`fail_all_pending`, unit-tested).
- **README accuracy (S7, fixed).** `crates/outou-lsp/README.md` no longer
  claims "everything else is forwarded to rust-analyzer transparently" (it
  was not true even before this pass — every rust-analyzer -> client
  request was answered locally with `null`); it now documents the
  two-rust-analyzer process model explicitly and a forwarded/dropped method
  matrix.
- **Backend-vocabulary translation remains a small, extensible table**
  (`crates/outou-lsp/src/translate.rs`), now shared by diagnostics,
  completion and hover sanitization (`docs/backend-leakage.md` rows 24-26).
- **Latency numbers here are a single cold-then-warming run**, not a
  long-lived warm editor session — see S8 above, still open.

## Not applied as reviewed

Per the fix list's own SKIP category and this pass's own scoping decisions,
recorded rather than silently dropped:

- **S4** (root fallback to `rootUri`/`rootPath`; deferring rust-analyzer
  resolution until a planned workspace needs it) — `TODO(phase0)` at
  `crate::server::resolve_root`. Not needed for Gate 3's own criteria; every
  probe here uses `workspaceFolders`, which already works.
- **S6** ($/progress forwarding) — `TODO(phase0)` at
  `crate::dispatch::handle_ra_notification`. See "Known blockers" above.
- **S8** (re-measuring proxy overhead against a genuinely warm session) —
  not done this pass; recorded as open in "Measured latency" above.
- **LOW-16** (Windows/UNC file URIs) — `TODO(phase0)` at `crate::uri::to_path`.
  Not reachable on Phase 0's macOS/Linux platforms.
- **MEDIUM-15's SKIP half** (a diagnostic with several source spans always
  uses only the first) — `TODO(phase0)` at `crate::mapping`. ADR 0007
  permits this; no observed case in the target program needs more.
- **HIGH-8's SKIP half** (`additionalTextEdits`/`completionItem/resolve`) —
  `TODO(phase0)` at `crate::response::map_completion_response`. This
  server's `completionProvider` does not advertise `resolveProvider`, so a
  conformant client never calls it; an auto-import edit is lost, not
  corrupting (the corrupting half of HIGH-8, the primary edit's wrong
  range, is fixed generically as part of M4).
- **Recovery quality for `<div class=`** (the parser/codegen swallowing a
  following sibling element into the broken tag's own attributes) — noted
  at `crate::complete`, deferred to a follow-up on issue #7. M3 and M4 make
  the *editor* behavior safe (an honest empty completion, never a
  corrupting edit) without fixing the underlying recovery shape.
- **`crates/outou-lsp/src/dispatch.rs` is 959 lines**, over this project's
  usual file-size guidance. Not split in this pass: every candidate split
  (the `State`/`Pending` bookkeeping the rest of the file mutates through
  private fields) would need to either make those fields `pub(crate)` or
  move most of the file's functions together, and doing that safely this
  late in the pass was judged riskier than the file-size guidance was worth
  — flagged here for a follow-up refactor, not silently accepted as
  ideal.

## Decision

**Gate 3: PASS.** Completion, hover, definition and diagnostics all work
through the real parser and codegen for `let user = load_user(); <UserCard
user={user} />`, including cross-file definition, completion under
incomplete/broken input — now answered correctly at every position tested,
including the two (tag name, attribute name) that previously reached
rust-analyzer as the wrong kind of answer entirely — and both Outou's own
syntax diagnostics and rust-analyzer/flycheck's diagnostics mapped back to
the right `.rsx` position, with severity preserved even when the exact
generated span has no direct source. Every payload reaching the editor is
verified, mechanically and recursively, to be free of backend vocabulary.
Phase 0 proceeds to Step 6.
