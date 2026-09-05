# 0004. The grammar extension is limited to JSX expressions

Status: Accepted

## Context

Earlier drafts had additional syntax: React-style `import` statements, JSX in macro bodies, postfix calls on elements, generic arguments in tags. Each one adds ambiguity to a grammar that already has to disambiguate `<` from Rust's less-than and qualified paths.

## Decision

The only extension to Rust's grammar is the JSX expression. Specifically:

- React imports are an attribute, `#[react_import(...)]`, not syntax.
- Inside macro token trees and attribute bodies JSX is not recognized.
- `<div />.into()` is not allowed; write `(<div />).into()`.
- `<List<T> />` is not supported; generics are inferred.
- Whitespace follows React/JSX, with a one-to-one golden test for each rule.

## Consequences

- A `.rsx` file with its JSX expressions replaced by Rust expressions is a valid Rust file; everything else can be delegated to rustc and rust-analyzer.
- The `<` ambiguity is resolved by expression position, a three-token decision and one bounded angle scan (reached only for `<Name<`), never by a blanket rule.
- Users who want JSX in a macro must wait.

## Alternatives considered

- **Recognize JSX inside macros.** Requires knowing each macro's grammar; Outou cannot.
- **Allow postfix on elements.** Makes `<a />.b` ambiguous with a comparison chain in enough cases that recovery suffers.
- **Explicit generics in tags.** `<List<T>>` collides with the closing `>` of the tag; deferred until the base grammar is stable.
