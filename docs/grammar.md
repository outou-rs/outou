# Grammar

This document is normative. It describes how Outou extends Rust syntax and nothing else. Words in **bold capitals** (MUST, MUST NOT, SHOULD) carry their usual meaning.

## 1. Scope of the extension

The only addition to Rust's grammar is the **JSX expression**. Everything else in a `.rsx` file is Rust, and is passed to rustc unchanged.

```text
Expression := RustExpression | JsxElement
```

React-style imports are not syntax. They are expressed with an attribute, `#[react_import(...)]`. In Phase 0 this attribute is parsed as an ordinary Rust attribute: an opaque token tree, exactly like any other `#[...]` (§3). Outou does not read, validate or lower its payload; it is inert.

`TODO(phase0): the payload and semantics of #[react_import(...)] are deferred to the React-interop phase (see docs/design.md, "Future"), which is explicitly out of scope before the front end, tooling and runtime decision are settled. Reason: the payload's shape depends on interop decisions not yet made; fixing it now would constrain that later work for no benefit today, since the attribute does nothing in Phase 0.` This does not change the grammar: the attribute is not part of the JSX extension and never will be recognized specially by the lexer.

A `.rsx` file MUST be a valid Rust file once every JSX expression is replaced by a Rust expression.

## 2. Lexer modes

The lexer is mode-aware. It has three modes and switches between them on specific tokens:

```text
Rust ──(JSX start, §4)──▶ JsxTag ──(>)──▶ JsxText ──({)──▶ Rust
  ▲                                          ▲               │
  └──────────────────(matching })────────────┘◀───────────────┘
```

| Mode | Meaning | Leaves on |
|---|---|---|
| `Rust` | ordinary Rust tokens, including expression islands | a JSX start (§4) |
| `JsxTag` | inside `<` … `>`: names, attributes, `=`, `/` | `>` (to `JsxText`) or `/>` (pops the frame, back to the mode active before the `<`) |
| `JsxText` | between tags: text and child elements | `{` (to `Rust`), `<` (to `JsxTag`, nested), or the matching closing tag (pops the frame) |

Modes nest on a stack: each JSX element pushes a frame when its `<` is recognized and pops it on `/>` or its matching closing tag; each island pushes a frame on `{` and pops it on the matching `}`. The mode after a pop is whatever was active before the push, which may itself be `JsxTag`, `JsxText`, or `Rust` at any brace depth.

### 2.1 Mode transitions in detail

| Context | Token seen | Effect |
|---|---|---|
| `Rust` | `<` that resolves to JSX (§4 rules 1–3) | push a JSX frame, enter `JsxTag` |
| `JsxTag` | `>` | enter `JsxText` for this element's children |
| `JsxTag` | `/>` | pop this frame; resume the mode active before the `<` |
| `JsxTag` | `=` followed by `{` (attribute-value island) | push an island frame, enter `Rust`; remember to resume `JsxTag`, not `JsxText`, on the matching `}` |
| `JsxText` | `{` (child island) | push an island frame, enter `Rust`; remember to resume this `JsxText` frame on the matching `}` |
| `JsxText` | `<` not followed by `/` | push a nested JSX frame, enter `JsxTag`; remember to resume this `JsxText` frame when the nested element's frame pops |
| `JsxText` | `</` `Name` `>` where `Name` equals the top frame's tag name | pop this frame; resume the mode active before its `<` |
| `JsxText` | `</` `Name` `>` where `Name` equals no open frame's name | ``closing tag `</Name>` does not match opening tag `<Top>` `` at the closing tag; **pop the top frame** and continue |
| `JsxText` | `</` `Name` `>` where `Name` equals the name of a frame below the top | ``missing closing tag `</X>` `` at the opening tag of every frame popped above it; then pop through the matching frame |
| `JsxText` | `</` … end of input reached before `>` | ``unexpected end of file inside closing tag, expected `>` `` at the `</`; pop the top frame |
| `JsxText` | `</` … `<` reached before `>` (`<A></B<C>`) | ``unexpected `<` inside closing tag, expected `>` `` **at the `<`** (§9 span convention); pop the top frame and resume lexing at that `<` in the mode active after the pop, where §4 decides it again |
| `JsxTag` / `JsxText` | `}` with no island frame open | terminator; see §2.2 |
| `JsxTag` / `JsxText` | end of input | terminator; see §2.2 |
| `JsxText` | `>` | literal text; only `<` and `{` are special in `JsxText` |
| `Rust` (island, any depth) | `{` | brace depth of the current island frame increases by one; stays in `Rust` |
| `Rust` (island, any depth) | `}` | if the island's brace depth is above zero, it decreases by one and lexing stays in `Rust`; if it is zero, this `}` closes the island and pops its frame (this is what "`}` at island depth 0" means) |
| `Rust` (island, any depth) | a string literal (`"…"`, `r"…"`, `r#"…"#`, …) | lexed as one atomic Rust token; `{`, `}`, `<`, `>` inside it are not counted and do not affect mode or brace depth |
| `Rust` (island, any depth) | a char literal (`'a'`) | lexed atomically, distinct from a lifetime |
| `Rust` (island, any depth) | a lifetime (`'a`) | lexed atomically; the leading `'` never starts a char literal here |
| `Rust` (island, any depth) | a `<` that resolves to JSX (§4) | push a new JSX frame (nested island-in-JSX-in-island is allowed); the enclosing island's brace depth is preserved and resumed when the nested element's frame pops |

