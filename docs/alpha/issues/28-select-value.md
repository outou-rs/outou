---
title: "[A5] <select value={V}>: select the matching option (F18)"
milestone: "Alpha"
gate: A5
droppable: no
labels: [alpha, gate-a5, must-keep, cross-cutting]
---

| | |
|---|---|
| **Gate** | A5 |
| **Droppable** | No |

Decided in ADR 0014. Dioxus applies a `<select value>` without selecting the matching option on mount, unlike React; the dogfood-board showed the wrong option until each `<option>` got a hand-written `selected` (ledger row 37, friction F18).

- [x] Within a `<select value={V}>` subtree, rewrite each lexically contained `<option>` (including those inside `.map(|p| <option ..>)`). Evaluate V once outside the `rsx!` call as a `String`, then add `selected` computed by `option_selected(..)` (string comparison with the option's value). An explicit `selected` attribute on an option wins.
- [x] Out of scope and documented (in `docs/grammar.md` or the lowering docs): options reached through variables, options returned by components, options without a `value` attribute, and `multiple`. Warn where this can be detected.
- [x] Tests: codegen goldens, a headless render test for the `selected` flags, and the dogfood-board's `selected={option == current}` workarounds removed. Check that duplicate diagnostics do not appear because the option expression is emitted twice (many-to-many source map).
- [x] Browser re-check (manual, F13): new-task form, edit form of a High task, and the remount case. Append a ledger row resolving row 37 within the stated limits.
  - Browser check (Fable, 2026-10-24): at `caaaf3c` / `8157e66`, with `dx serve --platform web`, the new-task form shows Priority Medium, the edit form of a High task shows High, the detail Status stays "In progress" after opening another task and coming back; no console errors.

**Acceptance:** `<select value={x}>` shows `x` on mount in a browser for the lexical cases; the limits are documented.

**Implemented** (2026-10-24), browser re-check done. Notes:

- Design: the front end scans each `<select value>` subtree (`crates/outou-syntax/src/select.rs`) and the backend binds `V` and each option's value once before the call (`crates/outou-backend-dioxus/src/select.rs`, `bindings.rs`); the facade has the sealed `outou::jsx::IntoOptionValue` (strings, integers, `char`). Because the option's value is bound and then used for both `value` and `selected`, the expression is emitted once, so the duplicate-diagnostic risk in the item above does not arise (checked in a source-map test and with a real `cargo check`). Details and limits: `docs/grammar.md` §5.1, ledger row 43.
- Warnings (Outou diagnostics, from `outou check` and the language server): `<select multiple value>`, an option without `value`, a component or a JSX-free `{ … }` child inside a `<select value>`, and a `<select value>` with no option in it. `multiple={false}` is a single select (rewritten); a dynamic `multiple={flag}` is treated as `multiple` and warned about. Not detected: a child that mixes JSX with options from elsewhere. `outou build` and `outou cargo` print the warnings to standard error as well (#31, ledger row 56, `docs/grammar.md` §5.1). `outou fmt` and the language server format a file that has only warnings. A user identifier with a generated-binding prefix (`__outou_select_…`, `__outou_opt_…`, `__outou_selected_…`, `__outou_key_…`) in a JSX island is a front-end error (`crates/outou-syntax/src/reserved.rs`). The generated `let`s allow `clippy::borrow_deref_ref`, so `outou cargo clippy -- -D warnings` passes for `key={*n}` and `value={*x}`.
- Tests: `crates/outou/tests/jsx_select.rs`, `crates/outou-syntax/src/select.rs` (unit), `crates/outou-backend-dioxus/src/lib.rs` (unit), `tests/golden/selects`, `tests/select_compile.rs` (`#[ignore]`d: headless `selected` flags for each shape; unsupported types reported once), `tests/ui/select-value-warnings` and `tests/ui/select-value-unsupported-type`, and the dogfood-board's `main.rsx` tests (edit form, new form, detail view) with its hand-written `selected={..}` removed.
- Browser check (Fable, 2026-10-24): new-task form shows Medium; the edit form of a High task shows High; the detail Status select is right after opening another task and coming back; no console errors.
