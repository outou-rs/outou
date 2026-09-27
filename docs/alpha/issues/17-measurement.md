---
title: "[A1] Measurement: fix the provisional performance budget"
milestone: "Alpha"
gate: A1
droppable: no
labels: [alpha, gate-a1, must-keep, measurement]
---

| | |
|---|---|
| **Gate** | A1 |
| **Droppable** | No |

`docs/phase0.md`'s "Performance budget" left three targets provisional; this issue fixes each one as a threshold. It also records baselines, without thresholds, for three measurements that rust-analyzer rather than Outou dominates: the two-process resource cost `docs/phase0-results.md` §10 asks for, flycheck latency and cold init.

- [ ] Fix the measurement setup: `examples/phase0-app` and `tests/fixtures/workspace` (the `app`/`ui-kit` workspace that §4 and §6 measured). Record the machine, OS, Rust toolchain and rust-analyzer version.
- [ ] Baseline: sample RSS and CPU time of `outou-lsp`'s rust-analyzer and the editor's own rust-analyzer during cold indexing and during a scripted editing session, against a single ordinary `rust-analyzer` session on the same code (`docs/phase0-results.md` §10).
- [ ] Baseline: flycheck latency (`didSave` → mapped semantic diagnostic), N ≥ 10 on a warm target directory, median and max, next to a plain `.rs` save under plain rust-analyzer on the same code so Outou's added part is separated out.
- [ ] Baseline: cold `initialize` → first usable hover/completion/definition, N runs, median and max.
- [ ] Threshold: incremental `.rsx` → generated Rust, measured on both workspaces; fix a millisecond threshold and change `regenerating_one_unit_is_fast`'s assertion to it.
- [ ] Threshold: forwarded-completion overhead, re-measured with the same-session A/B method (`docs/gate3-results.md`, "Proxy overhead (S8)"); fix a median-ms threshold stated per candidate-list size.
- [ ] Threshold: "a single-file edit never regenerates the whole crate" as pass/fail on both the language-server path and the `outou build` CLI path (the CLI path is A3's work).
- [ ] Record every number, threshold and exact method (tool, sample rate, N, workload script) in `docs/alpha-results.md` so they can be reproduced.
