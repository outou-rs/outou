# ui tests

rustc-style UI tests for Outou diagnostics (issue #11). Each case is a directory:

```text
tests/ui/<case>/
├── input.rsx
├── expected.stderr
└── NEEDS_CARGO_CHECK   # only for a "build" case, see below
```

`expected.stderr` is the exact rendered output for `input.rsx`, after path normalization, compared verbatim by `crates/outou-cli/tests/ui.rs`.

## Path normalization

Every occurrence of the case directory's own absolute path is replaced with the literal string `$DIR`, so `expected.stderr` is portable across machines and checkouts — the same convention rustc's own UI test suite uses. A `--> ` line therefore always reads `--> $DIR/input.rsx:L:C`, never an absolute path.

## Two kinds of case

- **Fast (syntax) cases** — no marker file. The harness calls `outou_cli::check::check_source` in-process (never by spawning the `outou` binary — "there is one compiler", `AGENTS.md`) and compares the rendered Outou syntax diagnostics. Runs on every `cargo test`.
- **Build cases** — a `NEEDS_CARGO_CHECK` marker file (empty) next to `input.rsx`. These exercise the "Rust semantic (mapped back)" and "backend (translated)" diagnostic layers: the harness generates the case's Rust, builds it in a shared throwaway crate against the real `dioxus` dependency tree, runs `cargo check --message-format=json`, and maps each diagnostic's span back through the source map to its `.rsx` position (translating any backend vocabulary the message still carries through `outou_syntax::vocabulary::translate_message`). `#[ignore]`d — a cold `target/` build can take well over 30s — and run once for the whole suite (every build case shares one throwaway crate, so the dependency build itself happens only once):

  ```sh
  cargo test -p outou-cli --test ui -- --ignored --nocapture
  ```

  A build case's `expected.stderr` is a checked-in file, not regenerated on every run; `crates/outou-cli/tests/leak.rs`'s backend-vocabulary scan reads it directly rather than rebuilding, so it stays cheap to run on every `cargo test`. Re-run the `--ignored` test (with `BLESS=1`, see below) after changing a build case's `input.rsx` or the translation table, to keep it honest.

## Blessing

Set `BLESS=1` to write `expected.stderr` from the actual rendered output instead of asserting equality, for either test:

```sh
BLESS=1 cargo test -p outou-cli --test ui
BLESS=1 cargo test -p outou-cli --test ui -- --ignored --nocapture
```

Always inspect a blessed diff before committing it — a `BLESS=1` run cannot tell a genuine fix from a regression that happens to produce different-but-still-wrong output.

## Provenance

`mismatched-closing-tag/` and `unterminated-attribute-value/` are copies of `tests/fixtures/{diagnostics,incomplete}/*.rsx` fixtures of the same shape (the originals stay where they are — they belong to the parser's own fixture suite, `crates/outou-syntax/tests/fixtures.rs`, and are not removed). `reserved-fragment/`, the `semantic-*` case and every `backend-*` case are new to this directory.

See `docs/backend-leakage.md` and `docs/phase0/issues/11-diagnostics-ui-tests.md` for which ledger rows each `backend-*` case exercises, and for the rows that describe a diagnostic but have no case here (with the reason why).
