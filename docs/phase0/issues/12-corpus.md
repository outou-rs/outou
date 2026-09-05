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

- [x] `cargo xtask corpus fetch` reads `corpus.lock` and clones each entry at its pinned tag (or commit) into `.corpus/` — `xtask/src/corpus/{lock,fetch}.rs`. A shallow, cone-mode sparse checkout (`git clone --filter=blob:none --no-checkout --depth 1 --branch <tag>` + `sparse-checkout init --cone` + `sparse-checkout set <path>` + `checkout HEAD`) fetches only `path`; a `.outou-revision` marker skips a repeat fetch already at the right revision. Nothing is vendored into the repository (`.corpus/` stays gitignored).
- [x] `cargo xtask corpus test` runs the parser over every file and reports: panics, files that fail to parse, files whose JSX-free round trip differs — `xtask/src/corpus/{test,splice,report}.rs`. Each file is parsed in a `catch_unwind` on its own thread with a 2s wall-clock budget (a hung parse is reported as a timeout, not joined). Since the corpus is plain Rust, an Outou diagnostic on it is a false positive (reported, not a failure) and any JSX element found at all is by definition a mis-detection (reported as a round-trip mismatch, with its first offending span). A full summary is written to `.corpus/report.json`; a truncated markdown summary prints to stdout. Only panics and timeouts fail the run by default; `--strict` also fails on false positives and round-trip mismatches.
- [x] pin `corpus.lock` to a stable rust-lang/rust tag instead of a moving commit — pinned to tag `1.98.1` (the release matching the installed toolchain when this pin was chosen; its commit is `48a229ceaefd4985c50990b14116b6d856af0985`).
- [x] `.github/workflows/corpus.yml` becomes a real signal (remove `continue-on-error`) — removed; the nightly job now runs `corpus fetch` then `corpus test` (non-strict) and fails on a panic or timeout.
- [x] corpus categories from the test matrix covered: valid/invalid Rust, macro token trees, qualified paths, raw strings, lifetimes, generics — `corpus test` reports per-category file counts for `tests/ui/{macros,parser,generics,lifetimes}` plus `qualified paths` (filed under `fully-qualified-type` as of tag `1.98.1`) and two content-based categories, "valid Rust" (no `.stderr` companion) vs. "invalid Rust" (has one) and "raw strings" (a substring scan for `r"`/`r#`/`br`/`cr` forms). `crates/outou-syntax/tests/corpus_smoke.rs` adds 20 in-repo snippets from the same categories (raw strings containing `<`/`>`, comparison chains, turbofish, HRTBs, a macro body containing literal `<div>` text, byte/char literals, lifetimes, nested generics, `<T as Trait>::f`/`::N`) to the normal `cargo test --workspace` suite, asserting zero diagnostics and no JSX detected on all of them.

**Real run against tag `1.98.1`'s `tests/ui` (20,722 files, `.corpus` ~179 MB):** 0 panics, 0 timeouts, **61 false positives** (0.29% of files — all pre-2018-syntax constructs such as `builtin-superkinds-*.rs`'s `BuiltinBounds`-style multi-bound lists and old-style associated-type paths, where a bound list or path segment is misread as a JSX tag), 61 round-trip mismatches (one-to-one with the false positives here: every false positive in this run was a JSX mis-detection, not a splice-partition bug). The 61-file false-positive count is the Gate 4 signal this issue feeds; see `.corpus/report.json` (not committed) for the full file list after running `cargo xtask corpus fetch && cargo xtask corpus test`.

**Note on `cargo xtask corpus fetch` performance:** a plain (non-cone) `sparse-checkout set` combined with `--filter=blob:none`, or a cone-mode sparse checkout followed by `git checkout HEAD -- .` (an explicit `.` pathspec instead of a bare `checkout HEAD`), both fetch missing blobs one at a time against this corpus and did not finish inside 10 minutes in testing. `sparse-checkout init --cone` followed by a pathspec-free `checkout HEAD` fetches the sparse tree's blobs as one batch and finished in ~11s; see the comment in `xtask/src/corpus/fetch.rs::clone_sparse` for the measurements. The issue's documented ">10 min, fall back to a plainer `--depth 1` clone without the blob filter" path was evaluated but is not needed with this sequence.
