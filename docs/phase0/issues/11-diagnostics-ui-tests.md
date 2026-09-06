---
title: "[Week 6] UI test harness for Outou diagnostics and backend-vocabulary leak detection"
milestone: "Phase 0"
week: "Week 6"
gate: "—"
droppable: no
labels: [phase0, must-keep]
---

| | |
|---|---|
| **Gate** | none (feeds Gate 4) |
| **Droppable** | No |

- [x] harness for `tests/ui/<case>/{input.rsx, expected.stderr}` (path-normalized, exact match)
- [x] three diagnostic layers exercised: Outou syntax, Rust semantic (mapped back), backend (translated)
- [x] a test that fails if any user-facing output contains `rsx! macro`, `PropsBuilder`, `dioxus_rsx`, `GeneratedNode` or similar backend vocabulary
- [ ] every row in `docs/backend-leakage.md` that is a diagnostic has a UI case

  Left unticked (issue #12 corpus review, F13: an earlier version of this
  file reworded the criterion itself to "... has a UI case (or is recorded
  below as needing none)" and ticked it — a scope change made by editing
  the acceptance text rather than recording it, which `AGENTS.md` does not
  permit). Every row *is* surveyed (see "Ledger coverage" below); one
  diagnostic row (18) genuinely has no UI case, for the good reason its
  own note there gives — a case reproducing it would fail `tests/leak.rs`'s
  own leak scan, defeating the point of that test. That is a tracked,
  deliberate gap, not a false claim of full coverage.

## What was built