Nested islands therefore work by the same stack: `{ if x { <A/> } }` pushes an island at the outer `{`, sees `if x {` as ordinary Rust (brace depth 1), then `<A/>` pushes and immediately pops a JSX frame within that island, then `}` brings the brace depth back to 0, then the final `}` closes the island.

### 2.2 Terminators

Two events end a JSX expression from the outside: **end of input**, and a **`}` seen while no island frame is open** (in `.rsx` that `}` almost always closes the enclosing Rust block, and a literal `}` in text is an error in React JSX too).

On either, the parser closes every open JSX frame implicitly in the AST, innermost first, and reports:

- one diagnostic for the innermost open construct — a tag, an island, or a closing tag — naming the terminator (``unexpected end of file …`` or ``unexpected `}` …``);
- ``missing closing tag `</X>` `` for each open frame that had become an element (that is, whose `>` was seen), at its opening tag.

A `}` terminator is then re-examined in the mode active before the outermost `<`, so it still closes the enclosing Rust block and the rest of the file parses. Enclosing Rust blocks left open at end of input are closed implicitly in the AST as well; that is a Rust-level recovery, not an Outou diagnostic.

This changes one earlier rule deliberately: a stray `}` in `JsxText` is a terminator, not literal text kept for recovery. Keeping it as text would lose every enclosing Rust block after an unclosed element, which breaks the Gate 1 promise in `tests/fixtures/incomplete/README.md` that recovery-mode codegen must emit Rust in which every complete Rust expression is still analyzable. Uniform unwinding costs one case (`<div>a } b</div>`, already an error in Babel) and buys block-structure survival plus a single rule for both modes.

A closing tag that matches no open frame is popped from the top rather than left in place: leaving it would cascade `mismatched-closing-tag.rsx` (`<div>` / `</span>` / `}`) into three diagnostics, but `mismatched-closing-tag.expected` records exactly one.

## 3. Opaque token trees

The lexer MUST NOT enter a JSX mode inside either region, and inside them MUST track delimiter balance only:

- a **macro invocation**: the whole `DelimTokenTree` — `( … )`, `[ … ]` or `{ … }` — that follows `SimplePath !`. The path is Rust's `SimplePath`, so `println!`, `path::to::mac!`, `::krate::mac!` and `r#mac!` all qualify; the `!` of `!=` and a unary `!` do not. The delimiter MUST follow the `!` immediately (trivia aside).
- a **`macro_rules` definition**: `macro_rules ! IDENTIFIER MacroRulesDef`. Here the delimiter follows the *name*, not the `!`, so the macro-invocation rule above does not reach it and this separate rule is required. All three body forms are opaque: `macro_rules! m { … }`, `macro_rules! m ( … );` and `macro_rules! m [ … ];`.
- an **attribute**: everything between `#[` or `#![` and its matching `]`, including every nested delimiter. This covers both of Rust's attribute-input forms — a delimited token tree (`#[a(…)]`, `#[a[…]]`, `#[a{…}]`) and `= Expression` (`#[doc = "…"]`, `#[a = <T>::C]`) — and `#[react_import(...)]` (§1).

Opacity is delimiter-recursive: the region ends at the delimiter matching the one that opened it, however deep. String, raw-string and char literals inside it are still lexed atomically, so delimiters inside them are not counted.

JSX written inside either region is not recognized in Phase 0 and produces no Outou diagnostic; rustc (or the macro) reports whatever it reports. This is a known, accepted gap — see §10.

## 4. Where a JSX expression may start, and the `<` ambiguity

`<` is also Rust's less-than operator and the opening of a qualified path. These MUST be distinguished:

```rust
a < b
<T as Trait>::method()
<Vec<i32>>::new()
<[T]>::len(&x)
```

from `<Button />`.

Two facts make an exact, backtrack-free rule possible:

