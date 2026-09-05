---
title: "[Week 6] Corpus fetch and nightly corpus test"
milestone: "Phase 0"
week: "Week 6"
gate: "—"
droppable: no
labels: [phase0]
---

| | |
|---|---|
| **Gate** | none (feeds Gate 4: parser and corpus failures) |
| **Droppable** | No (the nightly job may lag; the corpus run at Week 8 may not) |

- [ ] `cargo xtask corpus fetch` reads `corpus.lock` and clones each entry at its commit into `.corpus/`
- [ ] `cargo xtask corpus test` runs the parser over every file and reports: panics, files that fail to parse, files whose JSX-free round trip differs
- [ ] pin `corpus.lock` to a stable rust-lang/rust tag instead of a moving commit
- [ ] `.github/workflows/corpus.yml` becomes a real signal (remove `continue-on-error`)
- [ ] corpus categories from the test matrix covered: valid/invalid Rust, macro token trees, qualified paths, raw strings, lifetimes, generics
