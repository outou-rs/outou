# Alpha: internal front-end quality

Phase 0 proved the front end can work. The alpha does not build a public product. It answers one question:

> Are Outou's front end and tooling solid enough to build a real application on, and which runtime should they sit on?

`docs/phase0-results.md` §10 concludes that the Phase 0 criteria are met, with two stated qualifiers (flycheck-only semantic diagnostics; `cargo build` alone requiring a prior `outou build` or LSP save). Gate 4 there names the next step as "GO to a front-end alpha" (`docs/phase0.md`, Order of work and gates, row 8) without defining it; this document is that definition.

The purpose of the alpha is internal quality, not a public release: the people building Outou build a real application on it and record, with evidence, whether the result can be trusted.

## Non-goals

- **Public release.** No `crates.io` publish of `outou`/`outou-cli`, no VS Code Marketplace listing, no stable API promise.
- React interop.
- A router, SSR, HMR.
- Implementing a native runtime. The alpha only *decides* the runtime; it does not build one.
- Production CSS.

## Time box

None. The gates below are the only exit conditions.

## Order of work and gates

Work proceeds in this order, by risk, as in Phase 0.

| Gate | Work | Gate condition |
|---|---|---|
| A1 | **Measure first.** Sample RSS and CPU of the two rust-analyzer processes (`crates/outou-lsp/README.md`, "Two processes, not one") against a representative session, the way `docs/phase0-results.md` §10 describes wanting it measured. Measure flycheck latency, forwarded-completion overhead and cold init as baselines. Fix the thresholds `docs/phase0.md`'s "Performance budget" section left provisional. | Each of `docs/phase0.md`'s three provisional budget lines has a measured number and a fixed threshold (or pass/fail condition); the two-process RSS/CPU cost, flycheck latency and cold init are measured and recorded as baselines. |
| A2 | **Recovery and editor robustness.** Fix the swallowed-tail gap (`docs/phase0-results.md` §2/§10, `LOW-17`). Fix the two recovery gaps recorded as `TODO(phase0)` in `crates/outou-syntax/src/parser/jsx/mod.rs` (`recover_fragment`, `recover_stray_close`). Fix the `<div class=` sibling-swallowing gap in `crates/outou-lsp/src/complete.rs` (today `completion: null`). | Four fixtures exist under `tests/fixtures/incomplete/`, one for each A2 shape: the swallowed tail, a truncated `</` after a reserved fragment, a top-level truncated `</div` at end of file, and `<div class=` followed by a sibling element. In the swallowed-tail and `<div class=` fixtures, the item or sibling element after the broken construct survives as its own node. Every fixture passes both `every_incomplete_fixture_produces_parseable_analyzable_rust` and `every_incomplete_fixture_compiles_with_no_parse_errors` (`crates/outou-backend-dioxus/tests/recovery.rs`). `<div class=` followed by a sibling yields a correct non-fallback completion result, not `completion: null`. The three A2 `TODO(phase0)` markers are removed. |
| A3 | **Close the Phase 0 budget misses.** A single-file edit must not regenerate the whole crate on the `outou build` CLI path (`docs/phase0-results.md` §6 — met on the language-server path, missed on this one). Bring forwarded-completion overhead within the A1 threshold. | Both are met under the A1 thresholds. |
| A4 | **Dogfood application.** Build an internal, interactive multi-module app in `.rsx` that runs in a browser, larger than `examples/phase0-app` (forms, lists, several components across files, a workspace library). It may re-export the temporary backend's existing state, event and launch APIs through `outou::prelude`; each re-export is recorded under ADR 0010 and as a `docs/backend-leakage.md` row. Log every friction point, and every backend-leakage encounter as a new row in `docs/backend-leakage.md`. | The app builds and passes CI (mirroring the `example-app` job in `.github/workflows/ci.yml`), its interactive flows are exercised by a documented manual walkthrough in the app's README, and every friction item is either fixed or recorded with a disposition. |
| A5 | **Runtime decision** (final gate). Weigh `docs/backend-leakage.md`'s ledger and the A4 evidence against the criteria its closing paragraph states: how many rows make Outou semantics unnatural, weighed against interop freedom, renderer control, performance, bundle size, API stability and upgrade cost. Record the decision in a new ADR that supersedes ADR 0003 and updates ADR 0010 if the facade changes. | The ADR is written, and `docs/alpha-results.md` is written. |