- Rust's grammar is `QualifiedPathInExpression → QualifiedPathType (:: PathExprSegment)+` with `QualifiedPathType → < Type (as TypePath)? >`. The `::` is **required**. A bare `<T>` is neither an expression nor a complete type, so Outou may commit to JSX the moment it knows no `::` can follow the matching `>`.
- **Tag-name case is useless for this decision.** `<Vec<i32>>::new()` (Rust) and `<List<T> />` (JSX) both start with an uppercase letter; `<div>` (JSX) and `<dyn Trait>::f(x)` (Rust) both start lowercase. Case decides component vs intrinsic (§5) and nothing else.

A rule of the form "`<` in expression position starts JSX" remains **forbidden**. The parser MUST apply the three rules below in order.

**Rule 1 — expression position.** JSX can only start where Rust permits an expression to begin. After a complete operand (`a <`, `x <`) the token is the less-than operator and no further rule runs. A `<` in *type* position — after `:`, after `::` (turbofish), inside a generic argument list, in a `where` clause, in a function signature — is likewise never JSX. Expression position is the parser's knowledge, not the lexer's; the lexer is parser-driven.

**Rule 2 — three-token decision.** Let `t1` be the token after `<`, `t2` the token after `t1`, `t3` the token after `t2`. Trivia (whitespace, comments) is skipped when identifying a token.

| `t1` | Decision |
|---|---|
| `(`, `[`, `&`, `&&`, `*`, `!`, `_`, `::`, `<` (including the first `<` of `<<`), a lifetime, or a literal | **Rust.** These either begin a Rust `Type` (`(`, `[`, `&`, `&&`, `*`, `!`, `_`, `::`, `<`) or cannot begin a `JsxName` (a lifetime, a literal). Either way the `<` is not JSX. |
| one of `dyn` `impl` `fn` `unsafe` `extern` `for` | **Rust**, unconditionally. Each begins a Rust `Type` in a qualified path: `<dyn Trait>::f()`, `<impl Fn()>::x`, `<fn() -> T>::f`, `<unsafe fn()>::x`, `<extern "C" fn()>::x`, `<for<'a> fn(&'a T)>::x`. `<dyn Trait>::f()` and `<dyn class="x" />` are identical through `t2` and diverge only at a fourth token, so these six are **reserved tag names** in Phase 0 rather than special-cased (§5, §10). |
| `>` | **JSX, committed** — a fragment. Diagnostic: ``fragments are not supported in Phase 0`` (§5, §10). `<>` is not a Rust type. |
| `/` | **JSX, committed** — a closing tag with no element open. Diagnostic: ``closing tag `</X>` has no matching opening tag`` (§9). No `Type` starts with `/`. |
| any other IDENTIFIER_OR_KEYWORD, including a RAW_IDENTIFIER | consult `t2` below |
| anything else | **Rust.** Not JSX; let rustc report it. |

| `t2` (after an identifier-or-keyword tag name) | Decision |
|---|---|
| `as` | If `t3` is `=`, **JSX, committed** — an attribute named `as` (`<link as="style" />`). Otherwise **Rust** (`<T as Trait>::f()`). A *bare* attribute named `as` is reserved; see §10. |
| `::` | **Rust** — `<A::B>::c()`, `<crate::A>::b()`. |
| `<` (including the first `<` of `<<`) | Rule 3. |
| `>` | If `t3` is `::`, **Rust** — `<Self>::new()`, `<A>::B`. Otherwise **JSX, committed**: the lexer enters `JsxText` at the character after `>`. Text beginning with `::` immediately after an opening tag is reserved; see §10. |
| `!` | If `t3` is `(`, `[` or `{`, **Rust** — a type macro (`<ty!()>::default()`); `SimplePath ! DelimTokenTree` is a `TypeNoBounds`. Otherwise **JSX, committed**: nothing else can continue a Rust type here, and the `!` is recovered as an error inside the tag (§9). |
| `/`, `=`, `.`, `:`, `{`, `}`, `)`, `]`, `;`, a string literal, an IDENTIFIER_OR_KEYWORD, end of file, or any other token | **JSX, committed.** None of these can continue a Rust expression that began with `<`. `.`, `:` and `{` additionally produce the reserved-syntax diagnostics of §10. |

**Rule 3 — bounded angle scan** (reached only for `<Name<`, the one genuinely ambiguous shape). Scan forward from the **candidate `<` itself** — the one being decided, not the inner one — starting at angle depth 0. Each `<` increments the depth and each `>` decrements it, so the candidate `<` brings the depth to 1 and the `<` at `t2` brings it to 2.

Compound tokens are split into their `<`/`>` parts for this count, exactly as rustc's parser splits them: `<<` counts as two opens; `>>` as two closes; `>=` as one close plus a residual `=`; `>>=` as two closes plus a residual `=`. (`>>>` is not a Rust token: it lexes as `>>` then `>`.) A residual `=` is an ordinary token of the scan and may be the "next token" examined below. `( … )`, `[ … ]` and `{ … }` opened during the scan are skipped as balanced units, so angles inside them are not counted; string, raw-string and char literals are atomic for the same reason.

