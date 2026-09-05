# formatting fixtures

JSX whitespace golden tests. Outou follows React's whitespace rules (see `docs/grammar.md` §8); a case where the two disagree is a bug in Outou.

Each case is a directory with:

- `input.rsx` — a minimal `#[component] fn` in Outou JSX, with `use outou::prelude::*;` at the top.
- `input.jsx` — the exact React/JSX equivalent (same JSX content, wrapped in an ordinary `function App() { return (...); }`).
- `expected.txt` — the children the parser must produce for the function's returned JSX element, in the format below.

## `expected.txt` format

One node per line, depth-first, top to bottom:

- `Text("…")` — a normalized text child (`ast::JsxChild::Text`). The quoted content is the exact normalized string, including any leading/trailing spaces it keeps.
- `Expr(…)` — a `{ … }` Rust expression island (`ast::JsxChild::Expression`). The content is the island's source text, not re-parsed.
- `Element(name)` — a nested JSX element (`ast::JsxChild::Element`).

Lines are relative to the function's own returned JSX element, which is not itself printed (the fixture file already names it). If an `Element(name)` line has children of its own, they follow immediately below it, each indented two spaces per level of nesting relative to its parent's line. An element with no listed children (self-closing, or all of its children normalized away) is a leaf: nothing is indented under it.

Non-ASCII and non-printable characters inside `Text("…")` are written as Rust `\u{…}` escapes; `"` and `\` are escaped.

## Cases

| Directory | Rule |
|---|---|
| `multiline-text` | Text spread over several lines collapses to one string, joined with single spaces. |
| `tags-only-lines` | Elements separated only by newline + indentation produce no text nodes. |
| `inline-spaces-kept` | Text on the same line as its surrounding tags is never trimmed at either edge: `<p> a </p>` keeps both spaces; `<b>x</b> y` keeps the leading space of `" y"`. |
| `text-then-expression` | `Hello {name}` is two children: `Text("Hello ")`, `Expr(name)`. |
| `expression-then-text-newline` | `{name}` followed by a newline and text is `Expr(name)`, `Text("world")` — the text after the island is trimmed as an interior run, not treated as "same line". |
| `blank-lines-dropped` | A blank line in the middle of multi-line text is dropped, not turned into a double space. |
| `single-line-whitespace-only` | `<p> </p>` (one line) yields `Text(" ")`; `<p>\n</p>` (the same whitespace split across lines) yields nothing. |
| `element-between-text` | `a <b>x</b> c` on one line is `Text("a ")`, `Element(b)`, `Text(" c")`. |
| `leading-trailing-lines` | A newline right after the opening tag and right before the closing tag are both dropped, leaving no extra leading/trailing space around a single content line. |
| `tab-becomes-space` | A tab inside text on a single line becomes a single space, not a trim. |
| `nbsp-not-trimmed` | A no-break space (U+00A0) on its own line survives; only U+0020 is trimmed. |
| `crlf-line-endings` | `\r\n` and a lone `\r` are both line breaks, exactly like `\n`. The file uses CRLF throughout, with one lone CR between `Hello` and `world`; an implementation that treats only `\r\n` as a break yields `Text("Hello\r        world")`. The lone CR sits inside JSX text, where Outou's lexer — not rustc — decides line breaks. |

## Provenance

All 12 cases were derived by hand from the algorithm in `docs/grammar.md` §8, which is a description of Babel's `cleanJSXElementLiteralChild` (the function `@babel/plugin-transform-react-jsx` — and so React's own JSX transform — uses to normalize JSX text). `@babel/parser` and `@babel/core` were not available in the local npm cache (checked with `npm ls -g`, `node -e "require.resolve(...)"`, and `npx --offline`, all of which failed to resolve them without network access), so **none of the cases were cross-checked by running the real Babel package**.

As a substitute, all 12 `expected.txt` files were cross-checked against a from-memory transcription of Babel's `cleanJSXElementLiteralChild` (same line-splitting, same first/last-line trimming, same last-non-empty-line bookkeeping) driven by a small hand-written JSX-fragment parser, both written for this check only and not part of the repository. That script parses each `input.jsx`, applies the transcribed algorithm to every text run, and confirms the result matches `expected.txt` exactly; all 12 matched. This is weaker than executing the actual `@babel/core` package — a transcription error would pass its own check — but it is stronger than hand arithmetic alone, since the splitting/trimming/joining logic is applied uniformly by code rather than worked out fixture-by-fixture. If `@babel/core` becomes available later, running each `input.jsx` through it directly (translating its string-literal children to this format) would be a strictly better check and should replace this note.
