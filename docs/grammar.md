# Grammar

This document is normative. It describes how Outou extends Rust syntax and nothing else. Words in **bold capitals** (MUST, MUST NOT, SHOULD) carry their usual meaning.

## 1. Scope of the extension

The only addition to Rust's grammar is the **JSX expression**. Everything else in a `.rsx` file is Rust, and is passed to rustc unchanged.

```text
Expression := RustExpression | JsxElement
```

React-style imports are not syntax. They are expressed with an attribute, `#[react_import(...)]`, whose semantics are outside Phase 0. TODO(phase0): attribute payload.

A `.rsx` file MUST be a valid Rust file once every JSX expression is replaced by a Rust expression.

## 2. Lexer modes

The lexer is mode-aware. It has three modes and switches between them on specific tokens:

```text
Rust ──(JSX start)──▶ JsxTag ──(>)──▶ JsxText ──({)──▶ Rust
  ▲                                       ▲              │
  └──────────────────(})──────────────────┘◀─────────────┘
```

| Mode | Meaning | Leaves on |
|---|---|---|
| `Rust` | ordinary Rust tokens | a JSX start (§4) |
| `JsxTag` | inside `<` … `>`: names, attributes, `=`, `/` | `>` (to `JsxText`) or `/>` (back to the enclosing mode) |
| `JsxText` | between tags: text and child elements | `{` (to `Rust`), `<` (to `JsxTag`), matching closing tag |

A `{` in `JsxText` or as an attribute value opens a **Rust expression island**. Islands are lexed in `Rust` mode with brace depth tracked; the matching `}` returns to the previous mode. Islands nest: an island may contain a JSX expression, which may contain islands.

## 3. Opaque token trees

The lexer MUST NOT enter a JSX mode inside:

- the token tree of a macro invocation: `println!("{}", a < b)`, `some_macro! { <anything> }`
- the body of an attribute: `#[some_attribute(...)]`

These regions are lexed as opaque Rust token trees. JSX inside macro invocations is not supported in Phase 0 and produces no Outou diagnostic; rustc reports whatever the macro reports.

## 4. Where a JSX expression may start, and the `<` ambiguity

`<` is also Rust's less-than operator and the opening of a qualified path. The following MUST be distinguished:

```rust
a < b
<T as Trait>::method()
<Vec<i32>>::new()
```

from

```rsx
<Button />
```

A rule of the form "`<` in expression position starts JSX" is **forbidden**. The parser MUST combine:

1. **Expression position.** Only where Rust permits an expression to begin. After a complete operand (`a <`) it is an operator.
2. **Lookahead.** `<` followed by an identifier-like token or `>` (fragment) is a JSX candidate; `<` followed by `(`, `[`, `&`, a literal, or a lifetime is not.
3. **Qualified path recognition.** A JSX candidate whose tag is followed by `as` or `>::` is a qualified path, not JSX.
4. **Speculative parse.** When 1–3 do not decide, parse as JSX with backtracking; on failure, reparse as Rust. The speculative parse MUST be bounded to the current expression.

TODO(phase0): exact bound for speculative parsing and its interaction with error recovery.

## 5. Elements

```text
JsxElement   := SelfClosing | Open Children Close
SelfClosing  := '<' TagName Attribute* '/>'
Open         := '<' TagName Attribute* '>'
Close        := '</' TagName '>'
TagName      := Identifier ( '.' Identifier )*   // TODO(phase0): dotted names
Attribute    := AttrName ( '=' AttrValue )?
AttrName     := Identifier ( '-' Identifier )*
AttrValue    := StringLiteral | '{' RustExpression '}'
Children     := ( JsxText | '{' RustExpression '}' | JsxElement )*
```

A tag name beginning with an uppercase letter is a **component** reference; any other name is an **intrinsic element**. The closing tag's name MUST equal the opening tag's name; a mismatch is an Outou diagnostic (§9).

Fragments (`<>` … `</>`): TODO(phase0).

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

## 7. Restrictions in Phase 0

### 7.1 No postfix on a JSX expression

```rsx
<div />.into()      // error
(<div />).into()    // ok
```

A JSX expression MUST NOT be followed directly by `.`, `?` or `(`. Parenthesize it. This may be relaxed later.

### 7.2 No explicit generic arguments in tags

```rsx
<List<T> items={items} />     // not supported
<List items={items} />        // ok, inferred
```

Component generics are resolved by Rust type inference only.

### 7.3 No JSX inside macros or attributes

See §3.

## 8. Whitespace in text

Outou follows React/JSX whitespace rules. Text is normalized as follows:

1. Lines of text are split at newlines.
2. Leading and trailing whitespace of each line is removed.
3. Empty lines are removed.
4. Remaining lines are joined with a single space.
5. Whitespace between elements that consists only of newlines and indentation produces no text node.

Examples:

```rsx
<p>
    Hello
    world
</p>
```

yields `Text("Hello world")`.

```rsx
<div>
    <A />
    <B />
</div>
```

yields two element children and no text nodes.

Every rule here has a golden test in `tests/fixtures/formatting/` paired with the same input as React JSX. A difference from React is a bug.

## 9. Error recovery and diagnostics

The parser MUST produce an AST for any input. Broken regions become error nodes; parsing continues after them. At minimum the following MUST recover with the rest of the file intact:

```rsx
<div cl
```

```rsx
<User name={
```

```rsx
<section>
    <Foo>
</sect
```

Structural JSX errors are diagnosed by Outou, in Outou's words, at the `.rsx` position:

```text
closing tag `</span>` does not match opening tag `<div>`
```

A backend's parser MUST never see broken JSX, and its errors MUST never be shown for `.rsx` files.

## 10. Reserved

The following are reserved for future versions and MUST be rejected with a diagnostic in Phase 0 rather than silently accepted: fragments, spread attributes (`{...props}`), namespaced names (`xlink:href`), dotted tag names. TODO(phase0): confirm the list.
