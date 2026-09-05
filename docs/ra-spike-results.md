# rust-analyzer spike results

Outcome of the Week 1 feasibility spike (`spikes/rust-analyzer/`). Fill in every field; a blank field means the spike is not finished.

## Environment

| Item | Value |
|---|---|
| Date | 2026-09-05 |
| rust-analyzer version | 1.98.1 (48a229ce 2026-09-01) |
| rustc / cargo version | rustc 1.98.1 (48a229cea 2026-09-01) / cargo 1.98.1 (797e8a9bc 2026-08-05) |
| OS | macOS 26.6.2 (build 25G83), Darwin 25.6.0, arm64 |

Node v24.14.0 ran `client/ra-client.mjs`. rust-analyzer was invoked as the
rustup shim at `~/.cargo/bin/rust-analyzer` (the rustup-provided `rust-analyzer` component); the editor's own
rust-analyzer instance (PID 98407) was left untouched and not used for this
spike. All raw command output is under `spikes/rust-analyzer/results/`, with
the exact command for every file listed in
[`results/README.md`](../spikes/rust-analyzer/results/README.md).

## Strategy A, layout (b): `src/.generated/` + `#[path]`

| Criterion | Works? | Notes |
|---|---|---|
| 1. Cargo project loads normally | yes | `cargo check` (default features) finishes in well under 1 s on a warm cache (0.29 s wall-clock via `time`, 0.24 s per Cargo's own report; `results/environment-and-cargo.txt`) with no errors. `results/b-hover-user-cold.json.gz` shows `ready: true` and `serverInfo.name: "rust-analyzer"`. |
| 2. Generated source is in the crate graph | yes | Hover on `user` in the generated file resolves the real cross-file type: `let user: User` (`results/b-hover-user-cold.json.gz`). Go-to-definition on `load_user()` jumps to `src/main.rs:14` (`results/b-definition-load_user-cold.json.gz`), proving the generated file participates in the same crate graph as ordinary sources, not an isolated buffer. |
| 3. Editor changes reach rust-analyzer without `build.rs` | yes (vacuously and by measurement) | Layout (b) has no `build.rs` dependency for this file at all: it is an ordinary `#[path]` module, so there is nothing to re-run. `results/b-overlay-change-cold.json.gz` confirms the *mechanism* still works as expected: hover before the second buffer is `User`, hover2 after `textDocument/didChange` is `String`, in 145 ms (145/104 ms cold/warm), matching the edit `load_user()` → `load_user().name`. |
| 4. Completion uses the latest buffer | yes | Three probes, all positive: (a) member completion after `user.` typed on an otherwise-incomplete new line returns `name` and `age` as the top two of 10 items (`results/b-completion-member-cold.json.gz`) — this is completion on a buffer that does **not parse cleanly** (a bare `user.` statement), so it also demonstrates recovery under incomplete input; (b) component completion at the `UserCard` tag returns `UserCard` among 658 items (`results/b-completion-component-cold.json.gz`); (c) prop completion on `user: user` returns `user`/`build` among 4 items (`results/b-completion-prop-cold.json.gz`). |
| 5. Hover uses the latest buffer | yes | `results/b-overlay-change-cold.json.gz`/`-warm.json.gz`: hover is `User` before the `didChange`, `String` after, with no re-run of anything on disk. |
| 6. Definition uses the latest buffer | yes | `results/b-definition-latest-buffer-cold.json.gz` / `-warm.json.gz`: an overlay adds a use-site `let _probe = user.age;`; the `didChange` overlay inserts one line *above* the declaration. Definition at the use-site moves with the buffer — `App.rs` 8:8–8:12 before, **9:8–9:12** after — so the answer comes from the edited buffer, not a stale one. |
| 7. flycheck / diagnostics map back to `.rsx` | yes, via flycheck; native (pre-save) diagnostics do not surface semantic errors | Two separate mechanisms were measured. **Native (in-memory) diagnostics:** a type error overlay (`let user: u32 = load_user();`) produces **zero** diagnostics even though hover simultaneously confirms rust-analyzer understood the annotation (`let user: u32`, `results/b-diagnostics-native-typeerror.json.gz`) — this reproduces the limitation Task A flagged. A **syntax** error overlay (missing `;`) *does* produce a native diagnostic, and it maps correctly through `source-map.json` back to `App.rsx` (`results/b-diagnostics-native-syntaxerror.json.gz`, `unmapped: false`, source span `App.rsx:4:15-26`). A follow-up check (`results/supplementary-diagnostics-nonmacro-typeerror.json.gz`, `supplementary-diagnostics-nonmacro-badimport.json`) shows this is **not** specific to the `#[component]` macro: a plain `let x: u32 = load_user();` and an unresolved `use` inside ordinary `fn main()` code (no macro involved) also produce no native diagnostic. So the gap is general to this rust-analyzer configuration, not an Outou/Dioxus-macro problem. **Flycheck (on-save `cargo check`):** writing the same type error onto disk and letting `checkOnSave` run produces the full `rustc` diagnostic (`E0308`, both call sites), and it maps correctly through `source-map.json` to two separate `App.rsx` spans (`results/b-diagnostics-flycheck-typeerror.json.gz`, both `unmapped: false`). |

## Strategy A, layout (a): `OUT_DIR` + `include!`

| Criterion | Works? | Notes |
|---|---|---|
| 1. Cargo project loads normally | yes | `cargo check --no-default-features --features gen-outdir` succeeds; `OUT_DIR` is discoverable from `--message-format=json` `build-script-executed` events, filtered by `package_id` (13 dependencies also report `build-script-executed`; only one `package_id` contains `ra-spike-fixture`; `results/environment-and-cargo.txt`). See the caveat in `results/README.md` about `--message-format=json` returning 13 rows, not 1. |
| 2. Generated source is in the crate graph | yes | Hover on `user` at the (corrected) position resolves `User` (`results/a-hover-user-cold.json.gz`); go-to-definition on `load_user()` resolves cross-file to `src/main.rs:14` (`results/a-definition-load_user-cold.json.gz`). **Caveat:** the positions in `source-map.json`/the README's probe table are specific to layout (b)'s file text. `virtual/App.rs` (what `build.rs` copies into `OUT_DIR`) is missing that file's `use super::*;` line and the blank line after it (both files have the same three-line header comment), so every generated-line position is offset by **-2** for layout (a); see `results/README.md`. |
| 3. Editor changes reach rust-analyzer without `build.rs` | yes | `results/a-overlay-change-mtimes.txt` (transcript) / `-run.json.gz`: mtimes of `$OUT_DIR/outou/App.rs` and `target/debug/build/<pkg>/output` were recorded before and after a fresh overlay-change probe and are unchanged (`1788587415` both times, for both files), and the generated file's sha256 is identical before and after — `build.rs` did not re-run. `hover2` after the `didChange` correctly reports `String` (`let user: String`, from the overlay's `.name` edit), `overlayChangeToHover` 95 ms in this run (see also `results/a-overlay-change-cold.json.gz`/`-warm.json.gz`: 185 ms cold / 297 ms warm). |
| 4. Completion uses the latest buffer | **no** — fails on exactly the two scenarios that matter for Outou | Completion on the plain, non-macro `let` line works generically (199 items after the `.name` edit, `results/a-overlay-change-cold.json.gz`). But: (a) **member completion under incomplete input** — the identical `user.`-on-a-new-line overlay that returned 10 items (incl. `name`, `age`) in layout (b) returns **zero** items in layout (a) (`{"isIncomplete": true, "items": []}`, `results/a-completion-member-cold.json.gz` and `-warm.json.gz`, reproducible on both runs); (b) **completion inside the `rsx!` macro call**, on syntactically valid, *unmodified* content — both component completion and prop completion return **`null`** (not even an empty list) at the equivalent positions that returned 658 and 4 items respectively in layout (b) (`results/a-completion-component-cold.json.gz`, `results/a-completion-prop-cold.json.gz`, reproduced on the `-warm` runs too), even though `hover` at the exact same position works and returns full, correct type information. Mechanism not investigated; what was measured is a layout difference: the same generated text served as an `include!()` splice loses completion where the `#[path]` module keeps it. Completion breaks specifically at the macro-call positions and the incomplete-input position probed here, which are exactly where Outou's generated JSX call sites (`rsx! { ... }`) live. |
| 5. Hover uses the latest buffer | yes | Same overlay-change evidence as criterion 3: hover before is `User`, hover2 after is `String`. |
| 6. Definition uses the latest buffer | yes | `results/a-definition-latest-buffer-cold.json.gz` / `-warm.json.gz`: same probe as layout (b) — definition at the use-site moves with the buffer, `$OUT_DIR/outou/App.rs` 6:8–6:12 before, **7:8–7:12** after. |
| 7. flycheck / diagnostics map back to `.rsx` | yes, via flycheck, with an added integration cost | Same native/flycheck split as layout (b): native diagnostics show syntax errors but not type mismatches (`results/a-diagnostics-native-typeerror.json.gz` empty; `results/a-diagnostics-native-syntaxerror.json.gz` has the syntax diagnostic). Applying the **unmodified, layout-(b) `source-map.json`** to layout (a)'s diagnostic produced `unmapped: true` — not because the mapping mechanism failed, but because the map's hard-coded line numbers (8/11/12) don't match layout (a)'s actual lines (6/9/10), per the -2 offset above. Rebuilding a copy of the map with every `generated` line shifted by -2 made the identical diagnostic map correctly (`results/a-diagnostics-native-syntaxerror-adjusted-sourcemap.json.gz`, `unmapped: false`). Flycheck likewise surfaces the full `E0308` diagnostics and, with the adjusted map, maps them correctly to `App.rsx` (`results/a-diagnostics-flycheck-typeerror.json.gz`). **Net effect:** the mapping mechanism itself is layout-agnostic, but layout (a)'s generated file has a different header-line count than layout (b)'s, so a real implementation targeting both layouts from one compiler would need per-layout-aware span emission — a real, if small, added cost that layout (b) does not have. |

