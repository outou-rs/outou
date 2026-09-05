# outou-syntax

Mode-aware lexing primitives, a recovering parser, and the AST for `.rsx` sources. Produces Outou syntax diagnostics in Outou vocabulary.
Normative grammar: `docs/grammar.md`.

There is exactly one compiler: `parser::parse` is the only entry point that produces an AST. An earlier, free-standing, context-free tokenizer (`lexer::tokenize`) was removed (issue #4, decision D5): nothing in the workspace called it, and it could not perform the parser's own closing-tag matching against the open-tag-name stack, so it inevitably drifted from the parser's own decisions. `lexer` now exposes only the scanning primitives (`disambiguate`, `position`, `opaque`, `rust_token`, `scan_jsx_name`) that the parser itself drives.

## The Phase 0 round-trip contract (source-driven splicing)

The AST does not carry enough information to regenerate Rust source on its own — `Expr::Rust` and `ast::Item::Rust` nodes hold verbatim source slices, not a structured Rust AST. Instead, every construct that mixes Rust and JSX is guaranteed to exactly partition its own byte range, so the original source can always be reconstructed by walking the tree and splicing:

1. Every `JsxElement.span` is the exact byte range of that element in the `.rsx` source — from its `<` to just past `/>`, its matching `</name>`, or its recovery terminator. No span is ever widened over adjacent trivia.
2. `Island.parts`, `Block.statements` + `tail`, and `RustItem.parts` each **exactly partition** their construct's content range: no gaps, no overlaps.
3. Codegen emits, for each `Expr::Rust`, `source[span]` verbatim; for each `Expr::Jsx`, generated Rust. Leaf nodes *inside* a JSX element (tag spans, attribute spans, text spans) are informational only (for the source map and the LSP) and are explicitly **not** required to partition the element.

This is what lets a JSX element appear anywhere Rust allows an expression — a `const` initializer, an `impl` method body, any position inside a Rust expression island — without the parser needing a structural Rust grammar: everything that is not JSX is kept as an opaque, verbatim slice, and the contract above guarantees nothing is lost or duplicated when those slices and the JSX nodes are put back together in order. `tests/fixtures.rs::source_round_trips_by_splicing` and `tests/fixtures.rs::jsx_element_spans_are_exact` check this contract directly; `assert_full_coverage` checks it recursively for every fixture, including inside `Island.parts` and `RustItem.parts`.

Phase 0: Week 3 (Gate 1).
