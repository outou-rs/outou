# 0001. `.rsx` is a standalone source format

Status: Accepted

## Context

Earlier, Outou was going to be a procedural macro: JSX inside `outou::jsx! { … }` in ordinary `.rs` files. HTML-like syntax inside a macro invocation is an established approach in Rust UI code. It is safe, but it would only repeat that approach; it would not make JSX part of the language.

The alternative is a new file type, `.rsx`, in which JSX is an expression of the language itself. That is riskier: it needs a parser, a module resolver, Cargo integration, source maps and a language server, and it may turn out that rust-analyzer cannot be made to work well enough.

## Decision

Outou's source format is a standalone `.rsx` file. JSX is a first-class expression, not macro input. Phase 0 exists to find out whether this can be done without losing Rust's developer experience.

## Consequences

- The front end and the tooling are the product; a runtime is not built in Phase 0.
- If Phase 0 fails, the same grammar and parser fall back to the `outou::jsx!` macro and the standalone `.rsx` format is not adopted.
- Everything that rustc and rust-analyzer do for `.rs` files has to be arranged for `.rsx` files by Outou.

## Alternatives considered

- **Procedural macro only.** Zero tooling risk, but JSX stays macro input rather than a language expression. Kept as the fallback.
- **Preprocessor that rewrites `.rs` files in place.** Breaks the "source of truth" and every editor integration.