## Summary

| Question | Answer |
|---|---|
| Strategy A works / does not work | Works, for layout (b). Layout (a) fails one of the seven required criteria (completion). |
| Overlay works / does not work | Works for both layouts on ordinary (non-macro, syntactically valid) content; layout (a) additionally fails to serve completion for macro-call positions and incomplete input, where layout (b) succeeds. |
| Completion works | Yes for layout (b) (all three probes, including under incomplete input). No for layout (a) (fails inside the `rsx!` macro and under incomplete input; only succeeds outside the macro on valid syntax). |
| Hover works | Yes, both layouts, including live buffer updates via `didChange` with no `build.rs` re-run. |
| Definition works | Yes, both layouts, cross-file, both before and after a live buffer edit. |
| Diagnostics work | Native (pre-save) diagnostics only ever showed **syntax** errors, in both layouts; semantic (type-mismatch, unresolved-import) errors did not appear natively even outside any macro (see the two `supplementary-diagnostics-nonmacro-*` probes). **Flycheck** (on-save `cargo check`) diagnostics carry full rustc detail and map correctly to `App.rsx` through `source-map.json`, in both layouts (layout (a) needs a map built for its own line offsets). |
| Chosen layout: (a) OUT_DIR / (b) fixed path | **(b) fixed path.** Layout (a) fails criterion 4, which is also one of Phase 0's "what must not be cut" items (completion while input is incomplete or broken) — and it fails it at precisely the two positions (inside `rsx!`, and under incomplete input) that are Outou's actual, everyday generated-code shape. |

