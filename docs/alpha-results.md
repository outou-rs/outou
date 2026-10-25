# Alpha results

This document accumulates the alpha's decision report (`docs/alpha.md`, "Decision report"). This
revision covers **A1: measurement** (issue #17, `docs/alpha/issues/17-measurement.md`), **A2: recovery** (issues #18 and #19), **A3's forwarded-completion work** (issue #21), **A4: the dogfood application** (issue #22) and, last, **A5: the runtime decision** (ADR 0014, issues #25 to #30); later sections are appended
here rather than replacing this one.

Every number in (a), (c) and (d) is from the **settled re-measurement of 2026-10-11** (harness
fixed after review: samples are taken only once rust-analyzer has settled, RSS covers the whole
process tree and is shown next to macOS physical footprint, cold-init is timed from `initialize`
sent, and the RSS cold window ends on a settled session, not the first lucky hover). Machine state
for each run is in "Setup". (b) flycheck and (e) incremental regeneration are unchanged from the
**clean 2026-09-28 re-run** (the harness fixes do not affect them). The **first 2026-09-28 pass**
ran while a VS Code-owned rust-analyzer and one hung leftover rust-analyzer process were present on
the machine; that pass's numbers are not kept here (replaced, not appended) since a mixed-condition table would
misrepresent which numbers share a baseline.

## A1: measurement

### Setup

- **Workspaces measured**: `examples/phase0-app` (single crate, crate root + one module) and
  `tests/fixtures/workspace` (a two-member Cargo workspace, `app` + `ui-kit`). Every heavy
  (rust-analyzer-driving) measurement ran against a fresh temporary copy of the relevant fixture,
  built once via `outou build`, mirroring `crates/outou-lsp/tests/gate3.rs`'s own
  `fresh_copy`/`seed_build`. `coldinit`, `flycheck` and `completion` ran against
  `examples/phase0-app` only (its `main.rsx` is what those modes' cursor positions are anchored to);
  `rss` ran against **both** workspaces (against `tests/fixtures/workspace`'s `app` member, the
  crate with a `.rsx` root); the pure-Rust incremental-regeneration measurement ran against every
  planned unit of both workspaces directly.
- **Machine**: MacBookPro18,3 (Apple M1 Pro, 10 cores), 16 GiB RAM (`sysctl hw.model hw.memsize`).
- **OS**: macOS 26.6.2 (build 25G83).
- **Toolchain**: `rustc 1.98.1 (48a229cea 2026-09-01)`, `cargo 1.98.1 (797e8a9bc 2026-08-05)`.
- **rust-analyzer**: `rust-analyzer 1.98.1 (48a229ce 2026-09-01)` — the binary `outou-lsp` itself
  resolves via `OUTOU_RUST_ANALYZER`/`PATH` (`crates/outou-lsp/README.md`, "Running it"); the same
  binary was used for every "direct"/"plain" comparison session.
- **Node**: v24.14.0 (drives every heavy probe below, the same way `tests/gate3.rs` does).
- Every heavy run used a `CARGO_TARGET_DIR` shared across probes, outside the repository tree, so
  repeated cold-indexing runs were not each also paying to recompile `dioxus` and its dependency
  graph from scratch (`tests/gate3.rs`'s own `shared_cargo_target_dir` convention).
- **Build profile**: `outou-lsp` is the dev/debug binary (`CARGO_BIN_EXE_outou-lsp` under plain
  `cargo test`), which is also the profile the completion assertion runs in. `rust-analyzer` is the
  release binary shipped with the toolchain. Every magnitude below that involves `outou-lsp` (flycheck,
  cold-init, completion overhead, `outou-lsp`'s own RSS) is therefore a **debug-profile** magnitude.
  The settled re-measurement adds release `outou-lsp` completion numbers (`cargo test --release`)
  for information only.
- **Machine load and memory state, settled re-measurement (2026-10-11, ~45-57 minutes after a
  reboot, one run at a time; first measured run started 11:33 JST at uptime ~45 min, last ended
  11:45 JST at uptime ~57 min)**: `vm.swapusage` was `0.00M used` before and after
  every run (no swap, no memory compression pressure), no rust-analyzer or VS Code process was
  running, no Docker VM. The machine was not perfectly idle: macOS background services
  (WindowServer, iTerm2, Acronis agent, occasional `mediaanalysisd`/`duetexpertd` bursts) kept the
  1-minute load average at about 3-5 throughout, and part of the "after" value is the measurement
  itself (a rust-analyzer cold index pegs a core or two). Each run was started only when
  load1 <= 4, no rust-analyzer/VS Code process existed and no process used > 80% CPU; every run
  below passed that check on first try ("clean" = yes). Load averages are 1/5/15 min, before ->
  after (from the harness's `machineState`).

  | run | load before -> after | swap used before / after | clean pre-check |
  |---|---|---|---|
  | completion N=50, debug | 3.71/5.49/11.59 -> 3.93/5.30/11.20 | 0 / 0 | yes |
  | completion N=50, release | 3.58/5.15/11.04 -> 3.62/5.00/10.72 | 0 / 0 | yes |
  | rss phase0-app single #1 | 3.41/4.93/10.66 -> 5.32/5.32/10.70 | 0 / 0 | yes |
  | rss phase0-app two-process #1 | 3.18/4.65/9.84 -> 3.05/4.55/9.72 | 0 / 0 | yes |
  | rss workspace single #1 | 3.13/4.54/9.68 -> 3.34/4.54/9.62 | 0 / 0 | yes |
  | rss workspace two-process #1 | 3.15/4.48/9.57 -> 3.13/4.41/9.46 | 0 / 0 | yes |
  | rss phase0-app single #2 | 3.19/4.40/9.42 -> 4.15/4.57/9.42 | 0 / 0 | yes |
  | rss phase0-app two-process #2 | 3.90/4.51/9.37 -> 5.21/4.79/9.39 | 0 / 0 | yes |
  | rss workspace single #2 | 3.89/4.81/8.79 -> 4.94/5.00/8.81 | 0 / 0 | yes |
  | rss workspace two-process #2 | 3.74/4.69/8.55 -> 4.04/4.72/8.49 | 0 / 0 | yes |
  | rss phase0-app single #3 | 3.80/4.59/8.31 -> 5.52/4.96/8.38 | 0 / 0 | yes |
  | rss phase0-app two-process #3 | 3.62/4.51/7.96 -> 3.95/4.55/7.91 | 0 / 0 | yes |
  | rss workspace single #3 | 3.95/4.55/7.91 -> 3.89/4.51/7.84 | 0 / 0 | yes |
  | rss workspace two-process #3 | 3.89/4.51/7.84 -> 4.48/4.61/7.80 | 0 / 0 | yes |
  | coldinit N=5 | 3.79/4.43/7.62 -> 3.71/4.37/7.52 | 0 / 0 | yes |

  The clean 2026-09-28 re-run (flycheck (b), incremental (e)) did not record load or swap; it was checked
  with `ps` only. The 2026-10-11 code session that preceded this pass saw load 17.8 / 8.8 / 8.1
  and 11.2 of 12 GiB swap used, which is why nothing was measured then.

- **Machine state, verified before every run below**: `ps -eo pid,comm | grep -i rust-analyzer`
  empty, and no VS Code / other editor process running. No other rust-analyzer instance or editor
  ran concurrently with any measurement in this document. Compilation (debug and release test
  binaries, `outou-cli`) was finished before the first measurement, so no build overlapped one.

### Tooling

- **`spikes/rust-analyzer/client/measure-a1.mjs`**: the measurement harness, a JSON-RPC-over-stdio
  driver in the style of `spikes/rust-analyzer/client/{outou-lsp-client,gate3-overhead}.mjs` (Content-
  Length framing, `window.workDoneProgress` capability, `publishDiagnostics` tracking reused rather
  than reimplemented a third time). Four modes, one process per invocation, each printing one JSON
  document to stdout:
  - `coldinit` — cold `initialize` → first usable hover/completion/definition, N fresh processes.
  - `flycheck` — `didSave` → mapped semantic diagnostic latency, N repeated introduce/revert cycles
    on a warm session, `outou-lsp` vs. a plain rust-analyzer session on the same generated file
    (the same same-session A/B idea as `docs/gate3-results.md`'s "Proxy overhead (S8)", applied to
    flycheck instead of hover/definition/completion).
  - `completion` — the same same-session A/B method, run against a fixed list of measurement-only
    "sites" injected at runtime into the fresh temp copy (`COMPLETION_SITES`, never into a
    checked-in fixture), each engineered to make rust-analyzer's own candidate-list size land in a
    different rough order of magnitude — see (d) below.
  - `rss` — sampling every `--interval-ms` (default 500 ms) of one or two long-lived sessions
    (`--config single|two-process`). Each measured entry is the **whole process tree** under its root
    pid (`rust-analyzer-proc-macro-srv`, transient `cargo`/`rustc`), from one `ps -A` snapshot per
    sample: summed `ps` RSS next to the summed macOS **physical footprint** (`footprint(1)`), because
    `ps` RSS drops when macOS compresses a process's pages and footprint does not. `--phase cold`
    samples from spawn until both sessions have **settled** (same criterion for every session: a
    useful hover at an anchor that is identical for 5 requests over at least 2 s; a direct
    rust-analyzer session additionally waits for `experimental/serverStatus` quiescent, which
    `outou-lsp` does not forward). `--phase cold+edit` continues in the *same process* with
    `--edits` scripted edits and reports `windows.cold` and `windows.edit` separately. The anchor is
    `load_user` in `examples/phase0-app` and `Card` in `use ui_kit::{Button, Card};` in
    `tests/fixtures/workspace/app`, which only resolves once the `ui-kit` dependency is loaded.
  - Settling (all modes that sample after a warm-up): `completion` waits, per session, for a
    candidate count whose trailing run of identical answers has at least 5 requests and spans at least 2 s (direct: after
    `experimental/serverStatus` quiescent; `outou-lsp`: also equal to the direct session's settled
    count, otherwise it is sampled anyway and the bucket is reported as disagreeing). Both have a
    timeout with an explicit error.
  - `coldinit` starts its clock when `initialize` is *sent* (`outou-lsp` loads the workspace and
    handshakes with rust-analyzer before it replies) and reports cumulative time to first usable
    hover / completion / definition plus `totalMs`, alongside per-step times.
  - Every JSON result carries `machineState` (load average, swap) before and after the run.
- **`crates/outou-lsp/tests/measure_a1.rs`**: the orchestrator, in the style of `tests/gate3.rs`
  (its own `fresh_copy`/`seed_build`/`shared_cargo_target_dir` helpers are duplicated in miniature
  here rather than factored into a shared module — `tests/gate3.rs` also deliberately corrupts its
  copies mid-probe, and coupling that pairing to a shared module was judged not worth it for a
  handful of small helper functions). One `#[test]` (not `#[ignore]`d — pure Rust, no
  rust-analyzer, safe under the default `cargo test --workspace` budget) plus five `#[ignore]`d
  tests, one per heavy mode:

  ```text
  cargo test -p outou-lsp --test measure_a1 -- --ignored --nocapture measure_a1_coldinit
  cargo test -p outou-lsp --test measure_a1 -- --ignored --nocapture measure_a1_flycheck
  cargo test -p outou-lsp --test measure_a1 -- --ignored --nocapture measure_a1_completion_overhead
  # optional: MEASURE_A1_COMPLETION_N=50 MEASURE_A1_COMPLETION_SITE=large-enum-path (same test)
  cargo test -p outou-lsp --test measure_a1 -- --ignored --nocapture measure_a1_rss_single
  cargo test -p outou-lsp --test measure_a1 -- --ignored --nocapture measure_a1_rss_two_process
  ```

  The `rss` tests default to `examples/phase0-app`. `MEASURE_A1_FIXTURE=workspace` makes them copy
  `tests/fixtures/workspace` whole, rewrite the `outou` path dependency in **both** `app/Cargo.toml`
  and `ui-kit/Cargo.toml`, run the `outou build` step for both members and measure `app`. Each rss
  test runs one process per config (`--phase cold+edit`) and writes nothing into the repository.
  `MEASURE_A1_COMPLETION_N` (positive integer) and `MEASURE_A1_COMPLETION_SITE` (non-empty label) are
  validated before anything is spawned, as are the harness's own flags.

  The heavy tests need `node` and a rust-analyzer binary; they only run when asked for with
  `--ignored`, so a missing prerequisite **panics** rather than returning a pass that looks like a
  measurement. Raw JSON is **not** kept in the repository: a run prints its result and writes it to a
  system temp directory (or into `spikes/rust-analyzer/results/` if `MEASURE_A1_SAVE_ARTIFACTS=1` is
  set; those `measure-a1-*.json` files are gitignored), and the tables in this document are the record. Never run two of these at once, and never
  alongside `cargo clippy`/`cargo test` — CPU contention skews every number. Every heavy job in the
  original passes was run one at a time.
- **Why an `#[ignore]`d test binary over an `xtask` subcommand**: every heavy mode needs the
  just-built `outou-lsp` binary's path, which `env!("CARGO_BIN_EXE_outou-lsp")` gives a test binary
  for free; an `xtask` subcommand would have to locate the debug binary by convention instead. The
  fresh-copy/seed-build machinery also already exists in this exact shape in `tests/gate3.rs`, so a
  sibling test file reuses that pattern directly rather than inventing an `xtask`-flavored version of
  it.

### (a) Two-process resource cost: RSS and CPU, `--config single` vs. `--config two-process`

Method: `rss` mode, `--phase cold+edit`, `--interval-ms 500`, one process per config, 3 repeats per
cell (2026-10-11, debug `outou-lsp`, machine state in "Setup"). `single` is one plain rust-analyzer
session on the fixed workspace (`src/.generated/crate-root.rs`, the exact file `outou-lsp` opens as
its own overlay). `two-process` is `outou-lsp` (spawning its own rust-analyzer child, found via
`pgrep -P`) plus a *second*, independent plain rust-analyzer session on the same workspace —
standing in for the editor's own rust-analyzer session against the same crate
(`crates/outou-lsp/README.md`, "Two processes, not one"). Each entry is the **whole process tree**
(rust-analyzer, `rust-analyzer-proc-macro-srv`, transient `cargo`/`rustc`). The **cold** window
runs from spawn until every session has settled (see "Tooling"); the **edit** window continues in
the *same process* with 10 scripted edits (append/remove a trailing comment) 500 ms apart, saving to
disk every fifth edit to also exercise `checkOnSave`. All figures are the **peak over the window**
in MiB, as **median of 3 repeats [min-max]**; CPU is the tree's summed cumulative CPU seconds in
the window. "Footprint" is macOS physical footprint (`footprint(1)`, counts compressed pages) and
is the figure for "how much memory does this hold"; "RSS" is summed `ps` RSS, shown for
completeness (it is noisier between repeats and includes mapped pages that the footprint does not
attribute in the same way). No swap was in use (see "Setup"), so RSS was not compression-collapsed
this time.

**`examples/phase0-app` — cold indexing (to settled):**

| | peak footprint | peak RSS | CPU time |
|---|---|---|---|
| `single`: plain rust-analyzer | 906 [867-915] | 1000 [965-1238] | 5.71 s [5.58-5.73] |
| `two-process`: `outou-lsp`'s rust-analyzer child | 837 [808-889] | 903 [890-944] | 6.01 s [5.98-6.14] |
| `two-process`: plain rust-analyzer (editor stand-in) | 831 [791-836] | 959 [957-1124] | 5.40 s [5.37-5.49] |
| `two-process`: `outou-lsp` itself | 2 | 5 | 0.06 s [0.06-0.07] |
| **`two-process` total** (child + editor stand-in; excludes `outou-lsp` itself) | **1523 [1483-1680]** | **1891 [1862-2014]** | **11.41 s [11.34-11.63]** |

**`examples/phase0-app` — scripted editing session (10 edits, 2 saves; same process, after cold):**

| | peak footprint | peak RSS | CPU time |
|---|---|---|---|
| `single`: plain rust-analyzer | 545 [538-547] | 958 [944-1010] | 0.15 s [0.15-0.18] |
| `two-process`: `outou-lsp`'s rust-analyzer child | 565 [551-618] | 1001 [924-1020] | 0.23 s [0.20-0.33] |
| `two-process`: plain rust-analyzer (editor stand-in) | 611 [603-617] | 894 [768-1010] | 0.26 s [0.24-0.35] |
| `two-process`: `outou-lsp` itself | 2 | 5 | 0.03 s |
| **`two-process` total** | **1174 [1154-1180]** | **1819 [1645-2004]** | **0.47 s [0.46-0.54]** |

**`tests/fixtures/workspace` `app` — cold indexing (to settled; anchor `Card` from `ui_kit`):**

| | peak footprint | peak RSS | CPU time |
|---|---|---|---|
| `single`: plain rust-analyzer | 879 [844-898] | 980 [952-1013] | 5.24 s [5.15-5.26] |
| `two-process`: `outou-lsp`'s rust-analyzer child | 862 [823-876] | 941 [934-995] | 5.55 s [5.54-5.65] |
| `two-process`: plain rust-analyzer (editor stand-in) | 841 [805-867] | 982 [969-1014] | 4.95 s [4.76-5.00] |
| `two-process`: `outou-lsp` itself | 2 | 5 | 0.05 s [0.05-0.06] |
| **`two-process` total** | **1552 [1440-1619]** | **1935 [1858-1955]** | **10.41 s [10.29-10.65]** |

**`tests/fixtures/workspace` `app` — scripted editing session (10 edits, 2 saves; same process, after cold):**

| | peak footprint | peak RSS | CPU time |
|---|---|---|---|
| `single`: plain rust-analyzer | 537 [535-538] | 955 [937-1095] | 0.11 s [0.11-0.25] |
| `two-process`: `outou-lsp`'s rust-analyzer child | 560 [543-662] | 994 [857-1080] | 0.17 s [0.11-0.31] |
| `two-process`: plain rust-analyzer (editor stand-in) | 535 [534-539] | 893 [763-999] | 0.18 s [0.10-0.21] |
| `two-process`: `outou-lsp` itself | 2 | 5 | 0.01-0.02 s |
| **`two-process` total** | **1095 [1082-1196]** | **1972 [1620-1993]** | **0.38 s [0.21-0.49]** |

Findings:

- **Headline: the two-process setup costs about 1.7-2.2x the physical footprint and about 2x the
  CPU of a single plain rust-analyzer session, on both workspaces.** Cold: footprint 1523 vs 906 MiB
  (1.68x) on `phase0-app`, 1552 vs 879 MiB (1.77x) on `workspace`; CPU 11.4 vs 5.7 s (2.0x) and
  10.4 vs 5.2 s (2.0x). Edit window: footprint 1174 vs 545 MiB (2.15x) and 1095 vs 537 MiB (2.04x).
  Expected, since it is architecturally two independent rust-analyzer instances doing independent
  full analysis of the same crate (`crates/outou-lsp/README.md`, "Two processes, not one"). In
  absolute terms: about 1.1 GiB resident footprint at steady state (2x ~550 MiB) and about
  1.5 GiB at the cold peak, for the pair. The two-process total is higher than the single session
  in every repeat on both workspaces (no inversion; the previous harness's `workspace` rows, where
  the two-process total was *below* the single peak, were artefacts of its racy windows).
- **`outou-lsp`'s own process is negligible**: ~5 MiB RSS / ~2 MiB footprint, under 0.07 s CPU per
  window (debug build).
- **Both workspaces cost about the same.** The earlier report of `tests/fixtures/workspace` being
  4-8x smaller than `phase0-app` was an artefact of the old cold window ending on a hover at
  `fn main()`, before the `ui-kit` dependency (and `dioxus`) was loaded. With the cold window ending
  on a settled session anchored on `Card` from `ui_kit`, both are dominated by the same `dioxus`
  dependency graph (single cold footprint 879-906 MiB, edit 537-545 MiB).
- **The cold peak is transient.** Footprint falls from ~900 MiB (cold, single) to ~540 MiB in the
  edit window; the cold peak includes `proc-macro-srv`/build-script work and index construction.
  Peak **RSS** does not fall the same way (958 vs 1000 MiB) and varies widely between repeats
  (e.g. `phase0-app` single cold 965-1238), so peak footprint is the figure to compare. Treat the
  RSS columns as secondary.
- Figures in earlier drafts of this document no longer apply: the first-pass figures (665 MiB single
  / 1.15 GiB two-process) counted only the rust-analyzer processes themselves and used `ps` RSS on a
  swapping machine; they were lower bounds.
- Sampled at 500 ms; a shorter-lived spike between samples would not be captured. The cold window
  has only 15-21 samples per run. CPU is the process tree's summed cumulative CPU time; a transient
  child that exits stops contributing, so it can undercount slightly. Three repeats give a
  median and a range, not a distribution.

### (b) Flycheck latency: `didSave` → mapped semantic diagnostic

These numbers are for the dev/debug `outou-lsp`. Method: `flycheck` mode, N=10, warm target dir (the shared `CARGO_TARGET_DIR`, already populated by
prior runs), `examples/phase0-app` only. Each sample introduces a type error (`let user =
load_user();` → `let user: i32 = load_user();`), times `didSave` → the first `publishDiagnostics`
containing `"mismatched types"`, then reverts and waits for that diagnostic to actually clear before
the next sample (so a stale diagnostic from the revert cannot be mistaken for the next introduction;
the harness now throws if the diagnostic does not clear, instead of continuing).
Compared side by side: through `outou-lsp` (on `main.rsx`, the real editor path) vs. a plain
rust-analyzer session, same crate, saving the already-generated `src/.generated/crate-root.rs`
directly (the "plain `.rs` save" comparison the issue asks for).

| | median | max | samples (ms) |
|---|---|---|---|
| `outou-lsp` (`.rsx` save) | 711.5 ms | 1129 ms | 661, 661, 711, 1129, 1067, 611, 611, 715, 763, 712 |
| plain rust-analyzer (`.rs` save) | 712 ms | 1271 ms | 612, 1271, 815, 1170, 610, 660, 713, 711, 713, 711 |

Both sides are sampled by polling every 50 ms, so the resolution is 50 ms; the 0.5 ms median
difference (`outou-lsp` 711.5 ms vs. 712 ms) is far below it and means nothing beyond "no measurable
difference". Outou's added part (re-planning the crate from disk, writing every unit through the
Strict path, and forwarding the save) is within noise here — flycheck latency
is dominated by rust-analyzer's own `cargo check` invocation on this crate's dependency graph, not by
`outou-lsp`'s own bookkeeping. Consistent with the first pass (median ~712-772 ms both times) and
with `docs/phase0-results.md` §6's earlier, less formal figure ("4.7-8.9 seconds end to end" for the
Gate 3 probes, dominated by *cold* indexing state rather than a warm flycheck's own cost).

### (c) Cold `initialize` → first usable hover/completion/definition

Method: `coldinit` mode, N=5, `examples/phase0-app` only, debug `outou-lsp`, 2026-10-11 (machine
state in "Setup"). One fresh `outou-lsp` process per run (with a freshly spawned rust-analyzer
child each time), sequentially requesting hover, then completion, then definition, each retried
until a useful answer arrives. The clock starts when `initialize` is *sent* (`outou-lsp` loads the
workspace and handshakes with rust-analyzer before it replies, so that work is included). Values
are cumulative time since `initialize` sent.

| | median | max | samples (ms, runs 0-4) |
|---|---|---|---|
| `initialize` response | 48 ms | 525 ms | 525, 42, 48, 47, 48 |
| first usable hover (cumulative) | 3771 ms | 4266 ms | 4266, 3771, 3740, 3750, 3790 |
| first usable completion (cumulative) | 3886 ms | 4384 ms | 4384, 3886, 3865, 3867, 3910 |
| first usable definition (cumulative) = `totalMs` | 3887 ms | 4385 ms | 4385, 3887, 3866, 3867, 3911 |

Per step (time added after the previous step): hover 3728 ms median (max 3741), completion 118 ms
(max 125), definition 1 ms (max 1).

The cost is the first hover: `outou-lsp` answers `initialize` in ~48 ms (a first-run outlier of
525 ms), then the first useful hover arrives ~3.7 s after `initialize` was sent, which is
rust-analyzer's initial indexing (with the shared, already-populated `CARGO_TARGET_DIR`, this is
indexing/analysis time, not a cold `cargo build` of `dioxus`). Completion and definition, sent
right after in the same already-warm session, add ~120 ms and ~1 ms. The five runs are tight apart
from run 0 (+500 ms, the first process start after the build; likely a cold filesystem/page cache
for the binaries).

### (d) Forwarded-completion overhead per candidate-list-size bucket

> **Profile and settling.** The asserted numbers are for the **debug** `outou-lsp`
> (rust-analyzer is the release binary on both sides); the release `outou-lsp` is shown for
> information only. Both were measured on 2026-10-11 by the settled harness: each session first
> waits for `experimental/serverStatus` quiescent (direct) and then for a candidate count that is
> stable for >= 5 consecutive requests spanning >= 2 s, and the `outou-lsp` session must also reach
> the direct session's settled count. Earlier passes (the first 2026-09-28 pass at N=10, and the unsettled N=50
> pass kept further below as history) started sampling as soon as the first non-empty answer
> arrived; their candidate counts (3 / 122 / 1200) and the multi-second stalls came from that. The
> count flip 3 -> 5 (tiny type) and 1200 -> 1204 (large enum) is rust-analyzer making the
> blanket-impl methods of `core` visible once its analysis finishes; settled, every session reports
> 5 / 122 / 1204.

Method: `completion` mode, N=50, same-session A/B (`docs/gate3-results.md`, "Proxy overhead (S8)"),
`examples/phase0-app` only, against three measurement-only sites injected at runtime into the fresh
copy's `main.rsx` (and the equivalent plain-Rust text in the already-generated
`src/.generated/crate-root.rs`) — never into the checked-in fixture:

- **`tiny-local-type`**: a new 3-method local type (`struct __MeasureTiny; impl __MeasureTiny { fn
  alpha/beta/gamma(&self) {} }`), completed on `__measure_tiny.`.
- **`option-member-access`**: the existing `user.` (`Option<User>`) member-access site (same as the
  first pass's only bucket).
- **`large-enum-path`**: a new 1200-variant local enum (`enum __MeasureMega { V0, ..., V1199 }`),
  completed on `__MeasureMega::` — every earlier attempt at a large list (a longer identifier prefix
  on the member-access site; a single-character prefix at an unqualified expression position; a
  single-character prefix at a type-ascription position, each with `completion.autoimport.enable`
  explicitly set) plateaued at 26-42 candidates regardless of method, which looks like this
  rust-analyzer version capping its own import/flyimport suggestion count well under 1000 — a large
  *local* enum sidesteps import search entirely, since path completion on an already-resolved local
  type is not subject to that cap.

Each sample is retried (up to 4 attempts in total) if it comes back with 0 candidates (observed once,
transiently, for the large-enum site in an earlier run of this measurement). The harness now records
the number of attempts per sample and lists the retried samples (0-based index and attempt count) in
its output and in the test's printout, so a retry is visible instead of absorbed; see
`measureCompletionSamples` in `measure-a1.mjs`.

Settled results, N=50 per bucket and side, one fresh session pair per bucket, no retried samples
(0 empty answers), candidate counts equal in both sessions and constant across all 50 samples in
every bucket. Times in ms; "overhead" = `outou-lsp` minus direct, on medians and on p95s.
`settleMs` (time the session needed to settle) was 2.4-2.6 s for the direct session and
5.5-5.6 s for `outou-lsp` in all six runs.

**Debug `outou-lsp` (the asserted profile; PASSED):**

| bucket | candidates | `outou-lsp` median / p95 / max | direct median / p95 / max | overhead median | overhead p95 | ceiling (5 ms + 0.15 ms x n) | margin |
|---|---|---|---|---|---|---|---|
| `tiny-local-type` | 5 | 3 / 6 / 9 | 2 / 5 / 12 | 1 | 1 | 5.75 | 5.8x |
| `option-member-access` | 122 | 16 / 18 / 20 | 4 / 9 / 14 | 12 | 9 | 23.3 | 1.9x (tightest) |
| `large-enum-path` | 1204 | 57 / 59 / 60 | 8 / 11 / 20 | 49 | 48 | 185.6 | 3.8x |

**Release `outou-lsp` (`cargo test --release`; information only, no assertion):**

| bucket | candidates | `outou-lsp` median / p95 / max | direct median / p95 / max | overhead median | overhead p95 | debug ceiling / release median overhead |
|---|---|---|---|---|---|---|
| `tiny-local-type` | 5 | 2 / 3 / 5 | 2 / 3 / 4 | 0 | 0 | n/a (0 ms) |
| `option-member-access` | 122 | 5 / 11 / 13 | 4 / 8 / 12 | 1 | 3 | 23x |
| `large-enum-path` | 1204 | 14 / 16 / 22 | 8 / 12 / 18 | 6 | 4 | 31x |

Findings:

- **The debug threshold held in every bucket.** Median overhead 1 / 12 / 49 ms against ceilings of
  5.75 / 23.3 / 185.6 ms. The tightest margin is the middle bucket (12 of 23.3 ms, 1.9x), which
  is also the bucket where the fitted formula over-predicts least; the large bucket has 3.8x
  headroom and the tiny one 5.8x. Counts agreed and were stable in all three buckets.
- **Overhead grows with candidate-list size, roughly linearly, in both profiles** (debug: ~1 ms at
  5, 12 ms at 122, 49 ms at 1204, i.e. about 0.04 ms/candidate; release: 0 / 1 / 6 ms, about
  0.005 ms/candidate). Consistent with `docs/gate3-results.md`'s "Proxy overhead (S8)" explanation
  that the per-item work in `crate::response::map_completion_response` scales with the list. These
  numbers do not identify which per-item step costs what. In release the overhead is ~8x smaller
  than in debug, so the assertion's bound is about 31x loose at 1204 candidates and **would not catch
  a release regression**: A3 (#21) must judge completion-path optimisation on release numbers.
- **The multi-second `large-enum-path` stall did not occur on settled sessions.** Debug max was 60
  ms (`outou-lsp`) / 20 ms (direct); release max 22 / 18 ms. This supports the earlier diagnosis
  (see the unsettled pass below) that the ~2.4 s stalls came from rust-analyzer's late
  re-analysis landing inside the sample window, not from `outou-lsp`. It is one settled session per
  bucket and profile (50 samples each), so it is consistent with the stall not being triggered once settled (2 settled
  sessions only), not that it can never happen. Still no p95/tail threshold is derived from this.
- Medians and p95s are from 50 samples; p95 is the 3rd-largest value, so the p95 and max columns
  are close to the same one or two samples.

#### (d) Earlier N=50 pass, unsettled harness (2026-10-11; historical, kept for the stall finding)

**Superseded as a baseline by the settled tables above** (candidate counts, medians and overheads
below include unsettled sampling). Kept because it is the evidence that the multi-second stall is
rust-analyzer's late re-analysis rather than `outou-lsp`'s work. A pass (2026-10-11, same Apple M1 Pro; unsettled harness; only a `ps` check for
rust-analyzer/VS Code was recorded beforehand, no load or swap, and the same day's earlier session
had about 11.2 GiB of swap in use) re-ran `completion` mode at **N=50** (`MEASURE_A1_COMPLETION_N=50`; the harness also
gained `--site <label>` / `MEASURE_A1_COMPLETION_SITE` to restrict a run to one bucket). Method otherwise the pre-settle method (no settling wait; the settled table above differs in that), with a fresh
temp copy and fresh sessions per run. Run 1 below is a `large-enum-path`-only run; the all-bucket run is
one run of all three buckets; runs 2-4 are further `large-enum-path`-only runs, each with fresh
sessions, to test repeatability.

All-bucket run, N=50 (medians in ms, sample counts > 1 s all 0):

| bucket | candidates | `outou-lsp` median / p95 / max | direct median / p95 / max | overhead median |
|---|---|---|---|---|
| `tiny-local-type` | 5 | 4 / 8 / 9 | 4 / 14 / 16 | 0 ms |
| `option-member-access` | 122 | 18 / 36 / 61 | 5 / 6 / 6 | 13 ms |
| `large-enum-path` | 1204 | 60 / 70 / 131 | 8.5 / 11 / 12 | 51.5 ms |

(The tiny bucket reports 5 candidates here against 3 in the first 2026-09-28 pass; the same site text
was used, so this is rust-analyzer-side: the extra two are the blanket-impl `into`/`try_into`
methods, visible only once analysis has settled. The sessions did not always agree on the count even
within a pass: a later verification run saw `outou-lsp` report 5 where the direct session reported 3
in the tiny bucket. The settled harness now fails a bucket in that case.)

`large-enum-path` only, four N=50 runs (200 samples per side), sorted, in ms:

- Run 1, `outou-lsp` (1200 cand.): median 59, p95 72, max 199, >1 s: 0. `57 57 57 57 58 58 58 58 58 58
  58 58 58 58 58 58 58 58 58 58 58 58 58 59 59 59 59 59 59 59 59 59 60 61 61 61 61 61 62 62 63 63 63 63
  65 67 68 72 76 199`. Direct: median 7, p95 12, max 99, >1 s: 0. `6 6 7 7 7 7 7 7 7 7 7 7 7 7 7 7 7 7 7
  7 7 7 7 7 7 7 7 8 8 8 8 8 8 8 8 8 8 9 9 9 10 11 11 11 11 12 12 12 30 99`.
- Run 2, `outou-lsp` (1200 then 1204): median 59, p95 65, max **2410**, >1 s: **1**. `57 58 58 58 58 58
  58 58 58 58 59 59 59 59 59 59 59 59 59 59 59 59 59 59 59 59 59 60 60 60 60 60 60 60 60 60 60 60 61
  61 61 62 63 63 63 64 64 65 65 2410`. Direct (1200 then 1204): median 8, p95 28, max **2443**, >1 s:
  **1**. `7 7 7 7 7 7 8 8 8 8 8 8 8 8 8 8 8 8 8 8 8 8 8 8 8 8 8 8 8 8 8 8 9 9 9 9 9 9 9 9 9 9 9 10 10 11
  11 28 76 2443`.
- Run 3, `outou-lsp` (1204): median 59, p95 64, max 69, >1 s: 0. `57 57 58 58 58 58 58 58 58 59 59 59 59
  59 59 59 59 59 59 59 59 59 59 59 59 59 59 59 59 60 60 60 60 60 60 60 61 61 61 61 61 61 61 62 62 63
  63 64 64 69`. Direct: median 8, p95 10, max 11, >1 s: 0. `7 7 8 8 8 8 8 8 8 8 8 8 8 8 8 8 8 8 8 8 8
  8 8 8 8 8 8 8 9 9 9 9 9 9 9 9 9 9 9 9 9 9 9 9 9 10 10 10 10 11`.
- Run 4, `outou-lsp` (1200 then 1204): median 59, p95 65, max **2448**, >1 s: **1**. `58 58 58 58 58
  58 58 58 58 58 58 58 58 58 58 58 58 58 59 59 59 59 59 59 59 59 59 59 59 59 59 59 59 60 60 60 60 60
  60 60 61 61 62 62 62 62 65 65 69 2448`. Direct (1204): median 8.5, p95 11, max 13, >1 s: 0. `8 8 8 8
  8 8 8 8 8 8 8 8 8 8 8 8 8 8 8 8 8 8 8 8 8 9 9 9 9 9 9 9 9 9 9 9 9 9 9 9 9 9 9 10 10 10 11 12 13`.
- All-bucket run's `large-enum-path`: see the table above (`outou-lsp` max 131, >1 s: 0; direct max 12).

Pooled over the five N=50 `large-enum-path` runs (250 samples per side): `outou-lsp` has **2**
samples over 1 s (2410 and 2448 ms), direct has **1** (2443 ms); every other sample in both sessions
is under 200 ms. Medians: `outou-lsp` 59-60 ms, direct 7-8.5 ms; overhead medians 50.5-52 ms.

Findings:

- **The multi-second stall is repeatable, but it is not specific to `outou-lsp`.** It reproduced in 2
  of 4 fresh `large-enum-path` sessions through `outou-lsp` (about 2.4 s each, smaller than the earlier
  pass's ~4 s) and in 1 of 4 fresh direct rust-analyzer sessions (2443 ms) — i.e. also in the
  session with no `outou-lsp` in the path. The first 2026-09-28 pass's 3994 ms is consistent with the
  same phenomenon; its "vs. 10 ms direct" comparison was a different session's tail, not evidence
  that the proxy adds seconds.
- **It coincides with a rust-analyzer-side state change, not with request volume.** In the stalled
  sessions the candidate count changes from 1200 to 1204 right at the stall (sample indices are
  0-based): direct run 2, sample 17
  (2443 ms, the first 1204-candidate answer); `outou-lsp` run 2, sample 1 (2410 ms, first 1204); `outou-lsp` run 4,
  sample 1 (2448 ms, first 1204). The harness's warm-up only waited for a non-empty answer, not for
  rust-analyzer to go quiescent, so a later background event (most likely a late build-script/
  proc-macro/`cargo check` result invalidating the analysis and forcing a re-analysis of the
  1200-variant enum, which is not itself ruled out here) lands inside the sampling window. Sessions that had already
  settled on 1204 before sampling (run 3, the all-bucket run) show no stall. The 1200 → 1204 change is
  `core`'s blanket-impl methods (`from`/`into`/`try_from`/`try_into`) becoming visible on the enum,
  which is what a late completion of rust-analyzer's analysis looks like from the outside.
- **`outou-lsp`'s own work is not the multi-second cost.** `map_completion_response`
  (`crates/outou-lsp/src/response.rs`) is a single linear pass over the items (a cursor-containment
  check, a leak check, a `textEdit` remap and two `remove`s per item); the steady-state overhead is
  a flat ~50 ms at 1200 items (0.04 ms/item), with `outou-lsp`'s p95 within ~15 ms of its median in the
  runs without a stall. No timing instrumentation was added to `outou-lsp`: the direct-session
  reproduction already localizes the stall to rust-analyzer, and the evidence is purely
  black-box (harness-side timings), so the *exact* rust-analyzer event was not identified.
- **Consequence for A3 (#21):** the stall is a rust-analyzer behavior a client can hit just after
  startup/first-edit, so the product question is mostly about what the first completions after
  open cost (e.g. whether `outou-lsp` should wait for/show readiness), not a leak-scan or mapping
  bug. Remaining unknown: the exact trigger. Reproducing it with rust-analyzer's own
  `RA_LOG`/`--log-file` on a larger set of sessions would pin it down. Items deferred to A3 (#21):
  pin down the stall trigger with `RA_LOG`; decide whether `outou-lsp` should forward
  `experimental/serverStatus` or otherwise show readiness (today it forwards only
  `publishDiagnostics` and `$/progress`, which is also why the harness has to settle on the answer
  itself for the `outou-lsp` side); and judge any completion-path optimisation on **release**
  numbers. The `outou build` CLI-path FAIL in (f) belongs to #20, not #21.

### (e) Incremental `.rsx` → generated Rust distribution

Method: `incremental_regeneration_distribution_on_both_workspaces` (not `#[ignore]`d — pure Rust, no
subprocess), N=40 per unit, calling `outou_cli::build::emit::generate_unit` directly on every planned
unit of both fixed workspaces. That is the unit-generation step of `Workspace::regenerate`, not all of
it (`regenerate` also re-parses for `declared_modules`, builds a `LineIndex` and updates the
registry), so these numbers are a lower bound on a whole edit's regeneration time. The test now
asserts the **p95** per unit (nearest rank) against 10 ms and prints the max without asserting it, so
one scheduler hiccup under concurrent compile load cannot fail it.

| workspace / unit | median | p95 | max |
|---|---|---|---|
| `examples/phase0-app` / `main.rsx` | 0.32 ms | 0.43 ms | 0.46 ms |
| `examples/phase0-app` / `components.rsx` | 0.27 ms | 0.30 ms | 0.39 ms |
| `tests/fixtures/workspace` `app` / `main.rsx` | 0.09 ms | 0.09 ms | 0.11 ms |
| `tests/fixtures/workspace` `ui-kit` / `lib.rsx` | 0.17 ms | 0.19 ms | 0.22 ms |
| `tests/fixtures/workspace` `ui-kit` / `widgets.rsx` | 0.07 ms | 0.08 ms | 0.08 ms |
| `tests/fixtures/workspace` `ui-kit` / `button.rsx` | 0.05 ms | 0.06 ms | 0.10 ms |
| `tests/fixtures/workspace` `ui-kit` / `extra.rsx` | 0.05 ms | 0.06 ms | 0.06 ms |
| `tests/fixtures/workspace` `ui-kit` / `circle.rsx` | 0.06 ms | 0.07 ms | 0.09 ms |

The worst observed value across every unit in both workspaces is 0.46 ms (`main.rsx`, the largest
unit measured) — actually *lower* than the first pass's 1.68 ms for the same unit (both comfortably
under the fixed 10 ms threshold; the difference is normal run-to-run noise for a sub-millisecond
measurement, not a regression). The 1.68 ms figure came from the earlier mixed-condition pass whose
numbers are otherwise discarded. The table above is the record; raw samples are not kept in the
repository.

### (f) "A single-file edit never regenerates the whole crate"

The criterion is interpreted on the **write side** (file writes and mtimes), the harm that was measured: PASS below means unchanged units' generated files are not rewritten, not that their codegen is skipped.

- **Language-server path: PASS.** `regenerating_one_unit_does_not_touch_another`
  (`crates/outou-lsp/src/documents/workspace/tests.rs`) re-verified passing (re-run for this
  report, alongside `regenerating_one_unit_is_fast` under its new 10 ms threshold — both pass).
  `components.rs`'s generated text is byte-identical before and after editing `main.rsx`.
- **`outou build` CLI path: FAIL at A1 (baseline), PASS after issue #20.** Baseline, re-verified
  for A1: touching only `examples/phase0-app`'s `main.rsx` and re-running `outou build
  --manifest-dir <copy>` reported `generated 2 file(s)`; both `crate-root.rs` and `components.rs`
  were rewritten although only the crate root's source changed.
- **Cause (issue #20).** `emit()` (`crates/outou-cli/src/build/emit.rs`) generated every unit in the
  plan and then called `atomic_write` (temp file + rename) on every `.rs` and `.rs.map.json`
  unconditionally. There was no comparison against what was already on disk, so every invocation
  gave every generated file a fresh mtime. Content was already deterministic, which is why this
  only showed in mtimes, never in `git status`.
- **Fix.** `emit()` still parses and generates every unit, but skips the write when the existing
  file's bytes equal the new bytes (`is_up_to_date`); `EmitOutput`/`Report` gain `written_files`,
  and `outou build` prints `wrote N file(s), M unchanged`. Comparing the final output (not only the
  unit's own source) is deliberate: output also depends on the plan (`#[path]` lines, crate-relative
  header and map paths), so it cannot miss a changed input. Codegen cost per unit is far below the
  10 ms threshold (e), so skipping parse/codegen was not needed to meet the criterion and would have
  required a dependency graph for cache keys.
- **CLI-path tests** (`crates/outou-cli/tests/build_incremental.rs`, run through the real `outou`
  binary): editing `components/user.rsx` leaves the mtimes of `crate-root.rs`, `components.rs` and
  their maps untouched (pinned to a 2001 timestamp beforehand); an unchanged rebuild writes nothing;
  a hand-edited generated file is restored; adding a child module still rewrites the parent.
- **Workspace measurement** (`tests/fixtures/workspace` copy, `ui-kit` generated files pinned to
  mtime 2001-01-01 after a first build, then `// edit` appended to `app/src/main.rsx` and
  `outou build --manifest-dir <ws>` re-run; debug binaries before/after the change):

  | | `ui-kit` generated files rewritten (of 10: 5 `.rs` + 5 maps) | `app` files rewritten | summary line |
  |---|---|---|---|
  | before | 10 | 2 | `generated 6 file(s)` (all units) |
  | after | 0 | 2 | `wrote 2 file(s), 10 unchanged` |

  Determinism is unaffected (generation is unchanged; `cargo xtask determinism` passes).

- **Alternative not taken (unit-level codegen skip).** Skipping parse+codegen for units whose
  source is unchanged was rejected (Opus review conclusion): savings are only 0.05-0.46 ms per unit
  (see (e)), while a correct cache key must include every plan-dependent input (`#[path]` lines,
  crate-relative header and map paths, child module set), so a missed invalidation would leave
  stale output. The byte comparison needs no such bookkeeping.
- **Known pre-existing bug (not a #20 regression; recommend a separate issue).** On a
  case-insensitive filesystem (macOS APFS), if only the case of a generated name changes (for
  example `mod components;` to `mod Components;`), `clean_stale` (`crates/outou-cli/src/build/clean.rs`,
  ~line 30) checks a case-sensitive `HashSet` and treats the existing on-disk spelling as stale. It
  deletes it after the new write resolved to the same file, so `outou build` exits 0 and the
  generated module is gone. HEAD behaves the same. Candidate fixes: skip removal when the
  `(dev, inode)` of the candidate matches a produced file, or have emit remove differently-cased
  existing entries before writing. No code change made here.

### Thresholds

1. **Incremental `.rsx` → generated Rust: Fixed 10 ms (user-confirmed 2026-09-27).** Rationale: the
   worst measured `generate_unit` value across both fixed workspaces and every planned unit (N=40
   each) was 1.68 ms in the earlier mixed-condition pass — roughly 6x headroom under 10 ms; the
   clean-machine pass in (e) above found an even lower 0.46 ms worst case, so headroom is, if
   anything, wider than first estimated. `generate_unit` is one step of `Workspace::regenerate`
   (which also re-parses for `declared_modules`, builds a `LineIndex` and updates the registry); the
   whole call was not timed separately. 10 ms is also comfortably under a single 60 Hz frame (16.7 ms), the usual perceptual
   anchor for "instantaneous" UI feedback. **Applied**: `regenerating_one_unit_is_fast`
   (`crates/outou-lsp/src/documents/workspace/tests.rs`) now asserts a 10 ms bound (on the median of
   5 calls, after review; the diff below is the original single-call change):

   ```diff
   --- a/crates/outou-lsp/src/documents/workspace/tests.rs
   +++ b/crates/outou-lsp/src/documents/workspace/tests.rs
   @@ -169,11 +169,12 @@
    /// Perf smoke test for issue #9's budget ("incremental `.rsx` ->
    /// generated Rust: perceived as instantaneous"): regenerating one
    /// small-to-medium real unit is a single `outou_syntax::parse` +
    /// `DioxusBackend::generate` call (`outou_cli::build::emit::generate_unit`),
    /// the same primitive `outou build` uses per file — no LSP-specific
   -/// overhead beyond that. 50ms is a generous bound (typical runs are
   -/// well under 1ms for a file this size); this exists to catch a
   -/// catastrophic regression, not to pin an exact number.
   +/// overhead beyond that. 10ms is the fixed A1 threshold
   +/// (`docs/alpha-results.md`, "A1: measurement", user-confirmed
   +/// 2026-09-27): the worst measured value across both fixed workspaces
   +/// (`examples/phase0-app`, `tests/fixtures/workspace`) was 1.68ms (N=40
   +/// per unit).
    #[test]
    fn regenerating_one_unit_is_fast() {
        let outcome = Workspace::load(&phase0_app_dir()).expect("plans and generates");
        let LoadOutcome::Planned(mut workspace) = outcome else {
            panic!("examples/phase0-app has a `.rsx` crate root");
        };
        let main_rsx_uri = workspace
            .rsx
            .keys()
            .find(|uri| uri.ends_with("main.rsx"))
            .expect("main.rsx is a known unit")
            .clone();
        let text = workspace.rsx[&main_rsx_uri].line_index.text().to_string();

        let start = Instant::now();
        workspace
            .regenerate(&main_rsx_uri, &text, 2)
            .expect("regenerating succeeds");
        let elapsed = start.elapsed();

   -    assert!(
   -        elapsed.as_millis() < 50,
   -        "regenerating one unit took {elapsed:?}, expected well under 50ms"
   -    );
   +    assert!(
   +        elapsed.as_millis() < 10,
   +        "regenerating one unit took {elapsed:?}, expected well under 10ms"
   +    );
    }
   ```

   The same 10 ms bound is also asserted, on the per-unit p95 (as a catastrophic-regression guard, not
   a re-derivation), in `crates/outou-lsp/tests/measure_a1.rs`'s own
   `incremental_regeneration_distribution_on_both_workspaces`.

2. **Forwarded-completion overhead: Fixed (user-confirmed, 2026-10-11): median overhead ≤ 5 ms +
   0.15 ms × candidate count, per bucket, same-session A/B — as the bound for the dev/debug-profile
   `outou-lsp`** (`CARGO_BIN_EXE_outou-lsp` under plain `cargo test`, which is where
   `measure_a1_completion_overhead` is calibrated). `assert_completion_buckets` applies the same
   formula **unconditionally in both profiles** (there is no profile-specific assertion; since
   2026-10-24 it also prints a per-bucket verdict line naming the profile). In release the bound is
   very loose (measured: ~31x at 1204 candidates, release median overhead 6 ms vs. the 185.6 ms
   ceiling; ~23x at 122 candidates, 1 ms vs. 23.3 ms) and **a release pass would not catch a
   moderate release regression**: A3 (#21) completion-path optimisation must be judged on the raw
   release numbers, not on this assertion. Measured at three candidate-list-size buckets (see (d)); the form is a
   rate-plus-fixed-cost fit to the two extreme buckets of the first 2026-09-28 pass (N=10, unsettled; fixed term about 2x the
   fitted 2.4 ms, per-candidate term 3x the fitted 0.05 ms/candidate, for headroom; both fit points
   came from that pass's unsettled sessions, see (d)).
   - Ceilings at the measured sizes (N=50, settled re-measurement 2026-10-11, debug): 5.75 ms at 5
     candidates (measured median overhead 1 ms, 5.8x margin), 23.3 ms at 122 (12 ms, 1.9x, the
     tightest), 185.6 ms at 1204 (49 ms, 3.8x). **The assertion passed in all three buckets**, with
     candidate counts equal in both sessions and constant across samples. Release (information
     only): 0 / 1 / 6 ms median overhead at 5 / 122 / 1204.
   - **No p95/tail threshold is set.** The `large-enum-path` multi-second stall (see (d), "Earlier
     N=50 pass") is a rust-analyzer-side event that also appears in direct sessions and did not occur
     in the settled runs (max 60 ms debug), so a tail bound cannot be derived from them. A3 (#21)
     later found that the stall coincides with the initial cache priming and was not seen in any
     settled session (see "The multi-second stall"), so a tail bound for *settled* sessions could be
     set from settled runs if wanted; none has been proposed.
   - Asserted by `measure_a1_completion_overhead` in `crates/outou-lsp/tests/measure_a1.rs` (ignored
     test, per bucket, median overhead against the formula). The test also fails a bucket whose two
     sessions did not settle on the same candidate count, or whose count changed while sampling,
     because the ceiling would otherwise be computed for the wrong list size.
3. **RSS/CPU, flycheck latency and cold-init: baselines only, no threshold proposed**, matching
   `docs/alpha.md`'s own scoping ("Baseline: ..." vs. "Threshold: ..." in issue #17's checklist) —
   these are rust-analyzer-dominated, not something `outou-lsp` controls the cost of.

### Issue #17 checklist — what this work satisfies

Referring to `docs/alpha/issues/17-measurement.md`'s checklist (not ticked there per this task's
instructions; recorded here instead):

- Measurement setup (fixed workspaces, machine/OS/toolchain/rust-analyzer version): **done**, see
  "Setup" above.
- Baseline: two-process RSS/CPU sampling, cold + scripted editing, vs. a single plain session:
  **done** (settled re-measurement, 3 repeats, process-tree
  footprint and RSS, cold and edit windows, both workspaces), see (a).
- Baseline: flycheck latency, N≥10, warm target dir, median/max, vs. plain `.rs` save: **done**, see
  (b).
- Baseline: cold `initialize` → first usable hover/completion/definition, N runs, median/max:
  **done** (N=5, timed from `initialize` sent, settled-harness
  re-measurement 2026-10-11), see (c).
- Threshold: incremental `.rsx` → generated Rust, both workspaces, N≥30: **measured** (N=40 per
  unit, both workspaces, re-confirmed on a clean machine) and **Fixed at 10 ms (user-confirmed)** —
  see "Thresholds" (1) and the applied diff.
- Threshold: forwarded-completion overhead, same-session A/B, per candidate-list-size bucket:
  **measured at three bucket sizes** (tens / hundreds / ~1000+, see (d); re-measured at N=50 on
  2026-10-11 on settled sessions, debug and release), **Fixed (user-confirmed 2026-10-11): median overhead ≤ 5 ms + 0.15 ms × candidate count,
  no tail threshold** — see "Thresholds" (2).
- Threshold: "a single-file edit never regenerates the whole crate," pass/fail on both paths:
  **done**, see (f) — language-server path PASS, `outou build` CLI path FAIL at A1 and PASS after issue #20.
- Every number, threshold and method recorded in this document: **done**, this section and (a)-(f)
  above.

### What could not be measured, or was measured differently than planned

- **Sampling resolution and repeat count.** RSS is sampled every 500 ms with 3 repeats per cell
  (median and range reported), cold-init has N=5 and completion N=50 per bucket (one session per
  bucket and profile). The machine was quiet but not idle (load1 about 3-5 from macOS background
  services, see "Setup"). Treat the numbers as a baseline for this machine, not as a distribution.
- **The `large-enum-path` completion bucket's tail behavior**: in the unsettled N=50 pass (2026-10-11,
  see (d)) the multi-second stall is repeatable (about 2.4 s, 2 of 4 `outou-lsp` sessions) but also
  occurs in direct rust-analyzer sessions (1 of 4), coincides with the candidate count changing
  1200 → 1204, and so points at rust-analyzer's own late background work rather than `outou-lsp`'s
  mapping. The exact rust-analyzer trigger was not identified, which is why no tail/p95 threshold is
  set. Flagged rather than smoothed over.
- **Deferred to A3 (#21)** (stall trigger, `serverStatus`, release numbers): worked in "A3:
  forwarded-completion overhead (issue #21)" below; the `serverStatus` decision was taken on 2026-10-24 and implemented there.
- ~~Deferred to #20: the `outou build` CLI-path FAIL in (f).~~ Fixed in issue #20, see (f).
- **Harness limitations (recorded, not fixed)**: harness requests have no per-request timeout, so a
  wedged server can hang the rss mode or a `Command::output()` call; and the `outou-lsp` side of the
  RSS settle uses hover stability only (no quiescence signal, no expected signature).
- Candidate-list-size bucketing has three real buckets (not one), and the RSS harness now supports
  both fixed workspaces directly (`MEASURE_A1_FIXTURE`).

## A2: recovery (issues #18 and #19)

Status: the four A2 shapes are fixed and have fixtures (commits `a6c54aa`, #18, and `771a8af`, #19). The facts below come from those commits' messages, `docs/grammar.md` §9 and the fixtures; no new measurement was made for this section.

**Correction (2026-10-24).** The gate `every_incomplete_fixture_compiles_with_no_parse_errors` checked nothing until 2026-10-24: it skipped every rustc error whose JSON had a `code` key, and rustc writes `"code": null` for a parse error, so every parse error counted as one with an E-code. Fixed the same day (a parse error is an error with `code` null, other than the `aborting due to` summary), the test reports every failing fixture and prints how many it checked (33). It then failed on 6 of the 33: `attribute-value-missing-before-truncated-closing-tag` and the four `lt-*-in-child` fixtures (a bare `recovery_element()` call in child position is not valid `rsx!`), and `unclosed-island-mid-file` (the enclosing `</ul>`, swallowed by the island that lost its `}`, reached the Rust text). Both causes are fixed (see the #24 section) and all 33 pass. The earlier statements that this gate "passes" had no evidence behind them.

### What changed

- **Swallowed tail (#18).** A function body that reaches end of input without its closing `}` is cut at the next column-0 item head and always diagnosed as an unclosed body (``unclosed body of function `Name`, expected `}` to close this `{` ``, at the body's `{`). An incomplete signature stops at an item head or at a mismatched closing delimiter. The boundary search walks Rust tokens lexically, is linear, and is capped at 16 body cuts per file. The item after the broken function (`fn After`) survives as its own item.
- **Truncated closing tags (#18).** A truncated `</` right after a reserved fragment, and a top-level truncated `</div` at end of file, keep their diagnostics and, for the stray close, the partially scanned tag name. The two deferred-work markers from Phase 0 in `crates/outou-syntax/src/parser/jsx/mod.rs` are removed.
- **`<div class=` followed by a sibling (#19).** A `<` that begins a tag (`<Name`, `<>`, `</`) now ends an unfinished opening tag, so the sibling is its own node instead of becoming attributes of the broken tag. The missing value is represented as a `JsxAttributeValue::Error` with an empty span at the `<` and diagnosed as ``attribute values must be double-quoted strings or `{…}` expressions``; `outou-lsp` answers `Cursor::AttrValue` with an empty list locally, so completion no longer returns `null` there. In Rust regions the following JSX is absorbed as the element's children and a matching `</Name>` closes it, which keeps Recovery output valid Rust. Fragments after a broken tag are still unsupported (`TODO(alpha) (#31)`).

### Fixtures

Under `tests/fixtures/incomplete/`: the `*-mid-file` fixtures (`bodyless-fn-mid-file`, `class-attribute-value-mid-file`, `unclosed-island-mid-file`, `unclosed-tag-mid-file`) pin that `fn After` survives; `fragment-truncated-closing-tag-eof`, `fragment-closing-tag-interrupted-by-lt` / `-rbrace`, `stray-truncated-closing-tag-eof` and `stray-closing-tag-interrupted-by-lt` / `-rbrace` cover the two `jsx/mod.rs` gaps; `attribute-value-missing-before-sibling` and its variants (top level, in a `let`, in an island, before a closing tag, before a truncated closing tag) and `attribute-name-only-before-sibling` cover #19. Each has a verbatim `.expected` file. Each fixture is checked by `every_incomplete_fixture_produces_parseable_analyzable_rust` and `every_incomplete_fixture_compiles_with_no_parse_errors` (`crates/outou-backend-dioxus/tests/recovery.rs`).

### Performance and corpus

`crates/outou-syntax/tests/item_recovery.rs` has timing tests with a fixed time bound for large runs after a broken function, many broken functions and many unclosed visibility groups (names ending `_in_linear_time`); the cut cap is tested separately (`cuts_per_file_are_capped`). The parser corpus was re-run on 2026-10-24 for the A5 close-out (`cargo xtask corpus test`, repository at `4b741c9`, after #18 and #19), against the same pinned corpus as Phase 0: `rust-lang/rust` tag `1.98.1`, commit `48a229ceaefd`, `tests/ui`, 20,722 files. Result: panics 0, timeouts 0, splice round-trip mismatches 0, JSX mis-detections 20 (same as Phase 0), and **false positives (Outou diagnostics on plain Rust): 33, of which 0 are on valid Rust and 33 on Rust that rustc rejects**; Phase 0 recorded 20 (`docs/phase0-results.md` §2: 0 on valid Rust, 20 on rejected Rust). The corpus tool classifies a file as rejected Rust when it has a `.stderr` companion in rust-lang's test suite; all 33 do (the report's own count: `on_rejected_rust` 33, `on_valid_rust` 0). The 13 additional files, identified from the diagnostic text, are all new #18 diagnostics, ``unclosed body of function `X` `` or `incomplete signature for function `X` ``, on files that rustc itself rejects for an unclosed or mismatched delimiter (`parser/brace-in-let-chain.rs`, `parser/missing_right_paren.rs`, `parser/issues/issue-2354.rs`, `-62894`, `-62895`, `-63135`, `-81804`, `-84148-1`, `-84148-2`, `-111148`, `span/issue-81800.rs`, `suggestions/issue-94171.rs`, `typeck/issue-92481.rs`). The other 20 are the 13 `diff-markers` fixtures and 7 parser-torture or bare-`:`/`<>` files of the Phase 0 list. That rustc rejects these 13 for an unclosed or mismatched delimiter was verified by the model that reviewed #18 and reported to the project manager; it was not repeated here, and the repository itself records only the `.stderr`-companion classification. None is on valid Rust. Source of the numbers: this run (the earlier report of 33 is not written down in any repository document).

### Known limits (from `docs/grammar.md` §9)

L1 (a late or missed cut when the earliest diagnostic is inside a later item), L2 (a leftover closing delimiter passes through to `rustc`) and L3 (text skipped by a truncated-construct recovery). A column-0 item head inside JSX text, an attribute string or `<pre>` content after the real error can be taken for a boundary. None changes valid input.

## A3: forwarded-completion overhead (issue #21)

Issue #21 (`docs/alpha/issues/21-completion-overhead.md`). Same machine (Apple M1 Pro, 16 GiB),
toolchain and rust-analyzer 1.98.1 as "A1: Setup"; measured 2026-10-24 with the A1 harness
(`measure_a1_completion_overhead`, N=50 per bucket and side, settled sessions, same-session A/B).

**Machine state, not clean.** Unlike the 2026-10-11 settled pass, this session did *not* run on a
freshly booted idle machine: VS Code was open (main process and helpers; **no rust-analyzer
process** was running, `pgrep -fl rust-analyzer` empty), `vm.swapusage` showed 5.5 GiB of 7 GiB used
before and after the first run, and load1 was 2-5 at the start of most runs (one release run started
at load1 39 and one other at 5; they are marked below). The numbers are therefore a regression check
against the 2026-10-11 baseline, not a replacement for it.

### Threshold and verdict

The user-confirmed threshold (2026-10-24; unchanged from "Thresholds" (2)): **median overhead <= 5 ms
+ 0.15 ms x candidate count. p95 is not used.** It was already encoded in
`assert_completion_buckets` (`crates/outou-lsp/tests/measure_a1.rs`) and is unchanged. The only
change to that test is a per-bucket `measure_a1 completion verdict [debug|release]` line (value,
ceiling, PASS/FAIL), so a release run states its verdict instead of passing silently; the formula
is the same in both profiles, which is why a release pass says little (see below).

| bucket | candidates | ceiling | debug median overhead (this pass; 2026-10-11) | release median overhead (this pass; 2026-10-11) |
|---|---|---|---|---|
| `tiny-local-type` | 5 | 5.75 ms | 0 (1) | 0 (0) |
| `option-member-access` | 122 | 23.3 ms | 11 (12) | 0-1 (1) |
| `large-enum-path` | 1204 | 185.6 ms | 48 (49) | 6 (6) |

All three buckets PASS in both profiles. Three further debug runs of `large-enum-path` alone:
49 / 50 / 49 ms (`outou-lsp` p95 59-67, no sample above 100 ms). Release: four N=50 runs; three give
0 / 0-1 / 6 ms, the fourth (the first release run, load1 3-5, swap in use) gave 0 / 8 / -1 ms with a
direct `large-enum-path` median of 16 ms instead of the usual 8, i.e. noise in the *direct* side;
all four pass. Largest release headroom is 31x at 1204 candidates, as noted in 2026-10-11.

Margin per profile (median overhead vs. ceiling), this pass: debug n/a / 2.1x / 3.9x (the tiny
bucket's overhead was 0 ms, so its ratio is undefined; the 5.8x in the 2026-10-11 pass came from a 1 ms
median against 5.75 ms); release n/a (0 ms) / 23x or more (0-1 ms against 23.3 ms) / 31x (6 ms against
185.6 ms). The debug assertion's tightest bucket is still `option-member-access`. Because the formula
is the same in release, a release regression is only caught once the median exceeds the ceiling:
roughly 23x the current value in the 122-candidate bucket (at 1 ms) and 31x in the 1204-candidate
bucket. The release judgement here is therefore the raw numbers above, not the pass.

### Profile of `map_completion_response` (no optimization made)

The threshold is met in both profiles, so by the issue's own conditional ("if over threshold")
nothing in `crates/outou-lsp/src/response.rs` was changed. For the record, a throwaway
release-profile in-process timing (a synthetic 1204-item list shaped like rust-analyzer's enum-variant
items, run against `examples/phase0-app`'s real workspace registry; the test was removed afterwards)
put the whole `map_completion_response` call at about 2.2-3.5 ms once warm (first call ~8 ms), of
the ~6 ms release overhead the A/B shows; the rest is JSON parse/serialize and pipe I/O outside this
function. Split of the pieces timed separately: `textEdit` reverse-mapping (`map_text_edit`) about
1.2-1.9 ms (largest, ~45%), `is_backend_leak` scan about 0.5-1.1 ms (~25%), the first cursor-containment
check about 0.4-0.7 ms (~15%; it `serde_json::from_value`-clones every range). If a release regression
ever shows up, those three are the order to look at (avoid the range clone/`from_value`; skip
`documentation`/`filterText` scanning when absent; cache the generated-file lookup across items).
Not applied: it would be an optimization against a threshold that is met by 31x.

### The multi-second stall: what it is (issue #21 item "pin down the trigger")

A new harness mode `--mode stall` (`measure-a1.mjs`; `measure_a1_completion_stall`) is the *opposite*
of the settled `completion` mode: right after the `didChange` that introduces the site, completion is
requested back to back (50 ms apart) for 9-20 s with **no wait for quiescence**, and every
`experimental/serverStatus` / `$/progress` notification is timestamped on the same clock (optionally
with `rust-analyzer --log-file` under `RA_LOG=info`).

Result: **the stall reproduced in 27 of 27 fresh sessions** sampled that way (split in the table
below), each with exactly one sample over 500 ms, and in none of the settled runs of this pass (5
all-bucket runs, 1 debug and 4 release, plus 3 debug `large-enum-path`-only runs, all N=50: no sample
over 1 s; the largest was 98 ms).

| condition | sessions | stalled | stalled request | when it ended |
|---|---|---|---|---|
| `large-enum-path`, direct, `RA_LOG=info` | 3 | 3 | 6.2-6.4 s | at quiescence + ~0.4 s |
| `large-enum-path`, direct | 10 | 10 | 2.12-2.23 s | at quiescence + ~0.4 s |
| `large-enum-path`, direct, autoimport off | 5 | 5 | 2.10-2.18 s | at quiescence + ~0.4 s |
| `tiny-local-type` (5 candidates), direct | 4 | 4 | 2.14-2.31 s | at quiescence + ~0.4 s |
| `large-enum-path`, via `outou-lsp` | 5 | 5 | 2.09-2.15 s | ~0.3 s after the forwarded `$/progress` `cachePriming` end |

What the timelines show:

- Every session first returns empty answers (0 candidates for the first ~0.4-1.3 s), sometimes then
  a fast intermediate list (1200, or 3 for the tiny site), and then has **one** slow request, the first
  that returns the final (1204 / 5) list. The slow request starts about when `Indexing` begins
  (`$/progress` `begin` "Indexing" at 1.2-1.7 s after `didChange`) and **returns about 0.3-0.4 s
  after rust-analyzer's `cachePriming` progress ends / `serverStatus` goes `quiescent: true`**. Every
  later sample in all 27 sessions is at most 47 ms (the tail right after the stall, while the
  analysis finishes settling), and a settled session answers in single-digit to low-tens of ms.
- It is **independent of candidate count and of `large-enum-path`**: the 5-candidate tiny site stalls
  for the same ~2.2 s. It is independent of `completion.autoimport`. It is not `outou-lsp`: the
  direct session stalls identically and the `outou-lsp` session adds nothing measurable.
- With `RA_LOG=info` the same stall lasts ~6.2 s (tracing makes the work slower). That it stretches
  with logging overhead and (below) with machine contention suggests computation rather than a fixed
  timer or a lock wait, but CPU time was not measured, so "CPU-bound" is an inference, not a result:
  about 2.1 s on an idle-ish M1 Pro. The `info` log of a stalled request is dominated by `inherent_impls_in_crate_query` (hundreds of
  thousands of lines), `crate_def_map_query`, `import_map_query` and `TraitImpls` across the ~93 crates
  of the dependency graph, i.e. rust-analyzer's first (cold) computation of those salsa queries, which
  the initial cache priming runs over the same period. The log shows the two overlapping; it does not
  show which of them the request waits on. The 1200 -> 1204 / 3 -> 5 flip is the sysroot's
  blanket-impl methods becoming visible, i.e. exactly the answer that needs those queries.
- On the 2026-09-28 `~4 s at ~1200 candidates, 1 of 10` outlier: it is consistent with this stall.
  The first pass sampled right after the first non-empty answer while a VS Code-owned rust-analyzer and
  a hung rust-analyzer were competing for CPU on the machine, which fits "the same work, longer under
  contention"; this cannot be proven after the fact. It is **not** a candidate-list-size or
  leak-scan/reverse-map effect (the steady-state `outou-lsp` cost is ~50 ms debug / ~6 ms release at
  1204 candidates, flat, p95 within 15 ms of the median).
- Product meaning: the first completion that arrives while rust-analyzer is still indexing
  (~1.2-3.3 s after the workspace opens on this fixture, longer for a big workspace, and the ~93-crate
  dependency graph is small) blocks until the indexing finishes. A user typing `.` in the first seconds
  waits; after that it is never seen. The editor already receives the matching signal today,
  because `$/progress` is forwarded: the stall coincides with a visible "Indexing" progress report.

No exclusion of the outlier from any measurement was made; the settled tables simply did not contain
one.

### `experimental/serverStatus` and indexing visibility (decided and implemented 2026-10-24)

User decision (2026-10-24): do not relay `experimental/serverStatus`; show a health problem once, in
Outou vocabulary. Implemented in `crates/outou-lsp/src/status.rs`, wired in
`dispatch/responses.rs` (`handle_ra_notification`) and `server.rs` (`setup_rust_analyzer`).

- **Declaration.** rust-analyzer only sends the notification if the client capabilities it receives
  contain `experimental.serverStatusNotification`. `outou-lsp` adds that flag to the capabilities it
  sends *to rust-analyzer* in its own `initialize` (the editor's capabilities are otherwise forwarded
  as before). It says nothing about what the real editor supports, and nothing is advertised to the
  editor.
- **Behaviour.** `health` `warning`/`error` produces one `window/showMessage` (warning/error
  severity) beginning `Outou:`. The same non-`ok` state does not notify again; `ok` re-arms; a change
  between `warning` and `error` notifies. `ok` shows nothing. The notification itself is never
  forwarded. Only an explicit `ok` re-arms; a missing, `null`, non-string or unknown `health` changes
  nothing and shows nothing. The upstream `message` contributes only its first line (200 chars max)
  and only if it passes `contains_backend_marker` and, after lower-casing and `-` to `_` (and again
  with all punctuation removed), contains no `dioxus`, `rsx`, `rust_analyzer` / `rustanalyzer`,
  `propsbuilder` or `.generated`; otherwise the fixed Outou text is shown alone. (The fixed text
  does not claim a cause.)
- **(B) investigation, before the change.** (1) Visibility: `window/workDoneProgress/create` was
  already forwarded to the editor under a fresh request id, the editor's response routed back to
  rust-analyzer (`reverse_pending`), and `$/progress` forwarded under the same token, all gated on the
  editor's `window.workDoneProgress`. So a progress-capable editor did see an indexing report;
  nothing was missing there. The gate3 `progress-before-hover` probe confirms an `end` reaches the
  client. (2) Leakage: it leaked. The title was rust-analyzer's own (`Indexing`, `Roots Scanned`,
  `Fetching`, `cargo check`), the message named crates and paths (`12/300 (dioxus_core)`), and the token
  was `rustAnalyzer/...`. This is a leak by AGENTS.md's rule even though the old comment on
  `handle_ra_notification` assumed the editor allocated tokens (it does not; rust-analyzer does).
- **(B) implementation, after review.** A first version renamed the `rustAnalyzer/` and
  `rust-analyzer/` prefixes to `outou/`. Review found that this is not injective (`rustAnalyzer/x`,
  `rust-analyzer/x` and `outou/x` collide), that `window/workDoneProgress/cancel` then reached
  rust-analyzer with a token it never created, and that the editor's own `partialResultToken`
  progress was rewritten or dropped. Now only tokens from rust-analyzer's `create` are mapped: each
  gets an opaque `outou/progress/<n>` token, stored both ways in `State` and removed at `end`. A
  client without `window.workDoneProgress` gets `null` for `create` and that token's progress is
  dropped. A `$/progress` for any other token passes untouched whatever the capability flag.
  `window/workDoneProgress/cancel` is mapped back to rust-analyzer's token (unknown tokens forwarded
  as is). `partialResultToken` is removed from requests forwarded to rust-analyzer (partial results
  cannot be reverse-mapped; rust-analyzer returns the full result in the ordinary response).
  Titles are mapped by keyword to `Outou: indexing`, `Outou: scanning files`, `Outou: fetching
  dependencies`, `Outou: building`, `Outou: checking`, anything else `Outou: working`. A message is
  reduced to its leading `done/total` counter or removed. Only `begin` / `report` / `end` objects are
  rewritten; `kind`, `percentage` and `cancellable` pass through.
- **What this does not do.** The first-completion stall itself is untouched (it is rust-analyzer-side);
  it is now only visible as an "Outou: indexing" report, in editors that support work-done progress.
  Not verified in a real editor (VS Code or other): coverage is the unit tests plus the gate3 probe
  against a live rust-analyzer. `health` notices were not provoked against a live broken workspace
  (the notice path is covered by unit tests with synthetic `serverStatus` params).
- **Tests** (`status.rs`, `dispatch/responses.rs`, `dispatch/progress_tests.rs`): warning and error each notify once with the right
  severity; ok is silent; a repeat is silent; ok then error notifies again; warning then error
  notifies; leaky upstream messages (`rsx!`, `dioxus_core`, `.generated`, `rust-analyzer`,
  `PropsBuilder`, and the spellings `DIOXUS_CORE`, `RSX!`, `dioxus-html v0.7.10`) never reach the
  editor; an unknown `health` does not re-arm; progress title/message rewriting; distinct upstream
  tokens stay distinct; the token is identical in `create` and `$/progress`; editor-token progress
  passes untouched with or without work-done support; `cancel` reaches a mock rust-analyzer under
  its own token; `partialResultToken` does not reach it.

### Issue #21 checklist status

- Re-measure with A1's method under the fixed threshold: done (above); PASS in debug and release.
- Profile and optimize if over threshold: **not over threshold**, so no optimization; profile
  recorded above for later.
- Re-measure after optimizing: n/a (no change to the completion path).
- Before/after in this document: done (after == before; the 2026-10-11 table is the "before").
- Pin down the stall trigger on a larger set of sessions: done (27 sessions, `RA_LOG` on 3): the first
  completion that returns the final list consistently overlaps rust-analyzer's initial cache priming and
  query computation and ends shortly after it (correlation; causation not isolated). What
  remains open is whether to *treat* it (user decision below).
- `experimental/serverStatus` decision: taken by the user (2026-10-24) and implemented as above, together with Outou-vocabulary rewriting of the forwarded progress.
- Release judgement: done; the formula's ~23-31x looseness stands, and a verdict line now names the
  profile.

## A4: dogfood application (issue #22)

Status: built, checked natively and for `wasm32-unknown-unknown`, and **walked through by hand in Chrome on 2026-10-24** (see "Browser run"). One defect found there (F18) was fixed afterwards and **re-checked in the browser the same day (2026-10-24)**: the new-task form shows Priority Medium at once, the edit form of a High task shows High, and the detail Status select keeps "In progress" after opening another task and coming back; no console errors. Full friction log with dispositions: `examples/dogfood-board/README.md` ("Friction log", F1 to F18).

### The application

`examples/dogfood-board`: an internal task board, subject chosen by the project owner on 2026-10-24 (forms to add and edit tasks, a filtered list, a detail view, state shared between components). It is its own Cargo workspace outside the root one (`examples/phase0-app`'s arrangement), about 430 lines of `.rsx` and `.rs` in the app and about 410 in the library:

- `board-core`: plain-Rust workspace library (domain model, validation, immutable `Board`, `Filter`), 8 unit tests.
- `app`: `main.rsx` (App, `launch`, three headless render tests), `filter_bar.rsx`, `task_list.rsx`, `task_detail.rsx`, `task_form.rsx`, and `state.rs` (a plain `.rs` module in the mixed crate holding the shared signals).

### What was added to the facade

`outou::prelude` re-exports the backend's existing `use_signal`, `Signal`, `use_context_provider`, `use_context`, `ReadableExt`, `WritableExt` and (behind the new opt-in `launch` / `web` features) `launch`. Nothing was wrapped or renamed. Recorded in ADR 0010 (Amendments) and `docs/backend-leakage.md` rows 31 and 32. Four more rows came from the build itself: 33 (signal read guard held across `set`), 34 (integer children are not renderable), 35 (`key` lowering), 36 (headless rendering names the backend). Two rows were added after the browser run and review: 37 (`<select value>` does not select an option on mount, F18) and 38 (a correction to row 35's description of the failing `key` shapes). The ledger now has 38 rows; none of the existing 30 was rewritten.

### Verification run (2026-10-24)

| Command | Result |
|---|---|
| `outou build --manifest-dir examples/dogfood-board`, then in that directory `cargo build` | pass |
| `cargo test` (example workspace) | pass: 8 `board-core` tests, 1 app render test |
| `cargo clippy --all-targets -- -D warnings` (example workspace) | pass |
| `cargo check --target wasm32-unknown-unknown` (example workspace) | pass |
| `cargo build --target wasm32-unknown-unknown` | pass (produces a `.wasm`; not bundled, not loaded) |
| `outou fmt --check` on `app/src` | pass after one `outou fmt` run (output kept) |
| `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (repository) | pass |
| `cargo +1.85 check --workspace` (repository) | pass. The new opt-in features add about 2,400 lines to the root `Cargo.lock`, but nothing in the workspace enables them, so MSRV is unaffected. The example workspace is outside the MSRV check by construction (own lockfile, own workspace). |
| `cargo xtask determinism` | pass, 10 targets; the example is not one of them, so its determinism was checked only by a second `outou build` rewriting nothing |

### Browser run (2026-10-24)

Run by hand by the project's manager: dioxus-cli 0.7.10 (`cargo install`), `~/.cargo/bin/dx serve --platform web --port 8099`, Chrome. `dx` installed `wasm-bindgen-cli` 0.2.129 and esbuild itself. The `dx` first on `PATH` was Deno's unrelated command, so the absolute path was required (F8).

- **F9 resolved**: `dx serve --platform web` started and rendered the app without `--features web`; the app's `web = ["outou/web"]` was recognized.
- Walkthrough steps 1 to 11 and 13 behaved as written, with no browser console errors: initial render, detail view, status change from the detail select and from the list checkbox, status chips, search (including the empty-result message), create with validation and success, edit pre-filled and per-task (the F14 reasoning was **observed** to hold), cancel from Edit, delete, and reset on reload. Not run in the browser: step 12 (length limits, covered by `board-core` tests) and New task then Cancel.
- **New defect, F18 (ledger row 37)**: a `<select value={..}>` did not show its value on mount (new form's Priority showed "Low" for a Medium value; an edit form showed "Low" for a High task; the detail Status select showed "To do" for an "In progress" task after a remount). Display only: the stored values were right. Cause: backend semantics (the select's `value` does not select the matching option on mount; React's does), not Outou's lowering. Fixed in the app with `selected={option == current}` on each `<option>`; two headless tests check the flags. **Re-checked in the browser afterwards (2026-10-24)**; the fix holds in the three cases above.
- **Automated versus manual coverage**: the app's automated tests are three headless first-render tests (whole tree, edit form, detail view). Every interactive flow above is covered by the hand-run walkthrough only; it is not repeatable automation (F13). The fixed `key` and `select` behavior in particular is only as verified as that one run.

### Findings that matter for A5

- **A state API is the first thing an application needs** and the facade had none: the re-exports are Dioxus's `Signal` model unchanged. Two everyday patterns fail on it with backend-flavored diagnostics (row 33's guard borrow, F4; row 34's integer child, F3). Neither is a language-level Outou rule.
- **`key` is the weakest seam** (rows 35 and 38, F1/F2): both a static key and a key expression containing a `{` or `}` byte (for example `format!("edit-{id}")`) fail with messages written by the backend, at a generated position. A string literal inside the expression is fine; row 38 corrects row 35's first description.
- **The biggest practical gap is not a ledger row**: `cargo build` diagnostics land on `src/.generated/*.rs` lines, one very long `rsx!` line each, instead of the `.rsx` position (F5). Only the language server and the UI-test harness map back.
- **`<select value>` carries different semantics from React** (row 37, F18): only a real browser run found it; no compile-time or headless check could have. Together with rows 33 to 35 it argues that the A5 weighing should count "same JSX, different meaning" separately from "backend-authored diagnostic".
- **What did not hurt**: mixed `.rs` / `.rsx` modules, a plain-Rust workspace library, JSX in `match` arms and closures, and the formatter, which is idempotent and keeps the app building (README, "What worked without friction").
- Count of A4's own additions to the ledger: 6 rows (31 to 36), of which 3 (33, 34, 35) are cases where a user hits a backend-authored diagnostic or rule while doing ordinary work; that count goes into the A5 weighing of rows that make Outou semantics unnatural.

### Not verified

- The F18 fix **was** re-checked in a browser (2026-10-24); the headless tests check only the `selected` flags. The first hand-run walkthrough and that re-check were of the application before #26 to #29; the migrated application was checked in the browser the same day (see the A5 section, "Browser check of the migrated application"). Walkthrough step 12 (length limits, covered by `board-core` unit tests only) and New task then Cancel were not run in the browser.
- The language-server experience on the app's `.rsx` files (F17) was not exercised when the A4 run was made. It was exercised later, in a real VS Code, on 2026-10-25; see "Real-editor check of outou-lsp (F17, 2026-10-25)" in the A5 section below.
- The `dogfood-board` CI job has not run on a pushed branch.

### Issue #22 checklist status

Done: build the app (native and wasm32 compile); prelude re-exports recorded (ADR 0010, rows 31 and 32); app under `examples/` as a non-workspace member; Friction log with dispositions; backend-specific frictions appended as numbered rows (33 to 36); manual walkthrough documented; this summary. Open: the first CI run of the new job (described above). The browser run is done, including the F18 re-check.


## A5: runtime decision (ADR 0014, issues #23 and #25 to #30)

Status: decided on 2026-10-24 and carried out the same day. [ADR 0014](adr/0014-dioxus-continues-as-backend-not-as-specification.md) is Accepted (commit `0d75d17`; it supersedes ADR 0003 and amends ADR 0010), and the five implementation issues it names are implemented and pushed. The language-server behavior was checked in a real VS Code on 2026-10-25 (see "Real-editor check of outou-lsp (F17, 2026-10-25)"); what that check did not cover is listed there. The migrated dogfood application was checked in a browser on 2026-10-24 (issues #28 and #29; see "Browser check of the migrated application" below). The facts in this section come from `git log`, `docs/backend-leakage.md`, the issue mirrors in `docs/alpha/issues/`, ADR 0014 and a ledger recount made on 2026-10-24.

### The decision

**Dioxus remains the backend for now, but it is not Outou's specification** (ADR 0014). Outou's semantics are defined by Outou-owned APIs and by the lowering; behavior that exists only because of Dioxus is a ledger row, not a rule. No Outou runtime, scheduler or update semantics is built, because the evidence that would justify one is unmeasured (below) and the semantics such a runtime would implement are undefined (ledger row 3). The facade rule that follows: `outou::prelude` does not `pub use` a backend item directly (ADR 0010, "Alpha A5"), except `Element`, `component` and the hidden macro-hygiene names; a test pins the list (`crates/outou-cli/tests/prelude_names.rs`).

### The ledger, recounted

At the time of the decision the ledger had 38 rows (ADR 0014): **25** Dioxus leakage, **2** Dioxus leakage (fixed) (rows 28, 30), **2** Dioxus leakage / Temporary limitation (24, 32), **8** Temporary limitation, **1** Outou intrinsic (marker) / Dioxus leakage (lowering) (row 6) and **0** Accepted permanent behavior. Seven of the 38 only correct, resolve or close earlier rows (16, 17, 22, 23, 25, 28, 38), so about 31 to 32 distinct constraints.

Counted again with `grep` on 2026-10-24, after issues #25 to #30 appended rows 39 to 46 (and a browser-check note, row 47, was added later the same day), the ledger has **47 rows**:

| Classification (as written in the row) | Rows | Count |
|---|---|---|
| Dioxus leakage | 1, 2, 3, 4, 5, 8, 9, 11, 14, 15, 16, 17, 18, 19, 22, 25, 27, 29, 31, 33, 34, 35, 36, 37, 38, 45, 46 | 27 |
| Dioxus leakage (fixed) | 28, 30, 41, 44, 47 | 5 |
| Dioxus leakage (fixed) / Outou decision | 42 (`bool` is an error), 43 (`<select value>`, fixed within limits; the type set is an Outou decision) | 2 |
| Dioxus leakage / Temporary limitation | 24, 32 | 2 |
| Temporary limitation | 7, 10, 12, 13, 20, 21, 23, 26 | 8 |
| Outou intrinsic (marker) / Dioxus leakage (lowering) | 6 | 1 |
| Outou intrinsic / leakage | 39 (rustc compiles generated Rust; long lines and registry spans come from `rsx!`) | 1 |
| Outou intrinsic | 40 (`include_str!` paths resolve against the generated directory) | 1 |
| Accepted permanent behavior | none | 0 |

Rows 39 to 46 are not new constraints found by the alpha so much as the outcomes of A5: 41, 42, 43 and 44 resolve rows 35/38, 34 (and 20 in part), 37 and 31/33 (and 8 in part); 39 and 40 record F5 and a side effect of ADR 0009's layout (the first resolved for `outou cargo`, the second mitigated, both with plain `cargo` unchanged); 45 lists what #29 deliberately did not fix; 46 restates the correction to rows 4 and 5 (`Element` is `Result<VNode, RenderError>` in `dioxus-core 0.7.10`, not `Option<VNode>`, which ADR 0014 and ADR 0010 already stated) and maps the rows A5 touched. 47 only records that row 43's generated form was checked in a browser. No row was rewritten.

### The nine groups, and what A5 did to them

ADR 0014 sorted the rows that make Outou's semantics unnatural into nine groups: five the facade and codegen can fix, four they cannot.

| Group (rows) | ADR 0014's view | Result after #25 to #29 |
|---|---|---|
| Hooks and signals API (8, 31, 33) | Fixable | **Fixed for everyday code** by #29: `State<T>` with `get` / `with` / `set` / `update`, no guard in the API. Kept on purpose: the backend's subscription semantics, hook-ordering rules, and a run-time panic in Outou's words when the same state is used re-entrantly (row 44); `set` or `update` in a component body that reads the same state never settles. |
| Numeric children (34; 20 in part) | Fixable | **Fixed** by #27: integers, floats and `char` render as text; `bool` is an Outou-worded compile error by decision. Remaining gaps are listed below (row 42). |
| `key` (35, 38) | Fixable | **Fixed** by #26: a static string and `format!("edit-{id}")` compile; accepted key types are narrower than at HEAD (strings and integers, `IntoKey`). |
| `<select value>` (37) | Fixable | **Fixed within stated limits** by #28: lexically contained options only; `multiple`, options without `value` and options reached through variables or components are not rewritten (row 43). Browser-checked on 2026-10-24 (new form Medium, edit form of a High task High, remounted detail Status unchanged). |
| Event types, in part (9) | Fixable | **Not started.** Handler argument types remain the backend's; row 9 is unchanged and the prelude exports no event type. |
| Props must be `Clone + PartialEq + 'static` (1 to 3) | Not fixable | Unchanged. |
| The shape of `Element` and `children` (4, 5) | Not fixable | Unchanged (the type is `Result<VNode, RenderError>`; row 46). |
| Borrowed props (10) | Not fixable | Unchanged. |
| Hyphenated attribute names (14, 17, 18, 29) | Not fixable | Unchanged. |

So four of the five fixable groups are fixed (one within limits) and one is untouched; the four groups the facade cannot absorb are exactly as they were. Separately, the diagnostics problem F5 (rustc reports generated positions) is resolved for `outou cargo` by #25; plain `cargo` is unchanged.

### What A5 shipped

| Issue | What | Commits |
|---|---|---|
| #25 | `outou cargo <build\|check\|clippy\|test>`: generates in Strict mode, runs Cargo with `--message-format=json-diagnostic-rendered-ansi` and reports each span at its `.rsx` line and column through the existing source maps. The translation is a library (`outou_cli::diagnostics`) shared with the UI-test harness. Rules for what cannot be translated are in `crates/outou-cli/README.md`. `outou check` now prints a success line (F16). | `1ecf4e9` |
| #26 | Typed `key`: the key is evaluated into a local before the `rsx!` call and checked by the sealed `outou::jsx::IntoKey`; the backend's format-string messages cannot be reached. | `22fa9fb`, `6bea1a0` (test: strip ANSI colour from cargo output before the leakage checks) |
| #27 | Typed children: every child island is passed to the sealed `outou::jsx::IntoChild<M>`; numbers and `char` render, `bool` is an error that points to `if` and `Option`. | `27d851f`, `628fdd3` (test: detect compiler warnings by diagnostic line, not by substring) |
| #28 | `<select value>`: lexically contained options are bound once and get a computed `selected`; values are restricted to strings, integers and `char` (`IntoOptionValue`); front-end warnings for the shapes it cannot rewrite. | `caaaf3c`; related fix `8157e66` (see below) |
| #29 | Outou-named state API: `State<T>`, `use_state`, `provide_context`, `use_context`, `launch`; the old names are removed from the prelude; `dogfood-board` and the prelude name list are migrated and pinned. | `997e851`, `4b741c9` (test: avoid `clone_on_copy` flagged by clippy 1.99) |
| #30 | This close-out: ledger row 46, Friction log dispositions, this section. | (this change) |

The decision itself is `0d75d17` (ADR 0014) and the issue mirrors are `79e1693`. Every dogfood-board friction item F1 to F6 and F18 now has a disposition that matches this table; F13 (no click-capable test renderer) remains open; F17 (editor experience) was checked in a real editor on 2026-10-25 (below); see `examples/dogfood-board/README.md`.

### When to switch to an Outou runtime, and the evidence so far

ADR 0014 reopens the decision if any of five conditions holds. The state of the evidence at the end of the alpha:

| Condition | State |
|---|---|
| Borrowed props or non-`Clone` props become a 1.0 requirement | No 1.0 requirement has been written; not triggered, not evaluated. |
| One Dioxus upgrade costs more than a limit to be fixed | No upgrade was made during the alpha (`Cargo.lock` stays at `dioxus-core 0.7.10`), so the cost of absorbing one in `__private` and the wrappers is **unmeasured**, and no limit is set. |
| React interop needs control of the renderer | Not designed and **not evaluated**. |
| Release bundle size or indexing cost is unacceptable | **Release bundle size not measured**: only the debug wasm of the dogfood application was built and it was not bundled or loaded. Indexing cost: A1 measured the absolute rust-analyzer cost of the two-process layout; the difference against a smaller runtime was not measured. |
| Rows keep being added where the same JSX means something different and the facade cannot absorb it | None added after row 37 (fixed by #28). Rows 39 to 46 are resolutions, mitigations and residue, not new "same JSX, different meaning" cases. |

Runtime **performance** was not measured at all. These gaps are the stated reason no runtime is built; they are not evidence for or against one.

### Signs that the recommendation is wrong: current state

- (a) *The facade wrappers break substantially at every Dioxus upgrade.* **Untested**: no upgrade was attempted.
- (b) *After wrapping the state API, guard or copy semantics still show through.* **Partly present.** No guard is exposed and the F4 form cannot be written. What shows through is the backend's re-entrancy rule: `d.update(|x| *x = Draft { title, ..d.get() })` compiles and panics when it runs (in Outou's words), and subscription behavior is the backend's (row 44). Whether this counts against the recommendation is a judgement left to the next application.
- (c) *In the next application, friction from the four unfixable groups dominates.* **Untested**: there is no next application. In the one dogfood application, props bounds produced one accepted friction item (F15) and no Friction log item names the other three groups; the friction that did hurt (F1 to F6, F18) was in the fixable groups.

### Side findings during A5, and what was done

- **`outou fmt` failed on any file with eleven or more JSX regions.** The formatter counted its placeholders with a substring match, so `__outou_fmt_jsx_1` matched inside `__outou_fmt_jsx_10`. Not caused by A5: an existing bug at HEAD, found while formatting the dogfood application. Fixed in `967d8d4` (identifier-boundary counting; regression tests for 11, 12 and 100 regions).
- **`outou fmt` and the language server's formatting refused a file that had only warnings.** The first front-end warning (`<select value>`, #28) made them fail. Fixed in `8157e66`: only a diagnostic of severity Error refuses.
- **A stale target directory in the matrix publish test.** `cargo package` unpacks `outou` into an overlay registry whose files carry a fixed old mtime, so a target directory shared across runs could treat an earlier `outou` rlib as newer than the new sources and build against the old facade (`cannot find function child`). Fixed in `27d851f` with a per-run target directory and a per-process counter for temp directories. Not a product bug.
- **Test lessons.** CI output is coloured, so the leakage checks of `outou cargo` strip ANSI before scanning (`6bea1a0`); counting warnings by substring was wrong and became a count by diagnostic line (`628fdd3`); the `outou cargo` tests that build Dioxus from a cold target directory are `#[ignore]`d (`crates/outou-cli/tests/cargo_cli_real.rs`); clippy 1.99 flags `clone_on_copy`, fixed in `4b741c9`.

### Known gaps at the end of A5

Each is recorded where it belongs; none is hidden by a passing test.

- `outou build` and `outou cargo` do not print front-end warnings (the `<select value>` shapes it cannot rewrite); `outou check` and the language server do (ledger row 43, `docs/grammar.md` §5.1, `TODO(alpha) (#31)`).
- A key written as the raw identifier `r#key` is not normalised to `key` (row 41, known gap (b)).
- In an attribute value, `bare_rust_expr` (`crates/outou-backend-dioxus/src/attribute.rs`) tests only for a top-level `;`, so an island of several statements with no `;` (a block-like expression followed by another expression) is judged a single expression and is lowered without braces. The child side was fixed in #27 (`expr_shape`, row 42); the attribute side was not, and the behavior is the same as at HEAD before A5.
- Misuse messages can still carry the backend's vocabulary: a `State<T>` or `Signal<T>` used as a child where `T` is not renderable, and `Send`/generational-box types in rustc's error for a non-`Send` state (rows 42 and 45(b)); `IntoChild<Delegated<_>>` notes survive under plain `cargo`. The Outou wording for them is a `TODO(alpha) (#31)` that the decision keeps open.
- `&String`, `&&str`, `Arc<str>`, `Rc<str>`, `Cow<str>`, `Box<str>`, `Vec<Element>`, an iterator of text and `Option<number>` are not children; the diagnostics say how to write each (row 42, Gaps (b) and (d)). The `IntoKey` types are `str` and integer types; `char`, `bool`, floats and a custom `Display` type are no longer keys (row 41).
- Run-time output is not translated: a panic inside `cargo test` still prints `src/.generated/x.rs:L:C` (row 39), and plain `cargo` prints generated paths. `include_str!`-style relative paths still resolve against the generated directory (row 40). `outou cargo` is Unix-only for the alpha.
- Editor behavior: hover, references and rename on `key` and other lowered expressions are checked by `outou-lsp`'s mapping-layer unit tests against real generated code, and the 2026-10-25 editor check did not touch them. The editor experience on the dogfood files (F17) was checked by hand in VS Code on 2026-10-25 (below); the related-information fields of the `IntoChild` error were not looked at there (row 42). The keyword-named attribute rename/references gap (row 29, now `TODO(alpha) (#31)`) is unchanged.
- Existing bug, recorded in A3 and not fixed: on a case-insensitive filesystem, `outou build`'s `clean_stale` deletes a generated file whose only change is its case, and exits 0 (recorded under "(f)" in A1 above, from issue #20).
- Flaky tests, as observed during the A5 work: `outou-cli`'s formatter test that runs a generated shell script as `rustfmt` can fail with `ETXTBSY` (text file busy), and the three `outou-syntax` timing tests (`*_in_linear_time` in `crates/outou-syntax/tests/item_recovery.rs`) can fail under load, since they assert a fixed wall-clock bound. This was reported by the work's reviewers; I did not reproduce either and they are not recorded elsewhere in the repository.

### Browser check of the migrated application (2026-10-24)

Driven by Fable through the Chrome extension, `dx serve --platform web`, no console errors in any of the three runs:

- At the state of commits `caaaf3c` / `8157e66` (#28): the new-task form shows Priority Medium, the edit form of a High task shows High, and the detail view keeps Status "In progress" after opening another task and coming back (the generated `<select value>` form, ledger row 43).
- At the first #29 implementation (the `State<T>` migration, before `Slot`): a full walkthrough - search `ken`; the status chips (To do / Done / All) combined with search; New task (an empty-title error keeps the Assignee already typed, a valid save opens the detail); Edit (current values pre-filled, Save is reflected); a Status change in the detail updates the list and the chip counts; the list checkbox toggles done / not done; opening another task with a form open switches the form to that task's content; Delete returns the panel to the hint.
- At commit `997e851` (after the review fixes: `Slot`, Outou-worded `try_*` panics, `#[track_caller]`): a smoke run only - checkbox toggle with count update, detail view, search input.

### Not verified

- The parts of the browser walkthrough of `examples/dogfood-board` not run on the migrated application: the form length limits (covered by `board-core` unit tests only) and New task then Cancel. The commit `997e851` (after the `Slot` review fixes) got only a smoke run, not the full walkthrough; the full walkthrough was done at the first #29 implementation, before `Slot`.
- The `dogfood-board` CI job on a pushed branch after the migration.
- What the 2026-10-25 editor check left open: see its "Not verified" list below (other editors, Windows and Linux, long sessions, large or multi-folder workspaces).
- Runtime performance, release bundle size, React interop, Dioxus upgrade cost.

### Real-editor check of outou-lsp (F17, 2026-10-25)

Run by hand by the project's manager: VS Code 1.142.0 (not Cursor), an Extension Development Host started with `code --extensionDevelopmentPath=.../packages/vscode-outou .../examples/dogfood-board/app`, `vscode-languageclient` 10.1.2, on `app/src/task_list.rsx`, following the checklist in `packages/vscode-outou/README.md`. This is the first time `outou-lsp` was used in a real editor; the browser checks above did not involve one. Every item was OK when the workspace was opened by its real path. Opened through a symbolic link, the items needing rust-analyzer failed until fix 3 below; after it, items 4c/4d, 3d and 7 were re-run through the link and worked.

| Item | Result |
|---|---|
| 1 Starts | OK: Output shows `Starting .../outou-lsp` |
| 2 Progress | OK: status bar `Outou: indexing: 63/283`, then `Outou: checking`, then gone; no rust-analyzer or crate name |
| 3a Syntax diagnostic (`</p>` to `</q>`) | OK: ``closing tag `</q>` does not match opening tag `<p>` ``, source `outou` |
| 3b Unclosed tag (`/>` removed) | OK: ``unexpected `}` inside tag `<TaskRow>`, expected an attribute or `>` `` |
| 3c Type error | OK after save: `mismatched types` (E0308); the related location is a line and column of the `.rsx` file; source `rustc` |
| 3d Old API (`get` to `read`) | OK after save: ``no method named `read` found for struct `outou::prelude::State<T>` `` (E0599); no `Signal` or Dioxus wording |
| 4a Tag completion | OK: shown on `<`, narrows as typed, includes `TaskDetail` and `TaskForm` from other files and HTML elements |
| 4b Attribute-name completion | OK after fix 2: `ch` offers `checked`, `on` offers `onclick` and the other handlers |
| 4c, 4d Rust-expression completion | OK after fix 3: `app.` offers `board`, `filter`, `panel`; `app.board.` offers `get`, `with`, `set`, `update` |
| 5 Hover | OK: `pub board: State<Board>` and its doc comment; no `Signal` |
| 6 Go to definition | OK (`Cmd`+click) |
| 7 References | OK (context menu, Find All References) |
| 8a Rename | OK (context menu, Rename Symbol) |
| 8b Rename of the keyword-named prop `type` | Refused, as the known limitation says (ledger row 29, #31) |
| 9 Formatting | OK |
| 10 Semantic highlighting | OK |
| 11 serverStatus | OK: a toast |

**Defects found only by the real editor, and what was done:**

1. The extension passed an ordinary `OutputChannel`, and `vscode-languageclient` 10 calls `outputChannel.error(...)`; the client failed to start with `outputChannel.error is not a function`. Fixed by creating a log output channel (`2753023`).
2. Attribute-name completion on HTML elements always returned an empty list: the Phase 0 limitation "no attribute vocabulary" was only found in an editor. Fixed with an attribute table taken from `dioxus-html 0.7.10` (262 names, every name compiled on its element), excluding attributes already on the tag and treating a lowercase tag as an element (`f7f052b`, `f29ee1e`; ledger row 51; the table's gaps are owned by #31).
3. A workspace opened through a symbolic link (`~/company/workspace/outou-rs` to `~/Documents/projects/outou-rs`) made the editor send `.rsx` URIs in the link's spelling while `outou-lsp` keyed documents by the real path. Forwarded completion, hover and definition answered `null`, `didSave` never reached rust-analyzer, and no rustc diagnostics appeared. Fixed by normalizing URIs at the server boundary, recording the editor's spelling in an alias table, and writing it back into diagnostics, `Location`s and `WorkspaceEdit`s (`875e766`; the next commit, `6024556`, handles several spellings of one document at once, clears diagnostics when the spelling switches, and closes a document only when its last spelling closes). Ledger row 52. Confirmed in the editor on 2026-10-25: through the link, completion after `app.`, the `read` diagnostic and references work.

**Findings about the environment and the design:**

- The workspace has to be the crate directory (`examples/dogfood-board/app`). Opening the Cargo workspace root `examples/dogfood-board` puts `outou-lsp` in its degraded mode (syntax diagnostics only).
- On macOS `Ctrl+Space` collides with Google Japanese Input; use the Command Palette's "Trigger Suggest". `Shift+F12` and `F2` are function keys on a Mac keyboard; use the context menu or the Command Palette.
- The grey inline AI suggestions are not completions; `editor.inlineSuggest.enabled` off avoids the confusion. VS Code's word-based suggestions can also appear mixed into the list.

**New known gaps, owned by #31:** (a) the Output channel "Outou Language Server" shows rust-analyzer's whole standard error (`WARN overly long loop turn ...`, `PrimeCaches`, the full text of each `didSave`, which is the generated Rust and its `.generated` path), every line at `[error]` level (ledger row 53; a leak of generated code and backend vocabulary to the user); (b) rust-analyzer's own `non_snake_case` warning appears on `#[component]` functions (``Function `TaskList` should have snake_case name``); (c) opening a `.rsx` file that already has flycheck diagnostics publishes an empty set until the next save (an existing behavior, seen with the real path too); (d) rust-analyzer's file-watch globs use the real path, so with a symlinked workspace on-disk `.rs` edits may not reach it (a guess, not checked); (e) non-`file:` URIs, Windows and `file://localhost/` URIs (already in #31). **Update:** (a) and (b) are fixed and confirmed in a real editor on 2026-10-25 (ledger rows 54 and 55): rust-analyzer's standard error is dropped by default (`OUTOU_LSP_RA_LOG=1` shows it), the extension shows server stderr at `[info]`, and codegen writes `#[allow(non_snake_case)] ` before a PascalCase `#[component]` function. **Confirmed in a real editor (2026-10-25, workspace opened through the symbolic link `/Users/kiyoshi/company/workspace/outou-rs/outou/...`, binary built at HEAD `c068fe2` with `cargo build -p outou-lsp`, then Reload Window):** (a) the Output channel "Outou Language Server" shows only `Starting ...` and `outou-lsp: ...` lines at `[info]`, with no rust-analyzer log (`WARN overly long loop turn`, `PrimeCaches`) and no generated Rust; (b) no `should have snake_case name` warning on `TaskList` (line 7) or `TaskRow` (line 34). Hover on `board.get` showed `outou::state::State`, `pub fn get(&self) -> T`, `T = Board` and "A clone of the current value. Subscribes the running component.", with no `Signal`. One observation about files changed outside the editor: after `git checkout` restored a file from outside VS Code, the old diagnostic (E0615) stayed until the file was saved with `Cmd+S` and disappeared a few seconds after; this is the known absence of `didChangeWatchedFiles` handling (#31, "Language server").

**Not verified:** other editors (Cursor and so on), VS Code on Windows or Linux, a session of half a day or longer, a large workspace, several workspace folders, VS Code's native "Save changes" delay, and the Trace log (`outou.trace.server` with the channel's log level at Trace). The mapping-layer claims about `key` and other lowered expressions (rows 41 and 42) are still backed by unit tests only; the checklist did not touch them.

### Issue #23 and #30 checklist status

Done: ADR 0014 (supersedes ADR 0003, amends ADR 0010); the ledger weighed (recount above); the A4 evidence folded in (F1 to F18, A4 section); the A1 numbers are the performance evidence available and the runtime-level performance is stated as unmeasured; implementation issues #25 to #29 closed; ledger row 46; Friction log dispositions; this section and the A2 corpus figure. Open, and stated above rather than treated as passed: the editor check's remaining gaps (see its list).

## Cross-cutting: disposition of the Phase 0 deferred-work markers (issue #24)

Status: done on 2026-10-24. The filtered command of `docs/alpha.md`'s cross-cutting triage, which found 44 lines at HEAD `bf71bc0` (not the 46 recorded when the alpha started: two had already gone), now finds **0 lines**. The one line `docs/alpha/issues/24-todo-triage.md` expected to stay, `docs/adr/0013-rename-translation-and-refusal.md:30`, was reworded so that it no longer matches: it was an explanatory sentence, not a deferred item. Every deferred item is owned by issue #31 (`docs/alpha/issues/31-post-alpha-backlog.md`) and carries `TODO(alpha) (#31): <description>` in the source or document.

Counts of the 44 lines: **4 fixed**, **16 accepted** (permanent, deliberate; reason written where the marker was), **22 re-tagged** `TODO(alpha) (#31)` (the one README heading of the formatter that covered five items became four re-tagged items and one accepted item; counted once as re-tagged) and **2 wording-only** (the ADR 0013 sentence; the history sentence in this document). Locations are as of the end of the change.

### Fixed (4)

| Item | Change |
|---|---|
| `crates/outou-syntax/src/parser/jsx/children.rs`, a `<` in element content with no tag name | A real bug, present since Phase 0 (see below). Now diagnosed. |
| `crates/outou-fmt/src/rustfmt_proc.rs`, stdin written inline | `rustfmt`'s stdin is written on its own thread while the caller drains stdout and stderr; a `BrokenPipe` on the writer is ignored only when `rustfmt` exits with a failure (it stops reading on a syntax error), where the exit status decides; a successful exit after a broken pipe (output for a truncated input) is an error. Test `a_successful_exit_that_did_not_read_all_input_is_an_error` uses a script that reads one byte and exits 0. Test `a_large_input_does_not_deadlock_against_a_program_that_echoes_it` pipes 4 MB through a script that runs `cat` and fails by timeout (60 s) if it hangs. |
| `crates/outou-lsp/src/documents/workspace.rs`, `load_unit` | Reads the file and delegates to `load_unit_with_text`; the duplicated body is gone. No behavior change (the existing language-server tests pass). |
| `crates/outou-lsp/README.md`, the SKIP summary | Rewritten to match each item's disposition. The reference to `completionItem/resolve` was stale (no marker for it exists in the code) and is removed. |

### Accepted (16), with the reason

| Item | Reason |
|---|---|
| Doc tests containing JSX are not lowered (`examples/phase0-app/src/components.rsx`, golden `components/expected.rs` and `expected.map.json`, regenerated with `BLESS=1`) | Doc tests are plain Rust by design. |
| `crates/outou-cli/README.md`: a plain `.rs` file cannot declare an `.rsx` child module | Codegen never rewrites a plain `.rs` file, so the declaration cannot be pointed at the generated output. The user-facing message now reads "`.rsx` modules must be declared from `.rsx` files or from the crate root" (it said "Phase 0 requires"; `crates/outou-cli/src/build/plan.rs` and its test). |
| Workspace member globbing (README, `workspace.rs` doc and the `default-members` doc, 3 markers) | Only a trailing `/*` is supported and `default-members` is not read (`outou build` and `outou cargo` always cover every member). The earlier text said an unsupported pattern was treated "silently" as a literal path; it was not silent, since `"crates/**"` ended in an operating-system error. It is now an explicit `WorkspaceError::UnsupportedGlob` ("only a single trailing `/*` is supported (list the member paths explicitly)"), with a test; this was an optional small fix. A directory whose name really contains `*`, `?` or `[]` (`lit/q?x`, `crates/foo*`) and holds a `Cargo.toml` is a literal member, as in Cargo (test `a_member_directory_named_with_glob_characters_is_a_literal_path`). |
| `outou package`: other `-p` packages are passed through unverified (README, `package.rs`) | Verification covers the crate at `--manifest-dir` only. |
| `outou fmt` writes in place, not atomically (`fmt.rs`) | Written in place; a failure part-way through the write (a full disk; reproducible with `ulimit -f`) can leave the file truncated. A temporary file next to the resolved target plus a rename would fix that and still keep a symlink a symlink, but it was left out of the alpha. Listed in #31. |
| A symlinked `.rsx` file is written through (`rsx_files.rs`) | Ordinary Unix `write` semantics. |
| A diagnostic with several `.rsx` sources is shown at the first (`crates/outou-lsp/src/mapping.rs`) | ADR 0007's reverse-mapping rule asks for one location. |
| `mapping.rs` test fixture duplication | No longer a verbatim copy; the marker is removed. |
| Non-XID bytes above 0x7F accepted in a `JsxName` (`lexer/mod.rs`, two markers) | A full XID check would add a dependency to turn one rustc error into one Outou error. Written in `docs/grammar.md` §5 as an intentional leniency. |
| Hard tabs (`crates/outou-fmt/README.md`, ADR 0011) | Space-based formatting is the specification. |
| The hand-written TOML parser of `xtask/src/corpus/lock.rs` | A small hand-written parser is kept: it works, has its own tests and belongs to an internal tool. The earlier justification (no TOML dependency) is no longer true, so the text now says that. The one divergence from TOML, a trailing `# comment` on a value line, is fixed (test `accepts_a_trailing_comment_after_a_value`). |

### Re-tagged `TODO(alpha) (#31)` (22)

Formatter (`crates/outou-fmt/README.md`, `width.rs`, ADR 0011): CRLF; `max_width` not read from `rustfmt.toml`; the fit check uses the line's indent instead of the placeholder's column (`let some_long_variable_name_here = <div …/>;` stays at 115 columns); the fixed 4-space dedent; current directory and edition not forwarded (edition 2021).

Language server: rename and references for a keyword-named prop (`mapping.rs`, README, ADR 0013; ledger row 29, status unchanged); `prepareRename` on a closing tag returns the opening tag's range (`rename.rs`, its test comment, README, ADR 0013); lexical URI comparison (`uri.rs`, README); Windows drive-letter URIs (`uri.rs`); tag and attribute-name completion re-parses every `.rsx` file per key (`complete.rs`); `textDocument/formatting` is synchronous, without a deadline, and ignores `options` (`dispatch/requests.rs`); `replan` leaves the workspace empty on a mid-way failure (`workspace.rs`); only open documents are overlaid, `didChangeWatchedFiles` is not implemented (`workspace.rs`).

Grammar: the `#[react_import]` payload (`docs/grammar.md`, §1). Also re-tagged in this change (included in the 22), already `TODO(alpha)` but without an owner: fragments (`parser/jsx/mod.rs`), child types (`crates/outou/src/lib.rs`, README), Windows (`crates/outou-cli/README.md`), front-end warnings not printed by `outou build` and `outou cargo` (`crates/outou-cli/src/build/emit.rs`, `docs/grammar.md` §5.1, this document), and the "Phase 0" wording in user-facing messages (`crates/outou-modules/src/error.rs`, `crates/outou-syntax/src/parser/diag.rs`). The flaky tests (ETXTBSY, `outou-syntax` timing) are listed in #31 only. `< span/>` (a space after `<`) in child position is a design question recorded in #31; it is refused for now.

### Wording-only (2)

`docs/adr/0013-rename-translation-and-refusal.md` (the sentence about path-qualified component tags no longer contains the marker) and the history sentence of the A2 section of this document. In addition, the references to the marker for row 29 in this document now use the new form.

### The bug found while fixing the `children.rs` marker

The marker said "a byte-precise message for this shape is not specified". The shape was worse than unspecified. In element content, a `<` that is not followed by a tag name (`<div>a < b</div>`, `<div><3></div>`, `<div><<A/></div>`, `<div>< span/></div>`) produced an incomplete tag **without any diagnostic**, which breaks the invariant stated at the top of `crates/outou-syntax/src/parser/mod.rs` (every incomplete tag has a diagnostic). Effect at HEAD: `outou check` printed no errors, and `outou build` (Strict) exited 0 and wrote a recovery element into `src/.generated/`. Since Phase 0 (it is the first `parse_nested_child_element` branch, which pre-dates the alpha).

Fix: the parser now reports ``expected a tag name after `<`; write `{"<"}` to include a literal `<` in text`` at the `<` (``...; write a comment as `{/* ... */}` `` for `<!--`) and keeps the existing recovery (skip the `<`, resume scanning). `outou check` exits 1 and `outou build` refuses (checked by hand on `lt-in-child-text.rsx`). Tests: four fixtures with `.expected` in `tests/fixtures/incomplete/` (`lt-in-child-text`, `lt-before-digit-in-child`, `lt-before-lt-in-child`, `lt-space-before-name-in-child`), the row in `docs/grammar.md` §9, and `crates/outou-syntax/tests/child_lt.rs`, which also checks that `{a < b}` in an island, `if a < b` in a child island and in an attribute value, and nested elements stay free of this diagnostic. The recovery fixtures also run through the Recovery compile gate (`every_incomplete_fixture_compiles_with_no_parse_errors`, ignored test). **The earlier version of this sentence said that the gate passes; it did not check anything, see the correction in the A2 section.** The four `lt-*` fixtures failed it once it checked (a bare `::outou::__private::recovery_element()` as a child is not valid `rsx!` syntax). A space after `<` is not accepted as an element in child position (it is, at the top level); that is a design question and is recorded in #31.

### The Recovery compile gate fix (review of this change)

The review found the gate described in the A2 section empty. Changes: (1) the gate now counts a rustc error with `code: null` as a parse error, except the `aborting due to` summary line, and lists all failing fixtures; (2) in child position a recovery element is written `{::outou::__private::recovery_element()}` (the form `rsx!` accepts; attribute and top-level positions are unchanged), which fixed the five fixtures `attribute-value-missing-before-truncated-closing-tag` and the four `lt-*`; the golden files did not change, since none has a recovery element as a child; (3) `unclosed-island-mid-file`: an island that lost its `}` ran on into the enclosing `</ul>`, and the parser left it in the Rust text, because `<` after an operand is a less-than. `</` followed by anything but `/*` or `//` is never Rust, so it is now read as a closing tag wherever it stands (`try_jsx_at` in `crates/outou-syntax/src/parser/region.rs`); the fixture gains the diagnostic ``closing tag `</ul>` has no matching opening tag`` (`.expected` updated), and Recovery codegen leaves no placeholder for such a tag after an operand (`map(..) recovery_element()` is not Rust). After this, the gate checked 33 fixtures and all passed. Ledger row 49.

### Corpus re-run

`cargo xtask corpus test`, same pinned corpus (`rust-lang/rust` tag `1.98.1`, commit `48a229ceaefd`, 20,722 files): panics 0, timeouts 0, splice mismatches 0, JSX mis-detections 20, and false positives **33, of which 0 on valid Rust and 33 on Rust that rustc rejects**. The headline figures are identical to the A5 close-out run above (the new diagnostic created no false positive on valid Rust); the per-file diagnostic counts rose in two files, `issue-57819.rs` 11 to 17 and `range-exclusive-dotdotlt.rs` 65 to 67, both Rust that rustc rejects. The final run, after the closing-tag change described above, gives these figures. (The corpus is plain Rust, in which JSX is detected only in the `diff-markers` files, so it exercises the new branch very little; the fixtures and `child_lt.rs` are the real tests.) The command was run three times by mistake instead of once; the three reports were identical.

### Decision recorded

`< span/>` is not accepted in child position, for now. Not decided: whether it should be, as at the top level (#31).