### A1: measurement, in detail

`docs/phase0-results.md` §6 already measured all three against their provisional wording: incremental regeneration (met; `regenerating_one_unit_is_fast`, well under 50ms), forwarded completion overhead (10–25ms median, proportional to candidate-list size) and the single-file-edit line (met on the language-server path, missed on the `outou build` CLI path; carried into A3 below). What Phase 0 did not measure is the resource cost of running two rust-analyzer processes per workspace (`crates/outou-lsp/README.md`, "Two processes, not one") — `docs/phase0-results.md` §10 names this directly as something "an alpha would want ... measured directly, by sampling both processes' RSS and CPU time on a representative workspace ... versus a single ordinary `rust-analyzer` session on the same code." A1 does that sampling, records cold-init and flycheck latency as baselines, and turns every provisional line into a fixed, checked-in threshold.

### A2: recovery, in detail

Four known gaps, all already on record rather than newly discovered:

- The swallowed-tail limitation: a body-less function whose parameter list never reaches a closing `)` can swallow following source before Outou's own symbol-preservation recovers (`crates/outou-backend-dioxus/README.md:30`, `LOW-17`; `docs/phase0-results.md` §2/§10).
- `Parser::recover_fragment` in `crates/outou-syntax/src/parser/jsx/mod.rs`, marked `TODO(phase0)`: a truncated `</` right after a reserved fragment is spliced into Recovery output as invalid Rust with no diagnostic.
- `Parser::recover_stray_close` in `crates/outou-syntax/src/parser/jsx/mod.rs`, marked `TODO(phase0)`: a top-level truncated `</div` at end of file loses both its diagnostic detail and the tag name.
- The `<div class=` sibling-swallowing gap in `crates/outou-lsp/src/complete.rs` (`TODO(phase0)`, issue #9 Gate 3 review): the recovery parser folds a following sibling element into a broken tag's own attributes, so there is nothing meaningful at that cursor for completion to classify. Its visible symptom is `completion: null` at that position (`docs/phase0-results.md` §5, "Completion — HTML attribute value"): every forwarded item fails the cursor-containment check.

### A3: budget misses, in detail

`docs/phase0-results.md` §6 records the `outou build` CLI path regenerating every unit in a workspace's plan on every invocation, even when only one file changed — met on the language-server path (`regenerating_one_unit_does_not_touch_another`), missed on the CLI one. Forwarded completion overhead is measured but not yet judged against a fixed threshold, because A1 is what fixes that threshold; A3 is where the overhead is brought under it.

### A4: dogfood, in detail

`examples/phase0-app` was built to exercise Gate 4, not to be lived in. A4 builds something bigger and interactive: an app that runs in a browser, with multiple components across files, a workspace library dependency, forms and lists, the kind of structure a real internal tool has. It lives under `examples/` as a non-workspace member, the same way `examples/phase0-app` does; CI runs `outou build` then `cargo build` and clippy on it, mirroring the `example-app` job in `.github/workflows/ci.yml`. The app may re-export the temporary backend's existing state, event and launch APIs through `outou::prelude` — today's prelude publicly exports only `component` and `Element` (`crates/outou/src/lib.rs`) — and each such re-export is recorded under ADR 0010 and as a new row in `docs/backend-leakage.md`. Every rough edge — not just backend leakage — gets logged in a "Friction log" section of the app's README; backend-specific ones become new, appended rows in `docs/backend-leakage.md`, following its existing classification scheme.

### A5: runtime decision, in detail

This is the gate Phase 0 deferred on purpose (`docs/adr/0003-dioxus-as-temporary-backend.md`: "After the front end is proven, a runtime decision is made from that ledger: keep Dioxus, or build a native runtime"). `docs/backend-leakage.md` presently has 29 rows; `docs/phase0-results.md` §7/§10 already flags which of them make Outou semantics unnatural (props bounds, `Element`'s shape, borrowed props, non-`IntoDynNode` children, hyphenated component attributes, rustc printing backend paths directly). A5 weighs that list, plus whatever A4 adds, against interop freedom, renderer control, performance, bundle size, API stability and upgrade cost, and writes the decision into an ADR superseding ADR 0003.

## If a gate cannot be met

The alpha has no macro fallback; `docs/design.md`'s "If the bet fails" applied to Phase 0 only. An unmet gate is recorded rather than treated as passed:

- The shortfall is recorded in `docs/alpha-results.md` with evidence: what was measured or attempted, what was missing, and why.
- The gate is not marked passed. A3 and A4 may be cut only as described under "What is cut first", and a cut is recorded the same way.
- **A5, the runtime decision, proceeds using the evidence available.** The gap is stated in the ADR and in `docs/alpha-results.md`, as `docs/phase0-results.md` §10 states Phase 0's two qualifiers (criteria 5 and 9).

## Cross-cutting: `TODO(phase0)` triage

`docs/phase0-results.md` §9 inventories the `TODO(phase0)` markers at the close of Phase 0 (56 matches of its `'*.rs' '*.md'` command); one tracked marker outside that pathspec, `examples/phase0-app/src/components.rsx:17`, is also open. The markers to triage are the ones this command finds, which excludes documents that only discuss the convention (`AGENTS.md`, the alpha documents), historical records (`docs/phase0-results.md`, `docs/gate3-results.md`, `docs/phase0/issues/`) and the append-only `docs/backend-leakage.md`:

    git grep -n "TODO(phase0)" -- . ':!AGENTS.md' ':!docs/alpha.md' ':!docs/alpha/' ':!docs/phase0-results.md' ':!docs/gate3-results.md' ':!docs/phase0/issues/' ':!docs/backend-leakage.md'

It finds 46 lines at the start of the alpha. Each is triaged during the alpha:

- fixed, and the marker removed; or
- re-tagged `TODO(alpha)` with an owning issue, if it is deferred again; or
- recorded as accepted (a permanent, deliberate limitation), with the marker removed and the decision written down where it belongs; or
- `docs/backend-leakage.md` rows 11, 16 and 29 are triaged by appending a new row, never by editing them.

New unresolved points found during the alpha are tagged `TODO(alpha)`, not `TODO(phase0)`.

## What must not be cut

A1, A2, A5. Measurement, recovery robustness and the runtime decision are the point of the alpha; without them it produces an opinion instead of evidence.

## What is cut first

In this order, if the alpha needs to shed scope before A5:

1. Leftover `TODO(phase0)` items classified LOW during triage.
2. HTML entity decoding (`docs/grammar.md`, "HTML entities" row: not decoded, "may be added in a later phase").
3. `outou check --semantic` as a real subcommand (`crates/outou-cli/tests/ui.rs` — today the semantic/backend mapping machinery is test-only).
4. A3, the Phase 0 budget misses.
5. A4, the dogfood application. It is cut last because it supplies A5's evidence.

Items 1–3 are not gate work: they are picked up only once the gates are met, or not at all in the alpha if A5 is reached first. A cut gate (items 4–5) is recorded in `docs/alpha-results.md` as described under "If a gate cannot be met".

## Work items and issues

The initial work items — one for each gate plus the cross-cutting triage — are mirrored in `docs/alpha/issues/` as GitHub issues under the "Alpha" milestone, the same way Phase 0's issues are mirrored in `docs/phase0/issues/`. Scope changes during the alpha are kept in sync in both places.

## Working principles

Carried over from Phase 0, because they are not specific to feasibility work:

- Build the thing most likely to fail first, not the thing easiest to build. For the alpha that is measurement (A1) and recovery (A2), not the dogfood app.
- There is one compiler: `cargo build` and the language server call the same front end and codegen. The alpha does not create a second path.
- Unresolved points are written down as `TODO(alpha)` where they belong, not silently decided.
- Architectural changes still get an ADR (`docs/adr/`).
- The decision is made from a report, not from impressions.

Principles specific to feasibility work do not carry over: there is no time box to trade scope against. The alpha also does not begin the "known framework engineering" that `docs/phase0.md`'s "Working principles" places after Phase 0 ("A VDOM, hooks, a router and the rest"); those remain non-goals above.

## Decision report

The alpha produces `docs/alpha-results.md` with at least: the A1 measurements and fixed thresholds, the A2 recovery fixture results, the A3 budget-miss evidence, the A4 dogfood log and backend-leakage additions, and the A5 runtime decision with its ADR. Any gate recorded under "if a gate cannot be met" above is written up there too, with evidence. The decision is made from that report, not from impressions.
