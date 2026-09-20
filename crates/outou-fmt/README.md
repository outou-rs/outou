# outou-fmt

Formats `.rsx` source text: placeholder JSX regions, `rustfmt` for the Rust around them, then a JSX-aware pretty printer for the JSX itself.

Phase 0: droppable item, issue #13 (`docs/phase0/issues/13-formatter.md`), taken up after Gates 0–4 passed (`docs/phase0.md`).

## Overview

[`format_source`] is the one pipeline both `outou fmt` (`crates/outou-cli`) and `textDocument/formatting` (`crates/outou-lsp`) call — never two separate implementations. See [`docs/adr/0011-formatter-placeholder-rustfmt-splice.md`](../../docs/adr/0011-formatter-placeholder-rustfmt-splice.md) for the design and its trade-offs.

- A file with a syntax error is refused (`FormatError::SyntaxErrors`), never partially formatted.
- `rustfmt` is required on `PATH` (never reimplemented); its absence or failure is an explicit `FormatError` from this crate, never silently swallowed into "nothing to format". `outou fmt` prints that error and exits non-zero; `outou-lsp`'s `textDocument/formatting` logs it to its own stderr and answers with no edits rather than an LSP error response (see the ADR's Consequences).
- Formatting is deterministic and idempotent: `format_source(format_source(x)?)? == format_source(x)?`.
- A JSX text child and an attribute's string value are always copied byte-for-byte from the source — never reflowed, never re-escaped.
- A JSX children region containing something the AST does not model (an empty island, i.e. a comment) is left byte-for-byte untouched rather than risking data loss.
- An expression island containing a multi-line string/raw-string literal, a multi-line block comment, or multi-line JSX text *anywhere in its subtree* (including inside a nested element's attributes or children) is left byte-for-byte untouched, braces included, rather than formatted at all — this crate's line-based reindentation cannot safely touch a token whose own content spans more than one physical line (`multiline_guard.rs`).

## Module layout

- `lib.rs` — public API (`format_source`, `is_formatted`, `FormatOptions`, `FormatError`).
- `collect.rs` — finds the JSX elements that are placeholder sites for one `rustfmt` pass.
- `placeholder.rs` — collision-free placeholder identifiers.
- `multiline_guard.rs` — detects a multi-line token or multi-line JSX text that this crate's line-based reindentation cannot safely touch.
- `snippet.rs` — the placeholder → `rustfmt` → splice pipeline for one region (a whole file, or one expression island wrapped in a throwaway function).
- `rustfmt_proc.rs` — spawns `rustfmt` over stdin/stdout.
- `jsx_print/` — the JSX pretty printer: tags and attributes (`tag.rs`), children layout (`children.rs`), expression islands (`island.rs`, which consults `multiline_guard` before formatting).
- `width.rs` — the line-width budget and indent step.

## Testing

- `tests/golden.rs` + `tests/golden/*/{input.rsx,expected.rsx}` — golden fixtures, each checked for both the exact expected output and idempotency.
- `tests/sweep.rs` — runs the formatter over every `.rsx` file under `tests/fixtures/` and `examples/phase0-app/src/` as a never-panics + idempotency + semantics-preserved sweep (a span-independent comparison of the parsed JSX trees before and after).

## Known limitations (`TODO(phase0)`)

- Line endings are always normalized to `\n`, even for a CRLF input file.
- The line-width budget is a fixed constant matching `rustfmt`'s own default (100), not read from a project's `rustfmt.toml`.
- A snippet formatted through the synthetic-wrapper path (an expression island) is dedented by a fixed 4 spaces per level; a `rustfmt` continuation line that adds a non-multiple-of-4 alignment could come out mis-indented by that dedent step.
- `hard_tabs = true` in a discovered `rustfmt.toml` is contained, not fully supported: `rustfmt_proc.rs` always passes `--config hard_tabs=false` on the command line (which overrides it) and additionally refuses with `FormatError::TabIndentedOutput` if a tab ever shows up in `rustfmt`'s output anyway, rather than silently corrupting this crate's space-based indentation math. A project that genuinely wants tab-indented `.rsx` files gets spaces instead, always.
- `rustfmt` is spawned with the process's own current working directory, so its `rustfmt.toml` discovery is relative to wherever `outou fmt`/`outou-lsp` was launched from, not necessarily the crate the formatted file belongs to; the Rust edition passed to `rustfmt` is likewise `FormatOptions::edition`'s default (`"2021"`), not detected per-crate — a 2015-edition file using `async` as a plain identifier would be refused by `rustfmt`, and a 2024-edition file gets 2021-style formatting for anything the two editions disagree on.