The scan stops at the first of:

- **the depth returning to 0.** If the token immediately after that `>` is `::` → **Rust** (`<Vec<i32>>::new()`, `<A<B> as T>::f()`). Otherwise → **JSX, committed**, with ``generic arguments on a tag are not supported in Phase 0`` at the inner `<`; the balanced `<…>` after the tag name is skipped and attribute parsing continues at the token after it (§7.2, §10).
- `;`, a closing delimiter of a group opened *before* the candidate `<`, or end of input → **JSX, committed**, with the same diagnostic. No `::` can follow, so the construct cannot be a qualified path.

| Input | Scan | Outcome |
|---|---|---|
| `<Vec<i32>>::new()` | `<`→1, `Vec`, `<`→2, `i32`, `>>`→1→0; next token `::` | Rust |
| `<A<B<C>>>::x()` | `<`→1, `A`, `<`→2, `B`, `<`→3, `C`, `>>`→2→1, `>`→0; next token `::` | Rust |
| `<A<<B as C>::D>>::x()` | `<`→1, `A`, `<<`→2→3, `B as C`, `>`→2, `::`, `D`, `>>`→1→0; next token `::` | Rust |
| `<List<T> items={items} />` | `<`→1, `List`, `<`→2, `T`, `>`→1 (depth > 0, scan continues), `items`, `=`, `{items}` skipped as a balanced unit, `/`, `>`→0; next token `;` ≠ `::` | JSX, generic-arguments diagnostic |

The scan builds no nodes and is bounded by the enclosing statement, so nothing is ever undone. As with §10's `<A>::B</A>` row, an element whose children begin with `::` is read as Rust (`<A<B>>::x()`); write `{"::…"}`.

**Commitment is final.** Once rule 2 or rule 3 says JSX, the construct is JSX. Anything broken in the rest of it is recovered as an error node (§9), never reparsed as Rust — incomplete input stays JSX, which is what keeps IDE features alive mid-edit. There is no speculative parse and no backtracking anywhere: every `<` is decided by at most three tokens, plus one bounded scan in exactly one case.

### 4.1 Decision table

| Input | Rule | Outcome |
|---|---|---|
| `a < b` | 1 | Rust: `<` follows the complete operand `a`. |
| `a < B::C` | 1 | Rust: comparison against a path. |
| `a <b>c` | 1 | Rust tokens (`a`, `<`, `b`, `>`, `c`); rustc accepts or rejects. |
| `if x < y {` | 1 | Rust. |
| `\|x\| x < y` | 1 | Rust. |
| `x < y && y > z` | 1 | Rust. |
| `f::<T>()` | 1 | Rust: `<` follows `::`, a type position. |
| `let v: Vec<A> = …` | 1 | Rust: type position, never JSX. |
| `<T as Trait>::f()` | 2 (`t2`=`as`, `t3`=`Trait`) | Rust. |
| `<A::B>::c()` | 2 (`t2`=`::`) | Rust. |
| `<[T]>::len(&x)` | 2 (`t1`=`[`) | Rust. |
| `<&str>::from(s)` | 2 (`t1`=`&`) | Rust. |
| `<(A, B)>::default()` | 2 (`t1`=`(`) | Rust. |
| `<*const T>::f()` | 2 (`t1`=`*`) | Rust. |
| `<dyn Trait>::f(&x)` | 2 (`t1`=`dyn`) | Rust. |
| `<<A as B>::C>::d()` | 2 (`t1`=`<`) | Rust. |
| `<Vec<i32>>::new()` | 3 (`::` after the matching `>`) | Rust. |
| `<Self>::new()` | 2 (`t2`=`>`, `t3`=`::`) | Rust. |
| `<A>::B</A>` | 2 (`t2`=`>`, `t3`=`::`) | Rust. Reserved (§10): write `<A>{"::B"}</A>`. |
| `<div>` | 2 (`t2`=`>`, `t3`≠`::`) | JSX: intrinsic `div`; children then `</div>`. |
| `<div cl` | 2 (`t2`= name) | JSX: committed at `cl`; end of input recovered as an error node (§9). |
| `<div type="x" />` | 2 (`t2`= keyword `type`, a JsxName, §5) | JSX: attribute `type`. |
| `<link as="style" />` | 2 (`t2`=`as`, `t3`=`=`) | JSX: attribute `as`. |
| `let v = <Button />;` | 2 (`t2`=`/`) | JSX: self-closing; `;` is ordinary Rust. |
| `return <A />` | 1 + 2 | JSX: `return` opens an expression position. |
| `match m { _ => <A/> }` | 1 + 2 | JSX: the arm body is an expression position. |
| `<>` | 2 (`t1`=`>`) | JSX committed; fragment diagnostic (§10). |
| `</div>` with nothing open | 2 (`t1`=`/`) | JSX committed; stray-closing-tag diagnostic (§9). |
| `<Foo.Bar />` | 2 (`t2`=`.`) | JSX committed; dotted-tag-name diagnostic (§10). |
| `<svg:rect />` | 2 (`t2`=`:`) | JSX committed; namespaced-name diagnostic (§10). |
| `<A {...props} />` | 2 (`t2`=`{`) | JSX committed; spread-attribute diagnostic (§10). |
| `<List<T> items={items} />` | 3 (no `::` after the matching `>`) | JSX committed; generic-arguments diagnostic (§10). |
| `<User` then `}` | 2 (`t2`=`}`) | JSX committed; the `}` terminates the expression (§2.1, §2.2, §9). |
| `<fn />`, `<for />`, `<dyn />` | 2 (`t1`= reserved keyword) | Rust. Reserved tag names (§5, §10); rustc reports. |
| `<for<'a> fn(&'a T)>::x` | 2 (`t1`=`for`) | Rust. |
| `<ty!()>::default()` | 2 (`t2`=`!`, `t3`=`(`) | Rust: a type macro (`SimplePath ! DelimTokenTree` is a `TypeNoBounds`). |
| `<div!>` | 2 (`t2`=`!`, `t3` not a delimiter) | JSX committed; the `!` is recovered as an error node (§9). |

