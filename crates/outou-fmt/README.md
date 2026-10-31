# outou-fmt

Formats `.rsx` source text: placeholder JSX regions, `rustfmt` for the Rust around them, then a JSX-aware pretty printer for the JSX itself.

Phase 0: droppable item, issue #13 (`docs/phase0/issues/13-formatter.md`), taken up after Gates 0–4 passed (`docs/phase0.md`).

## Overview

[`format_source`] is the one pipeline both `outou fmt` (`crates/outou-cli`) and `textDocument/formatting` (`crates/outou-lsp`) call — never two separate implementations. See [`docs/adr/0011-formatter-placeholder-rustfmt-splice.md`](../../docs/adr/0011-formatter-placeholder-rustfmt-splice.md) for the design and its trade-offs.

- A file with a syntax error is refused (`FormatError::SyntaxErrors`), never partially formatted.
- `rustfmt` is required on `PATH` (never reimplemented); its absence or failure is an explicit `FormatError` from this crate, never silently swallowed into "nothing to format". `outou fmt` prints that error and exits non-zero; `outou-lsp`'s `textDocument/formatting` logs it to its own stderr and answers with no edits rather than an LSP error response (see the ADR's Consequences). `FormatOptions::deadline` bounds the whole call: past it `rustfmt` is killed and `FormatError::TimedOut` is returned (`outou-lsp` sets it; `outou fmt` does not). Only the direct `rustfmt` process is killed: a wrapper script that runs `rustfmt` without `exec` can leave a grandchild running, though the call still returns on time.
- Each call runs `rustfmt` in the formatted file's directory (`FormatOptions::source_path`), so the project's `rustfmt.toml` is found wherever `outou fmt` / `outou-lsp` was started, and with the crate's edition (`FormatOptions::edition`, or `edition` from the nearest `Cargo.toml`, honouring `edition.workspace = true`; 2021 otherwise). `max_width`, `tab_spaces` and `newline_style` are read once per call from `rustfmt --print-config current` (one extra short process; defaults 100, 4, `Auto`) and drive the JSX printer: a JSX element is kept on one line only if the whole line fits, counting the column it starts at and what follows it.
- Line endings follow `newline_style`: `Unix`/`Native` give LF, `Windows` gives CRLF, `Auto` (default) keeps the first line ending of the input, so CRLF in is CRLF out and mixed endings are made uniform.
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
- `ctx.rs` — the settings one call runs with (edition, working directory, `max_width`, `tab_spaces`, `newline_style`), resolved once and passed down; the one place that asks `rustfmt --print-config`.
- `project.rs` — the crate's edition from `Cargo.toml` (and `[workspace.package]`) and the working directory for `rustfmt`.
- `newline.rs` — CRLF normalisation and restoration.

## Testing

- `tests/project_config.rs` — behaviour that depends on the project: edition detection, `rustfmt.toml` discovery and `max_width` / `tab_spaces` / `newline_style`, the column-aware fit check, CRLF.
- `tests/golden.rs` + `tests/golden/*/{input.rsx,expected.rsx}` — golden fixtures, each checked for both the exact expected output and idempotency.
- `tests/sweep.rs` — runs the formatter over every `.rsx` file under `tests/fixtures/` and `examples/phase0-app/src/` as a never-panics + idempotency + semantics-preserved sweep (a span-independent comparison of the parsed JSX trees before and after).

## Known limitations

- Accepted (issue #24): `hard_tabs = true` in a discovered `rustfmt.toml` is contained, not supported, and that is the specification: this crate's indentation is space-counted throughout. `rustfmt_proc.rs` always passes `--config hard_tabs=false` on the command line (which overrides it) and additionally refuses with `FormatError::TabIndentedOutput` if a tab ever shows up in `rustfmt`'s output anyway, rather than silently corrupting this crate's space-based indentation math. A project that genuinely wants tab-indented `.rsx` files gets spaces instead, always.
- A package whose `Cargo.toml` names no `edition` is formatted as 2021 (Cargo itself would assume 2015). An invalid `Cargo.toml` is ignored for edition detection (fallback 2021); pass `FormatOptions::edition` to override.
- `rustfmt`'s user-level configuration (`~/.config/rustfmt/rustfmt.toml`) is honoured when the project has no `rustfmt.toml` of its own; it is not read by this crate, so tests isolate it by pointing `HOME` and `XDG_CONFIG_HOME` at an empty directory.
- A symlinked `.rsx` file is resolved to its real location (`canonicalize`) before its `rustfmt.toml` and `Cargo.toml` are looked up; if that fails (the file does not exist yet) the path as given is used. Edition detection stops at the nearest `Cargo.toml`: one with `[workspace]` but no `[package]` supplies only `[workspace.package] edition`, never an enclosing crate's.
- A relative `FormatOptions::rustfmt_program` containing a path separator is resolved against the file's directory, not the process's, once `source_path` is set. Use a bare name or an absolute path.
- When one line holds several JSX expressions, the fit check of the first counts the others as their placeholder identifier (17 or more characters) rather than their formatted width.