## Known blockers

- **Native (pre-save) diagnostics do not report semantic errors.** Confirmed with a type-mismatch overlay and an unresolved-import overlay, in both layouts and in plain non-macro code (`results/*-diagnostics-native-typeerror.json.gz`, `results/supplementary-diagnostics-nonmacro-*.json.gz`). Only **syntax** errors appear natively. Practical consequence: a Rust type error typed into a `.rsx` file will not show as a live diagnostic until a `cargo check` (flycheck) round-trips, roughly 4.5–7.3 s from rust-analyzer `initialize` with the error already on disk (the spike did not measure edit-to-diagnostic; see Measured latency below). Outou's "Rust type errors reported at the right position" success criterion (`docs/phase0.md`) is met, but only through flycheck, not instantly.
- **Layout (a) breaks completion inside the generated macro call and under incomplete input** (criterion 4, both instances reproduced twice — cold and warm). This is the decisive reason layout (a) is rejected; see the layout-(a) table above.
- **`source-map.json` as committed is layout-(b)-specific.** It encodes literal generated-file line numbers, which differ between layout (a) and (b) because layout (a)'s `virtual/App.rs` lacks layout (b)'s `use super::*;` line and the blank line after it (two lines). Since layout (b) is being adopted, this is moot for the shipped design, but it is worth noting for whoever writes the real compiler's span emission: the source map must be built from the actual generated text of whichever layout is in play, not treated as reusable across layouts.
- **`cargo check --message-format=json | jq 'select(.reason=="build-script-executed")'` returns one line per build-script dependency (13 in this fixture), not one line.** The README's example command needs the `package_id` filter used throughout this run (`results/README.md`) to reliably find the fixture's own `OUT_DIR`.