## 5. Elements

```text
JsxElement   := SelfClosing | Open Children Close
SelfClosing  := '<' TagName Attribute* '/>'
Open         := '<' TagName Attribute* '>'
Close        := '</' TagName '>'
TagName      := JsxName
Attribute    := AttrName ( '=' AttrValue )?
AttrName     := JsxName
JsxName      := IDENTIFIER_OR_KEYWORD ( '-' IDENTIFIER_OR_KEYWORD )*
AttrValue    := StringLiteral | '{' RustExpression '}'
Children     := ( JsxText | '{' RustExpression '}' | JsxElement )*
```

A `JsxName` may be any Rust identifier **or keyword** (`type`, `for`, `loop`, `as`, `in`, `ref`, `move`, `use`, `const`, …) and may contain `-`; React allows `class`, `for` and `type`, and HTML custom elements are hyphenated (`<my-element />`). Inside `JsxTag` the lexer produces `JsxName` tokens, not Rust keyword tokens; lowering to Rust identifiers is codegen's problem (issue #6).

Six keywords are **not available as tag names** in Phase 0: `dyn`, `impl`, `fn`, `unsafe`, `extern` and `for`. Each can begin a Rust type inside a qualified path (`<dyn Trait>::f()`, `<impl Fn()>::x`, `<fn() -> T>::f`, `<unsafe fn()>::x`, `<extern "C" fn()>::x`, `<for<'a> fn(&'a T)>::x`), and separating `<dyn Trait>::f()` from `<dyn class="x" />` would need a fourth token, so §4 rule 2 sends every `<` followed by one of them to Rust. None of the six is an HTML element name — `for` is only an attribute name — so nothing real is lost. A hyphenated name beginning with one of them (`<for-each />`) is reserved for the same reason; an exception for `t1` keyword + `t2` = `-` would be sound but is deliberately deferred so the `t1` row stays decidable on `t1` alone. The restriction is on **tag names only**: attribute names keep the full keyword set, so `<label for="id" />` and `<script type="module" />` are unaffected, as are uppercase components `<Fn />` and `<For />`.

A tag name beginning with an uppercase letter is a **component** reference and is lowered to a Rust path, so it MUST be a plain Rust `IDENTIFIER`; any other name is an **intrinsic element**. The closing tag's name MUST equal the opening tag's name; a mismatch is an Outou diagnostic (§9). `Self` is the only keyword with an uppercase initial: `<Self />` is an Outou diagnostic, ``` `Self` is not a valid component name ``` (§10).

For example, `<div type="x" />` lexes as: `<` (§4 rule 2: `t1`=`div`, `t2`=`type`, an identifier-or-keyword → JSX, committed), `JsxName(div)`, `JsxName(type)`, `=`, `StringLiteral("x")`, `/>` (pop).

**Dotted tag names** (`<Foo.Bar />`) and **namespaced names** (`a:b`, e.g. `xlink:href`) are reserved in Phase 0: `JsxName` does not admit `.` or `:`, so `<Foo` and `<svg` commit as JSX (§4 rule 2) and the lexer, expecting a `JsxName` continuation, reports an Outou diagnostic on the `.` or `:` rather than silently truncating or falling through to rustc. See §10.