- `crates/outou-cli/src/check.rs`: `run()`'s per-file body is now a reusable `pub fn check_source(file_name, source) -> CheckReport`, so the harness calls exactly the code `outou check` calls, in-process, never by spawning the binary ("there is one compiler", `AGENTS.md`).
- `crates/outou-syntax/src/vocabulary.rs` (new): `BACKEND_MARKERS`, `contains_backend_marker` and `translate_message` moved out of `crates/outou-lsp/src/translate.rs` into `outou-syntax`, so `outou-cli`'s harness and `outou-lsp` share one translation table instead of two copies. `outou-lsp/src/translate.rs` re-exports them under their original names; every existing `crate::translate::…` call site in `outou-lsp` keeps compiling unchanged, and `outou-lsp`'s own LSP-specific heuristics (`looks_like_generated_name`, `contains_component_props_marker`, `translate_diagnostic`) stay put.
- `crates/outou-cli/tests/ui.rs` (new): the harness, plus the "Rust semantic (mapped back)" / "backend (translated)" mapping+rendering machinery (harness-only for now — see the `TODO(phase0)` in that file's module doc about `outou check --semantic`).
- `crates/outou-cli/tests/leak.rs` (new): the backend-vocabulary leak scan, plus a unit test sweeping every `BACKEND_MARKERS` entry through `translate_message` and asserting the result is marker-free, and a second unit test pinning the exact rustc messages this issue observed.
- `tests/ui/README.md`, `tests/README.md`: document the case format, the fast/build split, and `BLESS=1`.
- `docs/backend-leakage.md`: appended row 27 (a newly observed, untranslated second diagnostic for the missing-required-prop scenario — see below).

## The three diagnostic layers

1. **Outou syntax** — `tests/ui/mismatched-closing-tag/`, `tests/ui/reserved-fragment/`, `tests/ui/unterminated-attribute-value/`. The first and third are copies of `tests/fixtures/diagnostics/mismatched-closing-tag.rsx` and `tests/fixtures/incomplete/unterminated-attribute-value.rsx` respectively (the originals are untouched — they belong to `crates/outou-syntax/tests/fixtures.rs`'s own fixture suite); `reserved-fragment/` (`<>`) is new. Fast: parsed and rendered in-process on every `cargo test`.
2. **Rust semantic, mapped back** — `tests/ui/semantic-type-mismatch/`: `let user: u32 = load_user();` where `load_user() -> String`, mapped back through the source map to the exact `.rsx` position of the `load_user()` call. The rendered message (`mismatched types`) needs no translation — it is already in Outou/Rust vocabulary, which is the point of this layer as distinct from the next one.
3. **Backend, translated** — three cases, each also a `docs/backend-leakage.md` row:
   - `tests/ui/backend-unknown-attribute/`: `<div frobnicate="x">` (row 12). rustc's raw message (`` cannot find value `frobnicate` in module `dioxus_elements::div` ``) contains no known specific rewrite target, so it gets the generic translation ("the backend rejected this element; see the generated code").
   - `tests/ui/backend-missing-required-prop/`: `<UserCard />` missing its required `name` prop (row 19). rustc's deprecated-`build`-method warning gets the specific `PropsBuilder` rewrite ("this component is missing a required property"); see row 27 for the second diagnostic this case surfaced.
   - `tests/ui/backend-non-into-dyn-node-child/`: `<p>{n}</p>` with `n: i32` (row 20). rustc's `IntoDynNode` trait-bound error gets the specific rewrite ("this value cannot be used as element content here").

All three build cases are `#[ignore]`d (need the full `dioxus` dependency tree) and share one throwaway crate, so the expensive build happens once for the whole suite, not once per case. `expected.stderr` for these three plus `semantic-type-mismatch` is a checked-in file that `tests/leak.rs` scans directly, without rebuilding — see `tests/ui/README.md`.

## Ledger coverage (`docs/backend-leakage.md`)

Every row surveyed; a row not listed here describes a Dioxus constraint or implementation note, never something a user sees as a diagnostic (props being `Clone`/`'static`/`PartialEq`, the `.generated/` file location, event-model/hook ownership, escaping/lowering internals, and so on).

| Row | Diagnostic? | UI case | Notes |
|---|---|---|---|
| 12 | Yes | `tests/ui/backend-unknown-attribute/` | |
| 18 | Yes, but not covered here | *(none)* | A hyphenated attribute name on a component reaches the user as a raw `PropsBuilder` compile error today; the row itself records that issue #6 deferred fixing it. Issue #11 owns the test *harness*, not `outou-backend-dioxus`'s escaping code (`crates/outou-backend-dioxus/src/escape.rs`, out of this issue's file ownership) — and a UI case whose `expected.stderr` contains `PropsBuilder` verbatim would itself fail `tests/leak.rs`'s scan, defeating the point of that test. Left as a known, tracked gap rather than either silently fixed (out of scope) or paved over with a leak-test exception. |
| 19 | Yes | `tests/ui/backend-missing-required-prop/` and `tests/ui/backend-non-into-dyn-node-child/` | The row's own two reproductions (a missing prop, a non-`IntoDynNode` child) map exactly onto these two cases; no separate case needed. |
| 20 | Yes | `tests/ui/backend-non-into-dyn-node-child/` | |
| 21 | Yes, historically; not live today | *(none)* | Row 23 narrows row 21: the one shape that still needs its synthesized braces (a nested-JSX island prop) always ships with the file-scoped `#![allow(unused_braces)]` that same row describes, so the lint this row documents does not currently fire for any input. Confirmed by inspection of `crates/outou-backend-dioxus/src/lib.rs`'s `GENERATED_LINT_ALLOWS` gating (`Writer::marked`) — a case reproducing row 21 verbatim would need to first construct an input where a kept-brace island exists *without* tripping that gate, which the current lowering rules do not allow. |
| 24 | Yes, but via the LSP, not `outou check` | *(none; covered by a unit test instead)* | Row 24 is specifically about `outou-lsp`'s translation table, reached through the IDE, not `cargo build`'s/`outou check`'s terminal output — there is no `outou check` invocation that exercises it. `crates/outou-cli/tests/leak.rs`'s `every_backend_marker_translates_to_a_marker_free_message` tests the (now shared) table directly instead. |
| 25, 26 | No | *(none)* | Both are about `textDocument/completion` and hover, never a diagnostic. |

### Newly observed, untranslated backend message

Building `tests/ui/backend-missing-required-prop/` surfaced a second rustc diagnostic beyond the one row 19 already documents: `` this method takes 1 argument but 0 arguments were supplied ``, pointing at the same `.build()` call as the (translated) deprecated-method warning. It contains no `BACKEND_MARKERS` substring, so `translate_message` leaves it untouched — recorded as `docs/backend-leakage.md` row 27, a known gap rather than a fix (substring matching cannot catch a message with no backend-shaped vocabulary in it at all).

## Verification

See the session's final report for verbatim command output. Summary: `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt -p outou-cli -p outou-syntax -p outou-lsp -- --check` and `cargo +1.85 check --workspace` all pass; the `#[ignore]`d UI/leak build case (`cargo test -p outou-cli --test ui -- --ignored`) passes, run manually.

## CI

No `ci.yml` job added by this issue (out of file ownership). `cargo test -p outou-cli --test ui --test leak` (the fast syntax-case and leak tests) already runs on every push through the existing `test` job's plain `cargo test --workspace` — no change needed there.

The one addition to make: the `example-app` job already builds the full `dioxus` dependency tree and runs the equivalent `#[ignore]`d probes for `outou-backend-dioxus` (`cargo test -p outou-backend-dioxus -- --ignored`, its last step). Add one more line there, after that step:

```yaml
      - run: cargo test -p outou-cli --test ui -- --ignored
```

It reuses the same warm `target/` the preceding steps already paid for (`Swatinem/rust-cache@v2` plus the `outou` crate's own `dioxus` build), so this costs one more `cargo check` of a small throwaway crate, not a second cold dependency build.
