# 0012. Semantic tokens: legend reuse and overlay precedence

Status: Accepted

## Context

`textDocument/semanticTokens/full` needs a *legend* — an ordered list of token type/modifier names that every response's numeric indices refer to — agreed between server and client at `initialize` time. rust-analyzer already computes rich semantic tokens for the generated Rust file (keywords, types, variables, …), but only Outou knows the JSX-specific vocabulary on top of it: a component reference vs. an intrinsic HTML element, an event attribute vs. a plain one, JSX text. Both sets of tokens have to end up in one response, in one shared legend, for one `.rsx` document.

Two decisions were needed: which legend to advertise (reuse rust-analyzer's, or define Outou's own and translate every index), and what happens when a rust-analyzer token and an Outou-native token would both claim the same span (the identifier written once in generated Rust for a JSX element name is exactly such a span — see ADR 0007).

## Decision

**Legend.** When a live rust-analyzer is attached, this server advertises rust-analyzer's own legend verbatim (read from its `initialize` response, `capabilities.semanticTokensProvider.legend`). Every rust-analyzer token's `tokenType`/`tokenModifiers` index is then used unchanged — no translation table. Outou's own overlay tokens are looked up **by name** in whichever legend is in effect: `class` (component), `type` (HTML element), `property` (attribute), `event` (event attribute), `string` (JSX text) — all standard LSP token type names, which rust-analyzer's legend always includes as a subset of its own (it extends the standard set, never replaces it), so the lookup never fails for a live rust-analyzer. In degraded mode (no rust-analyzer at all), the standard LSP token type/modifier list is used directly as the legend, so Outou's own JSX vocabulary still works with nothing rust-analyzer-specific mixed in.

**Overlay precedence.** Outou's own AST-derived tokens always win a JSX position. Any rust-analyzer-derived token whose mapped `.rsx` range overlaps an Outou-native token's range is dropped, never merged or layered: rust-analyzer only ever sees the *expanded* Rust, so at a JSX tag/attribute/text position its own classification (if it manages one at all inside the `rsx!` macro call) does not carry the JSX-specific vocabulary a user actually wants highlighted there.

**Mapping.** rust-analyzer's tokens are decoded from the generated file's delta encoding, converted to absolute `(line, start, length)`, and reverse-mapped through `outou_sourcemap::Registry::reverse` — the same many-to-many reverse-mapping rule every other feature uses (ADR 0007). A token whose generated span maps to several sources (an element name mapped from both its opening and closing tag) is emitted at each. A token with no source (synthesized code) is dropped. A mapped range that crosses a line break is split into one token per line, clipped to that line's content, since LSP forbids a single semantic token from spanning a line break.

## Consequences

- No index-translation table to keep in sync with rust-analyzer's own legend across versions: reusing it verbatim removes an entire class of bug.
- The advertised legend can differ between two runs of `outou-lsp` against the same file, depending on whether rust-analyzer attached successfully and what legend it happened to report. This is intentional and harmless: the legend is renegotiated at every `initialize`, and a client always reads the legend from the same `initialize` response the rest of its session is built on.
- A `.rsx` position that both rust-analyzer and Outou would classify (essentially every JSX tag/attribute name) is decided once, by Outou, with no per-token conflict-resolution heuristic needed beyond "does it overlap."

## Alternatives considered

- **Define Outou's own fixed legend and translate every rust-analyzer index into it.** Rejected: rust-analyzer's legend includes many extension types beyond the standard 23 (`lifetime`, `builtinType`, `selfKeyword`, …) that a fixed Outou legend would either have to enumerate exhaustively (coupling this server to rust-analyzer's exact vocabulary anyway) or silently drop tokens for, losing real highlighting fidelity for no benefit over reusing the legend directly.
- **Layer both token sets and let the client's own rendering pick a winner.** LSP semantic tokens are a flat, non-overlapping list by contract; producing overlapping tokens is not a supported response shape, so this was never actually available as an option.
