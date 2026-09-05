---
title: "[Week 2] Grammar specification"
milestone: "Phase 0"
week: "Week 2"
gate: "—"
droppable: no
labels: [phase0, must-keep]
---

| | |
|---|---|
| **Gate** | none (input to Gate 1) |
| **Droppable** | No |

Turn `docs/grammar.md` from a draft into the normative grammar. Every `TODO(phase0)` in it is resolved or explicitly deferred.

- [x] lexer modes and transitions, including nested expression islands — §2 now has a mode-transition table covering attribute-value islands, nested child islands, string/char/lifetime literals inside islands, stray `}` in text, and `>` as literal text.
- [x] macro and attribute opacity — §3 defines macro-invocation token trees (`SimplePath ! (…)`/`[…]`/`{…}`), `macro_rules!` definitions (whose body follows the *name*, not the `!`, and so needs its own rule) and attribute bodies (`#[…]`/`#![…]`) precisely; JSX inside any of them is a documented, accepted gap (§10), not a bug.
- [x] the `<` rule: expression position, a three-token decision, one bounded angle scan — §4 resolves the former TODO with a three-token decision table (`t1`/`t2`/`t3`), one bounded angle scan for `<Name<`, and a decision table covering `a < b`, `f::<T>()`, `<T as Trait>::f()`, `<Vec<i32>>::new()`, `<[T]>::len(&x)`, `<&str>::from(s)`, `<(A, B)>::default()`, `<dyn Trait>::f(&x)`, `<div type="x" />`, `<link as="style" />`, `<Foo.Bar />`, `<List<T> />` and the reserved shapes. There is no speculative parse and no backtracking; a committed tag is recovered as an error node (§9).
- [x] whitespace rules with one golden fixture per rule under `tests/fixtures/formatting/` — 12 fixtures total (2 pre-existing, fixed to the `#[component] fn` convention; 10 new): `multiline-text`, `tags-only-lines`, `inline-spaces-kept`, `text-then-expression`, `expression-then-text-newline`, `blank-lines-dropped`, `single-line-whitespace-only`, `element-between-text`, `leading-trailing-lines`, `tab-becomes-space`, `nbsp-not-trimmed`, `crlf-line-endings`. Each is paired with an `input.jsx` React equivalent; see `tests/fixtures/formatting/README.md` for the `expected.txt` format and provenance (hand-derived from Babel's `cleanJSXElementLiteralChild`, cross-checked with a from-memory transcription of that algorithm since `@babel/core`/`@babel/parser` are not available offline — real Babel was not run).
- [x] list of reserved syntax rejected in Phase 0 — §10 records the Phase 0 status of every construct, marking each **Rejected, diagnosed**, **Reserved (read as Rust)**, **Allowed**, or **Not recognized**.

One item was deliberately deferred rather than resolved: the payload/semantics of `#[react_import(...)]` (§1) stay `TODO(phase0)`, explicitly pushed to the React-interop phase (see `docs/design.md` § Future) with the reason recorded inline — Phase 0 only needs the attribute to be opaque and inert, which is already normative.
