# vscode-outou

Visual Studio Code support for `.rsx` files (language id `outou-rsx`).

## What exists today

- Language registration (`package.json` `contributes.languages`) and `language-configuration.json` (comments, brackets, auto-closing pairs).
- A TextMate grammar (`syntaxes/outou-rsx.tmLanguage.json`), the stand-in for semantic highlighting until `outou-lsp` serves `textDocument/semanticTokens/full` in a real editor session (`docs/phase0/issues/14-semantic-tokens-rename.md`). It highlights: Rust by embedding/including `source.rust` for everything outside JSX; JSX tags, distinguishing component names (uppercase-initial, `entity.name.tag.component.outou-rsx`) from HTML element names (lowercase-initial, `entity.name.tag.html.outou-rsx`) on both the opening and closing tag; attribute names, with event attributes (`on*`) scoped separately; double-quoted string attribute values; `{ … }` islands (attribute values and JSX children), scoped `meta.embedded.block.rust` so they re-enter Rust highlighting, including further nested JSX; JSX text; and `//` comments.
- A no-network, no-dependency structural check for the grammar (`syntaxes/outou-rsx.test.mjs`, run with `node --test packages/vscode-outou/syntaxes/*.test.mjs`): valid JSON, every `include` resolves to a repository key or a known external scope, every `match`/`begin`/`end` regex compiles, the closing-tag rule backreferences the opening tag's captured name, and a set of behavioral checks on `jsx-element`'s own `begin`/`end` regexes directly (which Rust/JSX snippets must and must not trigger a tag start, and that a hyphenated tag name's `end` closes correctly) — run as plain `RegExp`, without a real Oniguruma/vscode-textmate engine.

## Known limitations of the TextMate grammar

This is a regex-based stand-in, not a parser (`docs/grammar.md`'s real grammar disambiguates Rust vs. JSX with full tokenization, which a TextMate grammar cannot do), so it has real, permanent gaps (issue #14 review, SHOULD-LAND-9):

- **Rust `<`/`>` false positives/negatives remain possible.** `jsx-element`'s `begin` pattern uses a lookbehind (rejects a `<` immediately preceded by a word character, `>`, `)`, `]`, `:` or `.` — blocking `Vec<T>`, `impl<T>`, `x.collect::<Vec<_>>()`, `fn f<T: Clone>`) and a lookahead (rejects a name immediately followed by only whitespace then `{`, blocking `if a <b {}`). Both are heuristics: a generic type parameter written with a space before `<` (`Vec <T>`), or a comparison whose right-hand side happens to look exactly like a JSX-shaped continuation, can still be misclassified.
- **A misclassified `begin` match is bounded, not silently unbounded.** `jsx-element`'s `end` pattern has an extra zero-width alternative that fires at the start of a line beginning with `}`, or a top-level item keyword (`fn`, `pub`, `impl`, `struct`, `enum`, `trait`, `mod`, `use`, `#[`) — so a false-positive tag-start does not swallow the rest of the file into `meta.tag.outou-rsx`, only the region up to the next such line.
- **A bare (valueless) attribute (`<input disabled />`) and an ordinary word of JSX child text are the same shape to this grammar.** `jsx-bare-attribute-name` and `jsx-text` are both tried within the same `patterns` list (attributes and children are not scanned in separate sub-scopes), so a plain word in JSX text can be highlighted as an attribute name.
- **JSX spread attributes (`<div {...props}>`), if ever added to the grammar, are not supported** — the lookahead that blocks `if a <b {}` (an ordinary Rust comparison) would also reject that syntax; Outou's own grammar does not have spread attributes as of Phase 0.

There is still no client for `outou-lsp`: the extension gains one once the server's LSP surface (hover, completion, definition, formatting, and now semantic tokens/rename/references where implemented) is exercised end to end in a real editor session.
