# 0015. Fragments: `<>…</>` is real syntax

Status: Accepted

## Context

Fragments were reserved in Phase 0: `<>` was committed as JSX only so that Outou, not rustc, reported ``fragments are not supported`` (`docs/grammar.md` §5, §10, the reserved-syntax list). The body of a fragment was never parsed, so one `<>…</>` gave two or three errors. Without fragments a component that returns two siblings needs a wrapper element, which changes the DOM (table rows, flex and grid children, list items). Dioxus 0.7 accepts several roots in one `rsx!` call, so the backend does not stand in the way. The post-alpha backlog (issue #31) asked for a decision; the project manager decided on 2026-11-08 (option B of the grammar proposals).

## Decision

`<>…</>` is syntax, allowed wherever an element is: a top-level JSX expression, a child, an attribute island, a closure body in `.map`.

- `<>` takes **no attributes**. A `key` on a fragment is not supported, as in React's short form; `<Fragment key>` is not part of this change, and `Fragment` is an ordinary component name that Outou does not define.
- `<></>` is legal and renders nothing. A single-child fragment is legal and the formatter never removes it.
- Whitespace inside a fragment follows grammar §8 like any element's children.
- **AST.** `JsxTag` gains a variant, `JsxTag::Fragment { span }`, used for both `open` and `close` of a `JsxElement` whose `attributes` stay empty. A separate element kind was rejected: a field next to `open` would leave every existing `match` on `JsxTag` compiling, and each of them reads a name that a fragment does not have. With a variant the compiler lists every site to revisit; they were: the parser's closing-tag matching, `outou_syntax::select`, the backend's lowering, key and select scans, the formatter's tag and children printers, and the language server's completion and semantic tokens.
- **Parsing.** `</>` is a closing tag with the empty name, and an open fragment is an open-tag entry with the empty name, so the existing rules of §2.1 apply unchanged and the mismatch messages read ``closing tag `</div>` does not match opening tag `<>` `` and ``closing tag `</>` does not match opening tag `<div>` ``. An unclosed fragment is ``missing closing tag `</>` ``. `recover_fragment`, `recover_top_level_fragment`, the ``fragments are not supported`` message, `tests/ui/reserved-fragment` and the two `TODO(alpha) (#31)` fragment-recovery markers are gone.
- **Lowering.** A fragment adds no node. At the top of a JSX expression its children are the roots of the `rsx!` call (`rsx! { a {}, b {} }`); in child position they are spliced into the parent's child list, at any depth; an empty fragment is `rsx! { }` at the top and nothing as a child. No `dioxus_core::Fragment`, no new facade API. `<>` and `</>` have no generated span of their own (ADR 0007: a source span may map to zero generated spans); the call site (`rsx! {` and its closing brace) still maps to the whole fragment, as for every element, and the children keep their own mappings.
- **Scans that look inside an element** (`<select value>` options, `key` bindings, the select bindings) recurse through a fragment, since it is lexically inside the select.

This amends ADR 0004 and the reserved-syntax list in `docs/grammar.md` §10: the one extension to Rust's syntax is still the JSX expression, and a fragment is a JSX expression. Fragments are no longer reserved.

## Consequences

- Questions the decision left open were answered with the choice closest to React, and are recorded here:
  - `<key={x}>` (how a keyed fragment would be mistyped) gets one message, ``unexpected `=` after the tag name `key`: a fragment `<>` takes no attributes, so it has no `key`; put the `key` on an element``, and its value is dropped, instead of the old ``spread attributes are not supported`` for the `{x}`. Any other tag name keeps the general message. `<Fragment key={x}>` is a component call; if no `Fragment` is in scope rustc reports ``cannot find value `Fragment` `` at the tag, in `.rsx` terms.
  - The backend uses the key of the **first root** of a call as the item's key and warns about a key on any later root (`docs/backend-leakage.md` row 63). Outou reports such a key as a warning in its own words (``this `key` has no effect …``), still evaluates and type-checks it, and does not pass it on, so the backend's deprecation warning never reaches the user. React keys each child; this is a temporary limitation, listed for the runtime decision.
  - `<></>` at the top of a call is an empty `rsx! { }`, which the backend renders as one placeholder node. It shows nothing (row 63).
  - A closing tag cut short by a `<` that begins JSX (`<p></div <q/>`) used to leave the following element to Rust as a less-than operand and produce invalid Rust in recovery output for named elements. It is now taken as part of the element, as an unfinished opening tag already did. This was found when fragments became ordinary elements and is not specific to them.
- Language server: `<>` and `</>` are semantic tokens of type `operator` (a standard LSP type; ADR 0012's overlay names gain one); hover and rename on them are answered locally (`null` and a refusal in Outou words, a fragment has no name); `</` typed directly in a fragment offers `</>`.
- The formatter prints `<>`, the children as for an element, then `</>`. A tag whose trivia holds a comment is kept as written, as for `< /* c */ span>`.
- Every `JsxTag` consumer must now decide what a fragment means for it. The ones that need a name (rename, attribute completion, tag-name hover) treat it as nameless.
- `key` on a keyed list item that has several roots is keyed by the first. Whether every root should be keyed (React's `Fragment key` and per-child keys) belongs to the runtime decision.

## Alternatives considered

- **Keep fragments reserved and finish the recovery.** Cheapest, but a JSX language without multi-root components pushes users to wrapper elements that change the DOM.
- **A named `<Fragment>` component in the prelude, no `<>` syntax.** No grammar change, and it would carry a `key`. It adds a facade API over `dioxus_core::Fragment`, makes users write more, and can clash with a user's own `Fragment`. It can still be added later on top of this decision as the keyed form.
- **A separate element kind or a flag on `JsxElement`.** Rejected under Decision, AST.
- **Lower to `dioxus_core::Fragment`.** Would add backend vocabulary to the generated code and a facade function; several roots in one call say the same thing natively.
