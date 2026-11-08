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
- `blank-lines-in-text-collapsed` (no `input.jsx`): text is re-indented but not reflowed (see below), so a blank line between two text lines stays, as one blank line. Blank lines next to the tag or child before the text, or after it, are dropped. Prettier reflows the text and drops every blank line.

## Multi-line text runs (`text-reindent-*`)

Decision of 2026-11-08 (the manager): a multi-line text run is re-indented to follow the surrounding indentation and is **not reflowed**. Words are never joined, split or moved between lines, no `{" "}` is inserted. Prettier does reflow text (it fills lines and drops blank lines), so these goldens diverge from `prettier.jsx` on purpose; the `prettier.jsx` files are kept for comparison only.

### The rule

Split the run on `\n`, `\r\n` and lone `\r` (grammar §8 step 1). A run with one line is copied as written. Otherwise only characters that §8 discards are changed, and only U+0020 and tab are ever removed:

- First line: if it starts with a space or tab, that whitespace is significant (it continues the line of the preceding tag or island) and the line stays put. If it starts with anything else and the run is the element's first child (it follows the opening tag), it moves to a line of its own at the children's indentation (the old first line is then whitespace-only and §8 drops it). After a sibling child it stays glued to that child, as Prettier keeps `<b>x</b>after` and `All ({x})` together. Trailing whitespace is removed either way.
- Middle lines: leading and trailing whitespace removed, then the line starts at the children's indentation.
- Last line: leading whitespace removed. If it ends with a space or tab, that is significant: the line stays at the children's indentation and the next child or closing tag follows directly. If a sibling child follows, the child is glued to the text the same way whatever the line ends with (`more<i>y</i>`). Only when the run is the element's last child and its last line has no trailing whitespace does the closing tag go on its own line, at the element's indentation. An empty last line is a line start for what follows: the children's indentation for a next child, the element's for the closing tag.
- Blank lines inside the text: two or more in a row become one. Blank lines are dropped at the start when the first line is empty (they follow the opening tag or the previous child) and at the end when the next child or closing tag sits on its own line, as between element children. §8 step 4 discards all of them, so the value does not change. A blank line is never written with spaces on it.
- Lines that hold a no-break space, U+000B, U+000C or any other non-ASCII whitespace keep it, as it is text. Tabs inside a line are kept; tabs at the edges are trimmed under the same rules as spaces.
- Relative indentation between the lines of a run is lost (all lines align). It is insignificant in JSX.
- A children region kept verbatim (a comment in it) and an expression island holding multi-line text (a line break is `\n` or a lone `\r`, as in §8) are untouched, as before.
- Output line endings follow the usual newline style; a lone `\r` inside text is written as a line break of that style.

The parsed text value (and so the generated Rust string literal) is identical before and after. `tests/sweep.rs` checks that for every golden and `outou build` output was diffed for each case.

| Directory | Pins |
|---|---|
| `text-reindent-nested` | Text re-indented deeper than written, and the closing tag with it. |
| `text-reindent-outdented` | Text written far to the right comes back to the element's indentation. |
| `text-reindent-before-element` | Text followed by an element on the same line; the text's last line keeps its trailing space before `<b>`. |
| `text-reindent-after-element` | Text after an element on the same line keeps its leading space. |
| `text-reindent-tabs` | Tabs at the edges trimmed, tabs kept inside and at the start of the first line. |
| `text-reindent-nbsp` | No-break space and form feed are never touched, even alone on a line or just before the closing tag. |
| `text-reindent-crlf` | CRLF input gives CRLF output with the same layout. |
| `text-reindent-blank-lines` | Two or more blank lines inside text become one; blank lines at both edges go. |
| `text-reindent-glued-edges` | The opening tag's text moves to its own line and the closing tag goes to its own line when there is no significant edge whitespace; text glued to a sibling child (`<b>x</b>after b`, `more<i>y</i>`) stays glued; a leading or trailing space keeps the text glued to the tag too. |
| `text-reindent-fragment` | Same for `<>...</>`. |
| `text-reindent-edges` | First line's leading and last line's trailing whitespace kept; trailing whitespace of the first line removed. |
| `text-reindent-comment-island-verbatim` | A children region with a comment stays byte-for-byte, text included. |

`text-reindent-nested`, `-outdented`, `-before-element`, `-after-element`, `-blank-lines`, `-fragment` and `-edges` have a React pair (`input.jsx`, `prettier.jsx`, kept for comparison since Prettier reflows). The goldens for tabs, NBSP, CRLF, comment islands and glued edges have none: they pin Outou-specific behaviour (what it does with whitespace that Prettier would reflow away or that React has no counterpart for).
