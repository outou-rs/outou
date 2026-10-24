---
title: "[A3] outou build: stop regenerating the whole crate on a single-file edit"
milestone: "Alpha"
gate: A3
droppable: yes
labels: [alpha, gate-a3, droppable, cli]
---

| | |
|---|---|
| **Gate** | A3 |
| **Droppable** | Yes (see `docs/alpha.md`, "What is cut first") |

`docs/phase0-results.md` §6: "a single-file edit never regenerates the whole crate" is met on the language-server path (`regenerating_one_unit_does_not_touch_another`) but missed on the `outou build` CLI path — it regenerates every unit in the plan on every invocation. A workspace measurement rewrote all five of `ui-kit`'s generated files for an edit confined to `app/src/main.rsx`.

- [x] Identify why `outou build` regenerates every unit instead of only the changed one(s) (`crates/outou-cli/src/build/`).
- [x] Implement change detection so `outou build` only rewrites units whose generated output changed (compared byte-for-byte against what is on disk). All units are still parsed and generated; the criterion is on the write side (file writes and mtimes), not on skipping codegen.
- [x] Add a CLI-path test: after an edit to one `.rsx` file, `outou build` does not write the sibling unit's generated file at all (its mtime is unchanged, or emit reports it as skipped). Byte-identical content is not enough, because it already holds today.
- [x] Confirm generated output remains deterministic (AGENTS.md: "Generated Rust is deterministic") after the change.
- [x] Record the fix's evidence in `docs/alpha-results.md`.
