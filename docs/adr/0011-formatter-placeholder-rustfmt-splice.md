# 0011. Formatter: placeholder JSX regions, `rustfmt` for the Rust, then a JSX pretty printer

Status: Accepted

## Context

`.rsx` mixes ordinary Rust with JSX expressions (ADR 0004). Formatting it well means two different jobs: the Rust around JSX should format exactly as `rustfmt` would format it, and the JSX itself needs its own layout rules (attribute wrapping, children indentation) that `rustfmt` knows nothing about. `AGENTS.md` and issue #13 rule out writing a custom Rust formatter, and the grammar's round-trip contract (`docs/grammar.md` §9) already guarantees that a `.rsx` file becomes a valid, complete Rust file once every JSX expression in it is replaced by a Rust expression — a fact this design leans on directly.

## Decision

Format a `.rsx` file in four steps, implemented once in `crates/outou-fmt` and used by both `outou fmt` and `outou-lsp`'s `textDocument/formatting`:

1. Parse the file (`outou_syntax::parse`). A file with any syntax error, or any `ast::ErrorNode` even without one, is refused outright — never partially formatted.
2. Substitute every *top-level* JSX element (one not nested inside another JSX element's own tag structure) with a fresh placeholder identifier, guaranteed absent from the source by construction. The result is a complete, valid Rust file.
3. Run `rustfmt` on that file as an external process over stdin/stdout.
4. For each placeholder, in `rustfmt`'s output: read the indentation of the line it landed on, recursively format that JSX element (attributes, children, and any nested expression island's own Rust content — which goes through the same placeholder → `rustfmt` → splice pipeline one level down, wrapped in a throwaway function since an island's content alone is not a complete file), and splice the result back in at the placeholder's position.

Two invariants keep this safe:

- **Text is never reflowed.** Every JSX text child is copied byte-for-byte from its source span, which by construction always covers the entire maximal whitespace-normalization run (`docs/grammar.md` §8) — there is never leftover insignificant whitespace directly beside it to touch. Attribute string values are copied the same way.
- **What the AST does not model is left untouched.** An "empty island" (a comment, `{/* ... */}`) produces no child node at all (grammar §6). If any gap between children contains something other than whitespace, the whole children region is kept byte-for-byte rather than silently dropping it.
- **A token that spans more than one physical line is never reindented.** Step 4's "read the indentation of the line, then dedent/reindent every line" logic (both in unwrapping an island's throwaway-function wrapper and in laying its own multi-line result out) is only valid for *structural* indentation — a physical line inside a multi-line string/raw-string literal or block comment, or inside multi-line JSX text, is not structure, it is the token's own content, and blindly touching it corrupts that content (worse, a little more on every subsequent format). An expression island containing such a token anywhere in its subtree is therefore left byte-for-byte untouched, in its entirety, instead of formatted (`crates/outou-fmt/src/multiline_guard.rs`).

## Consequences

- Idempotent and deterministic by construction: every layout decision depends only on the AST and on `rustfmt`'s own (deterministic) output — including the multi-line-token guard above, whose own decision is a pure function of the AST and source text.
- `rustfmt` must be installed and on `PATH`; its absence or failure is an explicit `FormatError`, never a silent no-op, in the library itself. The two callers surface that differently: `outou fmt` prints it and exits non-zero (a CLI has no better option than telling the user directly), while `outou-lsp`'s `textDocument/formatting` logs it to its own stderr and responds with no edits rather than an LSP error response — the issue's spec calls for "never edit" on every failure a running editor session can hit, and a `rustfmt`-missing environment problem popping up as an error dialog on every format-on-save would be worse than one quiet log line an operator can go look for.
- Line endings: the pipeline works on `\n` text and applies `rustfmt`'s `newline_style` once to the result (see Amendment 2026-10-31, project settings).
- The line-width budget and the indent step are read from the project's `rustfmt.toml` (`max_width`, `tab_spaces`), defaulting to 100 and 4 (see the same amendment).
- A JSX region that itself has an unmodeled gap (a comment among its children) is reproduced verbatim rather than reformatted — correct, but such a region gets no layout improvement until the comment is moved outside JSX or the formatter is extended to preserve it structurally.
- An expression island containing a multi-line string/raw-string literal, a multi-line block comment, or multi-line JSX text anywhere in its subtree gets no layout improvement either, for the same reason and by the same trade-off: correctness over prettiness. A JSX element several levels deep inside such an island (e.g. a sibling `<span>` next to one holding the multi-line literal) is also left unformatted, even though its own content has nothing unsafe in it — the guard is whole-island, not per-line, because a per-line fix would need to track which physical lines are "inside" the unsafe token as the text is built up across several recursive calls, which is not worth the complexity for what is expected to be a rare shape.
- A discovered `rustfmt.toml` setting `hard_tabs = true` is contained rather than supported: `--config hard_tabs=false` is always passed on the command line, and a tab in `rustfmt`'s output is additionally treated as a distinct, refused error (accepted in issue #24 as the specification: there is no path to honoring hard tabs, since this crate's own indentation is space-counted throughout).
- `rustfmt` runs in the formatted file's own directory and with that crate's edition (see the same amendment).

