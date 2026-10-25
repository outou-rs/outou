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

The extension also has a development-only client for `outou-lsp` (below). It is not a release: the package stays `private`, is not published and has no bundling or `.vsix` step. What a real editor session shows is recorded in `examples/dogfood-board/README.md` (F17) once someone has run it.

## Trying outou-lsp in VS Code (development only)

`extension.js` is plain CommonJS (no build step). It starts `outou-lsp` over stdio and uses `vscode-languageclient` with its default client capabilities, so the server sees what a real editor declares (`window.workDoneProgress`, `workspace.didChangeWatchedFiles`, ...). That negotiation is the point of this client (F17); the headless Node clients cannot check it.

Settings: `outou.server.path` (the `outou-lsp` binary; empty means `target/debug/outou-lsp`, then `target/release`, in this repository, then in the ancestors of the workspace folder; a missing file is an error message, not a silent fallback) and `outou.trace.server` (`off` / `messages` / `verbose`, written to the Output channel "Outou Language Server").

Once, from the repository root:

```bash
pnpm install                       # vscode-languageclient, pinned in this package
cargo build -p outou-lsp
cargo run -p outou-cli -- build --manifest-dir examples/dogfood-board    # optional, see below
```

Then start an Extension Development Host on the dogfood app (open the **crate directory** `examples/dogfood-board/app`, not `examples/dogfood-board`: `outou-lsp` looks for `src/main.rsx` / `src/lib.rsx` directly under the workspace folder and otherwise falls back to a syntax-diagnostics-only mode):

```bash
code --extensionDevelopmentPath="$PWD/packages/vscode-outou" "$PWD/examples/dogfood-board/app"
```

`~/.local/bin/cursor` takes the same arguments. Or open `packages/vscode-outou` in VS Code and press F5 (`.vscode/launch.json`).

Notes:

- `outou build` is not required: `outou-lsp` writes the generated files itself, with the same Strict path as `outou build`, if any is missing. Run it if you want a fresh `app/src/.generated/` first (stale but complete files are left alone until the first save).
- The `rust-analyzer` extension, if installed, runs on the same workspace in addition to the rust-analyzer process `outou-lsp` spawns (see "Two processes, not one" in `crates/outou-lsp/README.md`). Expect two. To keep them apart, disable the rust-analyzer extension in the Extension Development Host. `outou-lsp` needs a `rust-analyzer` on `PATH` (or `OUTOU_RUST_ANALYZER`); VS Code must be started from a shell that has it, or set the variable.
- Where to look when it does not work: Output panel, channel "Outou Language Server" (server stderr and, with `outou.trace.server` set, the LSP traffic); Help > Toggle Developer Tools > Console of the Extension Development Host; the Extensions view (is "Outou" activated? it activates on `onLanguage:outou-rsx`, so open a `.rsx` file).

Checklist (open `app/src/task_list.rsx` unless noted; record gaps in `examples/dogfood-board/README.md`, F17):

| Check | How | Expected |
|---|---|---|
| Starts | Open any `.rsx` file | Output "Outou Language Server" shows `Starting .../outou-lsp`; no error toast |
| Progress | Right after opening | Status bar shows `Outou: indexing` (later `Outou: checking`), then disappears |
| Diagnostics | Delete a closing `>` or a closing tag, or save a type error | A red squiggle at the `.rsx` position, Outou wording (no `rsx!`/`dioxus` words); fixing it clears it (type errors appear after save) |
| Completion: tag | Type `<` then `Ta` | Component names (`TaskList`, ...) and HTML elements |
| Completion: attribute | In `<input ` type `pl` / `val` | Attribute names |
| Completion: Rust expression | Inside `{ ... }`, type `app.` | Fields/methods of the Rust type; no `__outou_*` names |
| Hover | Hover a variable, e.g. `board` | Type info; on a tag name hover shows nothing |
| Go to definition | F12 on a component tag or on `use_app_state` | Jumps to the definition (`.rsx` or `.rs`), never to `src/.generated/` |
| References | Shift+F12 on `TaskList` | Locations in `.rsx` files only |
| Rename | F2 on a component name, e.g. `FilterBar` | All uses renamed across `.rsx`/`.rs`; a rename it cannot map is refused with an Outou message |
| Formatting | Misindent a block, then Format Document (Shift+Alt+F) | Rust and JSX are re-formatted; a second run changes nothing |
| Semantic tokens | Command "Developer: Inspect Editor Tokens and Scopes" | Components, elements, attributes carry semantic token types on top of the TextMate scopes |
| serverStatus | Make rust-analyzer unhappy (e.g. break `Cargo.toml`) | One Outou-worded warning/error toast when health is not `ok` |