## Measured latency

All values are `latencyMs` fields read directly from the client's JSON output (see `spikes/rust-analyzer/results/`). "Cold" = first client invocation against a given `--file`/feature combination in this session; "Warm" = an immediately repeated, identical invocation. rust-analyzer is spawned fresh by the client on every invocation (there is no long-lived server across runs), so "warm" here reflects OS/filesystem and Cargo-metadata caching, not a persistent rust-analyzer process.

### Layout (b): `src/.generated/App.rs`

| Request | Cold (ms) | Warm (ms) |
|---|---|---|
| initialize → first hover | 7714 | 7052 |
| hover | 1378 | 1711 |
| completion | 70 | 174 |
| definition | 29 | 6 |
| overlay change → updated hover | 145 | 104 |
| flycheck diagnostics (initialize → all diagnostics published) | 4.5–7.3 s (bracketed)* — Cargo-warm, not a cold/warm pair | see note* |

### Layout (a): `$OUT_DIR/outou/App.rs`

| Request | Cold (ms) | Warm (ms) |
|---|---|---|
| initialize → first hover | 5622 | 5265 |
| hover | 920 | 688 |
| completion | 37 | 34 |
| definition | 1 | 1 |
| overlay change → updated hover | 185 | 297 |
| flycheck diagnostics (initialize → all diagnostics published) | ≤7.3 s (upper bound only)* — Cargo-warm, not a cold/warm pair | see note* |

\* `ra-client.mjs` has no field for "time until flycheck diagnostics arrived": it always sleeps the full `--settle` duration before checking, rather than returning as soon as a diagnostic lands. This number was derived by bracketing, and the environment throughout was Cargo-warm (Cargo's own incremental cache was already warm from earlier `cargo check` runs in this session), not a cold/warm pair in the sense used elsewhere in this table: `results/supplementary-flycheck-timing-bracket-b-settle500.json.gz` — `totalMs` 4991 ms, 2 of the eventual 4 diagnostics for the generated file published; `results/supplementary-flycheck-timing-bracket-b-settle3000.json.gz` — `totalMs` 7268 ms, all 4 diagnostics for the generated file plus 1 related diagnostic on `src/main.rs` published ("4+1"); `results/supplementary-flycheck-timing-bracket-a-settle3000.json.gz` — `totalMs` 7262 ms, also "4+1". No `a-settle500` probe was run, so layout (a)'s figure is an upper bound only (`≤7.3 s`), not a bracket. For layout (b), where both ends of the bracket exist, "all flycheck diagnostics published" happens somewhere between roughly 4.5 s and 7.3 s after `initialize`.

† A true cold/warm distinction for flycheck specifically (e.g., after `cargo clean`) was not measured, to keep the spike within its time box; the bracketing above was run once per layout, both effectively "warm" with respect to Cargo's own build cache.

## Decision

- **Gate 0: PASS.** Hand-written generated Rust gets real rust-analyzer hover, definition, and completion through Strategy A, with one layout (fixed-path) satisfying all seven criteria.
- **ADR 0009 status after this spike: adopt layout (b), `src/.generated/` + `#[path]`.** Layout (a) (`OUT_DIR` + `include!`) is rejected: it fails criterion 4 (completion) precisely at the two positions that matter most for Outou (inside the generated `rsx!` macro call, and under incomplete/broken input while the user is mid-edit), which is also one of Phase 0's non-droppable items. Per the ADR's own decision rule ("if (b) satisfies all seven Strategy A criteria, (b) is adopted and `OUT_DIR` generation is dropped along with `outou-build`"), `OUT_DIR` generation and `crates/outou-build` should be dropped; `outou package` only needs to generate `.generated/` files and rely on `#[path]`. (ADR 0009 was moved to **Accepted** after this spike, under issue #2.)
- **Strategy B: not needed.** Strategy A (layout b) passes all seven criteria, so the shadow-Cargo-project fallback described in `spikes/rust-analyzer/README.md` does not need to be spiked.