## Alternatives considered

- **A custom Rust formatter for the whole file.** Ruled out by `AGENTS.md` and the issue itself: reimplementing `rustfmt`'s output exactly is a maintenance burden with no upside, and any divergence from real `rustfmt` output would make `cargo fmt --check` and `outou fmt --check` disagree.
- **Feed `rustfmt` the file with JSX left in place, ignoring its errors.** `rustfmt` cannot parse JSX at all; this only works if it tolerates and passes through unrecognized syntax region-by-region, which it does not.
- **One `rustfmt` invocation per JSX region instead of one per file plus one per nested island.** Slower (many more process spawns) for no accuracy benefit, since the placeholder scheme already lets a single pass see the real surrounding Rust context (its actual indentation) for the common case of top-level JSX.

## Amendment (2026-10-31)

The Consequences bullet on the two callers says `outou-lsp` answers with no edits on every failure. One exception was added with the formatting deadline (issue #31): when `textDocument/formatting` exceeds its 10 second budget, `rustfmt` is killed and the request is answered with an LSP error, `RequestFailed` (-32803) "Outou formatting timed out". Unlike a missing `rustfmt` or a syntax error, the user asked and nothing happened for a reason they can act on (a very large file, a stuck `rustfmt`). The library reports it as `FormatError::TimedOut`; `outou fmt` sets no deadline and never produces it.

## Amendment (2026-10-31, timeout answers null)

The amendment above was reversed the same day. When `textDocument/formatting` exceeds its 10 second budget, `rustfmt` is still killed, but `outou-lsp` now answers `null` (no edits) and logs one line to stderr with the file URI and the budget, instead of a `RequestFailed` (-32803) error response. An error popup on every format-on-save is worse than doing nothing, which matches this ADR's original rule that `outou-lsp` answers with no edits on every failure; there is no exception any more. `FormatError::TimedOut` and the `outou fmt` behaviour are unchanged.

## Amendment (2026-10-31, project settings)

Issue #31 closed the "formatter ignores the project" gaps without changing the placeholder splice scheme above. `format_source` first resolves one `Ctx` (`crates/outou-fmt/src/ctx.rs`) and passes it down the pipeline:

- **Working directory and config discovery.** `FormatOptions::source_path` names the file being formatted (`outou fmt` and `outou-lsp` set it). Every `rustfmt` process runs with that file's directory as its working directory, so `rustfmt` finds the nearest `.rustfmt.toml` / `rustfmt.toml` (and any user-level config) exactly as `cargo fmt` would, wherever the process was started. `--config-path` was not used: running in the directory also makes `--print-config` below agree with the formatting runs.
- **Edition.** `FormatOptions::edition` is now `Option<String>`. `None` reads `edition` from the nearest `Cargo.toml` with a `[package]` table above the file, following `edition.workspace = true` to the nearest enclosing `[workspace.package] edition`, and falls back to 2021 (also when a package names no edition, where Cargo itself would assume 2015: this crate's users all have a 2021+ toolchain and the generated code needs it). `Some(..)` is an explicit override.
- **Layout settings.** One extra `rustfmt --print-config current` run (same directory, edition and overrides) reports the effective `max_width`, `tab_spaces` and `newline_style`. This avoids parsing `rustfmt.toml` here and re-implementing its discovery; the cost is one more short process per call, before any formatting run. If that probe fails because the configuration is invalid, the formatting run fails with the same message; a missing `rustfmt` or a timeout is reported as before. The JSX printer wraps at `max_width` and indents by `tab_spaces`; the expression-island wrapper is dedented by `tab_spaces` instead of a fixed 4.
- **Fit check.** The decision to keep a JSX element on one line counts the real columns: the column the element starts at on its line (`let x = ` before it), the characters that follow it (`;`), and, inside an expression island, the columns the island will be shifted right by when spliced into the enclosing text. Continuation lines still line up with the line's indent, as before. When a line holds several placeholders, one that is still waiting to be replaced counts as its identifier.
- **Hard tabs** stay contained as decided in issue #24: `hard_tabs=false` is forced on every `rustfmt` run, `tab_spaces` is honoured.
- **Line endings.** The source is normalised to `\n` before parsing and `rustfmt` is run with `newline_style=Unix` so it cannot interfere. The result is converted once at the end by `newline_style` from the discovered config: `Unix` and `Native` give `\n` (Windows is out of scope for the alpha), `Windows` gives `\r\n`, and `Auto` (rustfmt's default) takes the style of the first line ending in the input, as `rustfmt` does. A CRLF file therefore stays CRLF, a file with mixed endings becomes uniform, and a lone `\r` is left alone. A multi-line string literal keeps its meaning because Rust reads `\r\n` in literals as `\n`.
