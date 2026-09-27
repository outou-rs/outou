---
title: "[Cross-cutting] Triage every TODO(phase0) marker"
milestone: "Alpha"
gate: "—"
droppable: yes (LOW leftovers cut first)
labels: [alpha, cross-cutting, droppable, todo-triage]
---

| | |
|---|---|
| **Gate** | none (cross-cutting; runs alongside A1–A5) |
| **Droppable** | Partly — the triage walk is kept; fixing LOW items is first to cut |

`docs/phase0-results.md` §9 records 56 matches at the close of Phase 0; the markers to triage are the 46 lines found by the filtered command in `docs/alpha.md`'s cross-cutting triage section (it adds `examples/phase0-app/src/components.rsx:17`, which §9's pathspec misses). New unresolved points are tagged `TODO(alpha)`, not `TODO(phase0)` (AGENTS.md).

- [ ] Walk every line the filtered command finds, using `docs/phase0-results.md` §9's table for context.
- [ ] For each: fix it and remove the marker, **or** re-tag it `TODO(alpha)` with an owning issue if it is deferred again, **or** record it as accepted (a permanent, deliberate limitation) and remove the marker.
- [ ] Exclude the sites owned by other alpha issues: the two `crates/outou-syntax/src/parser/jsx/mod.rs` markers (`18-recovery.md`, #18) and the `<div class=` marker at `crates/outou-lsp/src/complete.rs:94` (`19-completion-gaps.md`, #19). The second `complete.rs` marker, at :432 (the L15 re-parse), stays in this issue.
- [ ] Re-run the filtered command at the end and confirm that it finds only `docs/adr/0013-rename-translation-and-refusal.md:30`, which is not a deferred item. Every other marker is fixed, re-tagged `TODO(alpha)` with an owning issue, or removed as accepted.
- [ ] Append a disposition row to `docs/backend-leakage.md` for row 29's rename/references gap if its status changes.
- [ ] Record the final disposition list in `docs/alpha-results.md`.