**Fragments** (`<>` … `</>`) are reserved in Phase 0. §4 rule 2 (`t1`=`>`) still commits `<>` as JSX — a bare `<` followed by `>` — specifically so that the resulting diagnostic, ``fragments are not supported in Phase 0``, is Outou's, at the `.rsx` position, and not a confusing rustc parse error on `<>`.

### 5.1 Attributes

- A **bare attribute** (`<input disabled />`) has no `AttrValue`; per the grammar above this is legal. It is boolean: wherever generated code observes it, it behaves exactly as `disabled={true}` would. This is a lowering rule, not additional grammar.
- `StringLiteral` uses Rust's own string literal grammar: plain (`"…"`) and raw (`r"…"`, `r#"…"#`, …) strings are both allowed as attribute values. Outou performs no escape processing of its own beyond what rustc's lexer already does for that token.
- **Single-quoted values are rejected.** `<div title='x' />` is not accepted: `'x'` is a Rust char literal (and `'x` a lifetime), not a string. The `'` is seen in `JsxTag`, where a value is expected, so Outou reports ``attribute values must be double-quoted strings or `{…}` expressions`` at the `'` rather than letting a char literal or lifetime reach rustc (§9, §10).
- A **duplicate attribute** on the same tag (`<input value="a" value="b" />`) is an Outou diagnostic: ``duplicate attribute `value` on this tag``, pointing at the second occurrence. Parsing continues; the first value wins for recovery-mode codegen.
- **Event handlers are not special syntax.** `onclick={handle_click}` is an ordinary attribute whose value happens to be an island; Outou does not recognize names beginning with `on`, and event semantics are a backend/runtime concern outside the grammar.

## 6. Rust expression islands

Anything inside `{ … }` in JSX is an arbitrary Rust expression:

```rsx
<div>{items.len()}</div>

<div>
    {
        if loading {
            <Spinner />
        } else {
            <Content />
        }
    }
</div>
```

Islands are lowered as expressions, never reduced to a backend format string. Text and islands are separate child nodes: `<h1>Hello {name}</h1>` is `Text("Hello ")` followed by `Expression(name)`.

An island containing only whitespace and/or a Rust comment (`{/* a note */}`, `{}`, `{ }`) is an **empty island**. It produces no child and no diagnostic, matching React's handling of `{/* comment */}`. Conditional rendering (`{if cond { <A/> } else { <B/> }}`) is not new syntax; it is an ordinary island whose expression happens to be an `if`.

## 7. Restrictions in Phase 0

### 7.1 No postfix on a JSX expression

```rsx
<div />.into()      // error
<div />[0]          // error
(<div />).into()    // ok
```

A JSX expression MUST NOT be followed directly by `.`, `?`, `(` or `[`. Parenthesize it. This may be relaxed later.

### 7.2 No explicit generic arguments in tags

```rsx
<List<T> items={items} />     // not supported
<List items={items} />        // ok, inferred
```

Component generics are resolved by Rust type inference only. This is why §4 rule 3 can decide `<Name<` by a single bounded scan: `<Vec<i32>>::new()` has `::` after the matching `>`, `<List<T> />` does not.

### 7.3 No JSX inside macros or attributes

See §3.

## 8. Whitespace in text

