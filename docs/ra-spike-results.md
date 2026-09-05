# rust-analyzer spike results

Outcome of the Week 1 feasibility spike (`spikes/rust-analyzer/`). Fill in every field; a blank field means the spike is not finished.

## Environment

| Item | Value |
|---|---|
| Date | |
| rust-analyzer version | |
| rustc / cargo version | |
| OS | |

## Strategy A, layout (b): `src/.generated/` + `#[path]`

| Criterion | Works? | Notes |
|---|---|---|
| 1. Cargo project loads normally | | |
| 2. Generated source is in the crate graph | | |
| 3. Editor changes reach rust-analyzer without `build.rs` | | |
| 4. Completion uses the latest buffer | | |
| 5. Hover uses the latest buffer | | |
| 6. Definition uses the latest buffer | | |
| 7. flycheck diagnostics map back to `.rsx` | | |

## Strategy A, layout (a): `OUT_DIR` + `include!`

| Criterion | Works? | Notes |
|---|---|---|
| 1. Cargo project loads normally | | |
| 2. Generated source is in the crate graph | | |
| 3. Editor changes reach rust-analyzer without `build.rs` | | |
| 4. Completion uses the latest buffer | | |
| 5. Hover uses the latest buffer | | |
| 6. Definition uses the latest buffer | | |
| 7. flycheck diagnostics map back to `.rsx` | | |

## Summary

| Question | Answer |
|---|---|
| Strategy A works / does not work | |
| Overlay works / does not work | |
| Completion works | |
| Hover works | |
| Definition works | |
| Diagnostics work | |
| Chosen layout: (a) OUT_DIR / (b) fixed path | |

## Known blockers

-

## Measured latency

| Request | Cold (ms) | Warm (ms) |
|---|---|---|
| initialize → first hover | | |
| hover | | |
| completion | | |
| definition | | |
| overlay change → updated hover | | |
| flycheck diagnostics | | |

## Decision

- Gate 0: PASS / STOP
- ADR 0009 status after this spike:
- If Strategy A failed, Strategy B spike planned for:
