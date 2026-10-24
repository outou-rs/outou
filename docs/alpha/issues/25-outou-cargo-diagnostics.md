---
title: "[A5] outou cargo <build|check|clippy|test>: translate rustc diagnostics to .rsx positions (F5)"
milestone: "Alpha"
gate: A5
droppable: no
labels: [alpha, gate-a5, must-keep, cli]
---

| | |
|---|---|
| **Gate** | A5 |
| **Droppable** | No |

Decided in ADR 0014 (F5 is part of A5): `cargo build` / `cargo clippy` diagnostics for `.rsx` code point at `src/.generated/<file>.rs`, not at the `.rsx` source (dogfood-board friction F5). A measured prototype showed that rustc's JSON `spans` map back through the existing source maps for type errors, borrow errors, unused variables, clippy lints and unknown attributes. `RUSTC_WORKSPACE_WRAPPER` was rejected: clippy ignores it.

- [ ] Add subcommand `outou cargo <build|check|clippy|test>`: generate (Strict), then run `cargo <sub> --message-format=json-diagnostic-rendered-ansi` and forward the other arguments. Never pass `--fix`.
- [ ] Parse `compiler-message` lines. Translate each span through the existing source maps (`outou-sourcemap`) to the `.rsx` file, line and column, and render the diagnostic at that position. Other output lines (test output) pass through. The exit code is cargo's.
- [ ] Rules for what cannot be translated (documented): spans with no source mapping fall back to the nearest mapped span or are kept by a stated rule; spans that point at backend paths (`def_site_span` and similar) are dropped as leakage; plain `.rs` paths such as `src/.generated/../state.rs` are normalised.
- [ ] `outou build` stays generate-only (add a test that it does not run cargo).
- [ ] Move the translation logic of `crates/outou-cli/tests/ui.rs` into a library module reused by both the CLI and the harness (one compiler, AGENTS.md); the UI tests keep passing. Move the shared narrow/nearest core into `outou-sourcemap` only if it can be done without touching `outou-lsp`'s behaviour.
- [ ] Tests: dogfood-board and a UI fixture show diagnostics at the `.rsx` position; untranslatable messages follow the rule; the backend vocabulary check passes on the output.
- [ ] Make the help text distinguish `outou cargo` from `outou check`; consider F16 (success output) at the same time.
- [ ] Ledger: append a row for F5 (diagnostics at generated positions) and its disposition.

**Acceptance:** `outou cargo check` in `examples/dogfood-board` with a deliberate type error prints the error at the `.rsx` line and column, with no `src/.generated` path and no backend vocabulary; the existing UI tests still pass using the shared library.