Outou follows React/JSX whitespace rules (the same algorithm as Babel's JSX text cleaning). Each maximal run of text between two non-text boundaries (a tag, an island, or the edge of an element) is normalized independently, as follows:

1. Split the run on line breaks (`\n`, `\r\n`, `\r`) into lines, keeping empty ones; the line breaks themselves are discarded.
2. In every line, replace each tab (U+0009) with one space (U+0020). This is a substitution over the whole line, not a trim: `<p>a\tb</p>` yields `Text("a b")`.
3. For each line: unless it is the **first** line of the run, remove leading U+0020 characters; unless it is the **last** line of the run, remove trailing U+0020 characters. **Only U+0020 is removed.** No other character is trimmed — in particular U+00A0 (no-break space), U+000B, U+000C and every other Unicode space survive into the text node.
4. Drop every line that is empty after step 3.
5. Join the remaining lines with a single U+0020. If no line remains, the run produces **no text node at all** — not even an empty one.

This has consequences that are easy to get backwards, so they are called out explicitly:

- `<p> a </p>` keeps **both** spaces: the text is one line, so neither edge is trimmed. Result: `Text(" a ")`.
- `<b>Hello</b> world` keeps the **leading** space of `" world"`: that text run, too, is one line. Result (as a sibling of `Element(b)`): `Text(" world")`.
- A single-line run that is whitespace-only (`<p> </p>`) is likewise never trimmed, so it survives step 4 as a non-empty line and yields `Text(" ")`. The same text spread across lines (`<p>\n</p>`) has its only line trimmed to empty and yields nothing.
- `<p>\n <NBSP>\n</p>` yields `Text("\u{a0}")`: the ASCII spaces around the no-break space are trimmed, the no-break space is not. Babel behaves the same way; anything that trims Unicode whitespace is a bug.

Text adjacent to an island is its own, independently normalized run: `Hello {name}` is `Text("Hello ")` followed by `Expr(name)`; `{name}\n    world` is `Expr(name)` followed by `Text("world")` (the run after the island starts with a line break, so its first line's indentation is not the special case above and is trimmed like any interior line). Whitespace between an element and an island that sits on one line is kept the same way: `<b>x</b> {name}` is `Element(b)`, `Text(" ")`, `Expr(name)`.

Whitespace made only of newlines and indentation between two elements or between an element and a tag boundary produces no text node at all (every line is empty after trimming). Every rule in this section has a golden fixture under `tests/fixtures/formatting/`, paired with the same input written as React JSX; a difference from React is a bug in Outou. See that directory's README for the fixture list and the exact `expected.txt` format.

## 9. Error recovery and diagnostics

The parser MUST produce an AST for any input. Broken regions become error nodes; parsing continues after them. Diagnostics are in the style `error: <message>` followed by `--> file:line:col`, and are reported in the order the parser discovers them.

**Span convention.** A diagnostic about an unexpected token points at that token. A diagnostic about something missing at end of input points at the **opening** token of the unterminated construct (as rustc does for unclosed delimiters).

At minimum the following MUST recover with the rest of the file intact, each with the exact Outou diagnostic shown (never backend vocabulary):

| Case | Fixture | Diagnostic |
|---|---|---|
| Truncated tag / attribute name at end of input | `incomplete/unterminated-attribute-name.rsx` | ``unexpected end of file inside tag `<div>`, expected an attribute or `>` `` |
| Tag terminated by an enclosing block's `}` | `incomplete/unclosed-nested-tag.rsx` | ``unexpected `}` inside tag `<User>`, expected an attribute or `>` `` |
| Unclosed element | `incomplete/unclosed-nested-tag.rsx` | ``missing closing tag `</section>` `` |
| Mismatched closing tag | `diagnostics/mismatched-closing-tag.rsx` | ``closing tag `</span>` does not match opening tag `<div>` `` |
| Stray closing tag | — | ``closing tag `</div>` has no matching opening tag`` |
| Truncated closing tag at end of input | — | ``unexpected end of file inside closing tag, expected `>` `` |
| Closing tag interrupted by `<` | — | ``unexpected `<` inside closing tag, expected `>` `` |
| Unterminated attribute value (string) | — | ``unterminated string in attribute value`` |
| Unterminated attribute value (island) | `incomplete/unterminated-attribute-value.rsx` | ``unexpected end of file, expected `}` to close the value of attribute `name` `` |
| Empty attribute-value island | — | ``expected an expression for the value of attribute `name` `` |
| Unclosed island at end of file | — | ``unexpected end of file, expected `}` to close this expression`` |
| Stray `}` in element content | `diagnostics/stray-rbrace-in-text.rsx` | ``unexpected `}` here; write `{"}"}` to include a literal `}` in text`` |
| Reserved syntax (§10) | — | ``fragments are not supported in Phase 0`` · ``dotted tag names are not supported in Phase 0`` · ``namespaced names are not supported in Phase 0`` · ``spread attributes are not supported in Phase 0`` · ``generic arguments on a tag are not supported in Phase 0`` · ``a JSX expression cannot be followed by `.`, `?`, `(` or `[`; parenthesize it`` · ``duplicate attribute `value` on this tag`` · ``` `Self` is not a valid component name ``` · ``attribute values must be double-quoted strings or `{…}` expressions`` |
| JSX element nested more than 128 levels deep | — | ``this element is nested too deeply (Outou supports at most 128 levels)`` |
| Inline `mod` nested more than 128 levels deep | — | ``modules are nested too deeply (Outou supports at most 128 levels)`` |

Each fixture under `tests/fixtures/incomplete/` and `tests/fixtures/diagnostics/` has a sibling `.expected` with these strings verbatim; the fixture, not this table, is the ground truth for issue #4.

A backend's parser MUST never see broken JSX, and its errors MUST never be shown for `.rsx` files.

**Nesting limit.** The parser recurses once per nested JSX element and once per nested inline `mod`; unbounded input would overflow the call stack before an AST could ever be produced, violating this section's first sentence. Both kinds of nesting are therefore capped at **128 levels** (decision D4, issue #4): the level that would exceed the cap is diagnosed with one of the two messages above instead of being parsed further, and the rest of the file is recovered on a best-effort basis rather than by fully reconstructing the over-deep structure. 128 was chosen with roughly 3x margin over the deepest nesting observed to overflow a 2 MiB thread stack in a debug build (500 JSX levels, 1000 inline-module levels), while comfortably exceeding any real UI's nesting.

**The round-trip contract (source-driven splicing).** The AST does not carry a structured Rust grammar: Rust content is kept as verbatim source slices (`Expr::Rust`, item-level Rust), never re-parsed. Reconstructing the original source is nonetheless always possible, by walking the tree and splicing each node's text in order, because: (1) every JSX element's span is the exact byte range of that element, from its opening `<` to its self-close, matching close, or recovery terminator, and is never widened over adjacent trivia; (2) an island's parts, a block's statements and tail, and a run of item-level Rust each exactly partition their construct's content range — no gaps, no overlaps; (3) codegen emits `source[span]` verbatim for each `Expr::Rust` and generated Rust for each `Expr::Jsx`. Leaf nodes inside a JSX element (tag, attribute and text spans) are informational only, for the source map and the LSP, and are not required to partition the element. This is what lets a JSX element be found anywhere Rust permits an expression — a `const` initializer, an `impl` method body, any island — without a structural Rust parser: everything that is not JSX stays an opaque slice, and the contract above guarantees the pieces recombine losslessly.

## 10. Reserved

This section fixes the Phase 0 status of the constructs listed below — the ones a JSX author is most likely to reach for. Rows marked **Rejected, diagnosed** MUST produce an Outou diagnostic at the `.rsx` position rather than being silently accepted or forwarded to rustc. The remaining rows record deliberate non-rejections, listed here so that the absence of a diagnostic is a recorded decision rather than an oversight.

| Construct | Status in Phase 0 | Note |
|---|---|---|
| Fragments (`<>` … `</>`) | Rejected, diagnosed | §4 rule 2 (`t1`=`>`) commits it as JSX so Outou, not rustc, reports it (§5). |
| Spread attributes (`{...props}`) | Rejected, diagnosed | Not part of `Attribute` (§5); reported inside `JsxTag`. |
| Namespaced names (`xlink:href`, `a:b`) | Rejected, diagnosed | `JsxName` excludes `:` (§5). |
| Dotted tag names (`<Foo.Bar />`) | Rejected, diagnosed | `JsxName` excludes `.` (§5). |
| Generic arguments in tags (`<List<T> />`) | Rejected, diagnosed | §4 rule 3: no `::` follows the matching `>`, so it commits as JSX and is diagnosed. `<Vec<i32>>::new()` stays Rust. |
| Postfix on a JSX expression (`<div />.x`) | Rejected, diagnosed | §7.1. |
| Single-quoted attribute values (`title='x'`) | Rejected, diagnosed | Not a Rust string literal: `'x'` lexes as a char literal, `'x` as a lifetime. Detected in `JsxTag` (§5.1). |
| Tag names `dyn`, `impl`, `fn`, `unsafe`, `extern`, `for`, and hyphenated names beginning with one of them (`<for-each />`) | Reserved; read as Rust, no Outou diagnostic | §4 rule 2's `t1` keyword row: each begins a Rust type in a qualified path (§5). Attribute names are unaffected; uppercase components are unaffected. |
| Text starting with `::` after an opening tag (`<A>::B</A>`) | Reserved; read as Rust, no Outou diagnostic | §4 rule 2: `>` followed by `::` is a qualified path. Write `<A>{"::B"}</A>`. |
| A bare attribute named `as` (`<link as />`) | Reserved; read as Rust, no Outou diagnostic | §4 rule 2 needs `as` `=`. Write `as="…"`. |
| `Self` as a component name (`<Self />`) | Rejected, diagnosed | Component names are Rust identifiers (§5). |
| JSX inside a macro or attribute token tree | Not recognized; **no Outou diagnostic is possible** | §3: the region is opaque to Outou's lexer. rustc or the macro reports whatever it reports; this is an accepted gap, not a bug to fix in Phase 0. |
| HTML entities (`&amp;`, `&nbsp;`, …) | Treated as literal text | Not decoded in Phase 0; the characters pass through `JsxText` verbatim. Decoding may be added in a later phase. |
| Comments inside JSX (`{/* … */}`) and empty islands (`{}`) | Allowed | Produce no child and no diagnostic, matching React (§6). |
| Conditional / `{if}`-style syntax | Not syntax at all | Rust islands already cover it (§6); nothing to reserve. |

Every construct above with "Rejected, diagnosed" MUST have a corresponding parser test once the parser exists (issue #4); this document only fixes the required behavior.
