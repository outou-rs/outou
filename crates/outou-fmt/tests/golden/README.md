# outou-fmt golden fixtures

Each directory holds `input.rsx` and `expected.rsx` (what `format_source` must produce exactly, and must leave unchanged when formatted again; see `../golden.rs`). `parse-error-refused` has no `expected.rsx`: the file is refused.

## Blank lines between JSX children (`blank-lines-*`)

`outou fmt` follows Prettier on blank lines between JSX children (decision of 2026-11-08, replacing the old rule that removed all of them). Each `blank-lines-*` case with a React equivalent also holds:

- `input.jsx`: the same JSX content as `input.rsx`, wrapped in `function App() { return (...); }`.
- `prettier.jsx`: the output of `prettier --parser babel input.jsx`.

`expected.rsx` agrees with `prettier.jsx` line for line apart from indentation (Prettier indents by 2, `outou fmt` by rustfmt's `tab_spaces`) and the Rust wrapper, except where a case below says otherwise.

### Provenance

Prettier 3.9.10 (`npm i --save-exact prettier@3.9.10`, the `latest` dist-tag on 2026-11-08), default options, `--parser babel`, run on 2026-11-08. The full probe set (0 to 3 blank lines, text and element neighbours, expression children, leading and trailing blank lines, fragments, whitespace-only lines, comments, `{" "}`, nested elements, CRLF input, fit-on-one-line elements) was run beyond the checked-in cases.

### Rules observed in Prettier 3.9.10

1. A blank line between two non-text children (elements, expression containers) is kept; two or more collapse into one. Lines holding only spaces or tabs count as blank.
2. A blank line right after the opening tag or right before the closing tag is dropped.
3. Prettier always breaks an element that has an element child or two expression children (`forcedBreak`), blank line or not; `<div><a/>\n\n<b/></div>` becomes three lines plus the blank one. Outou has a one-line form for such elements, so it is never chosen while a blank line is kept.
4. If the element has any text child, every blank line among its children is dropped. This is Prettier's `isPairOfHardlines && containsText` filter. Text means non-whitespace text, whitespace-only text with no line break (the single space in `<a/> <b/>`), or `{" "}`, which Prettier rewrites to a text space first. The test looks at the element's own children only: a nested element without text keeps its blank lines, and an inner element with text does not affect its parent.
5. Fragments follow the same rules as elements.
6. A comment container `{/* c */}` is an ordinary expression child for these rules.

### Cases

| Directory | Pins |
|---|---|
| `blank-lines-between-children-kept` | 0, 1, 2 and 3 blank lines between `<li>` children: 0 stays 0, the others become 1. |
| `blank-lines-forces-multiline` | An element that fits on one line is laid out on several because a blank line is kept. |
| `blank-lines-leading-trailing-dropped` | Blank lines after the opening tag and before the closing tag go; the one between children stays. |
| `blank-lines-expression-children` | Same rules between `{expr}` children and between an expression and an element. |
| `blank-lines-dropped-when-text-present` | One single-line space child (`<b/> <c/>`) drops all blank lines of the element; the nested `<section>` has no text and keeps its own. |
| `blank-lines-space-island-counts-as-text` | `{" "}` counts as text: all blank lines go. Layout differs from Prettier here for a reason unrelated to blank lines: Prettier turns `{" "}` into a text space (`<a /> <b />`), `outou fmt` leaves the island where it is, on its own line. |
| `blank-lines-in-fragment` | `<>...</>` keeps one blank line between children, drops the leading and trailing ones. |
| `blank-lines-whitespace-only-lines` | Lines containing only spaces or tabs are blank lines. |
| `blank-lines-nested` | Each element decides for itself; leading and trailing blank lines are dropped at every level. |

### Known divergences, pinned without a Prettier pair or with a different result

- `blank-lines-around-comments`: a children region with a comment (`{/* c */}`) is kept byte-for-byte (the AST does not model the comment), so all its blank lines survive as written, including 2 or more and leading or trailing ones. Prettier (see `prettier.jsx`) normalizes them like any other expression child.
- `blank-lines-in-text-kept-verbatim` (no `input.jsx`): text is never reflowed, so a blank line inside or next to a text run is copied with the text. Prettier reflows the text and drops those blank lines.
