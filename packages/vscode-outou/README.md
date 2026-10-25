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

The extension also has a development-only client for `outou-lsp` (below). It is not a release: the package stays `private`, is not published and has no bundling or `.vsix` step. What a real editor session showed (VS Code 1.142.0, 2026-10-25, every checklist item OK) is recorded in `examples/dogfood-board/README.md` (F17) and `docs/alpha-results.md`.

## Trying outou-lsp in VS Code (development only)

`extension.js` is plain CommonJS (no build step). It starts `outou-lsp` over stdio and uses `vscode-languageclient` with its default client capabilities, so the server sees what a real editor declares (`window.workDoneProgress`, `workspace.didChangeWatchedFiles`, ...). That negotiation is the point of this client (F17); the headless Node clients cannot check it.

Settings: `outou.server.path` (the `outou-lsp` binary; empty means `target/debug/outou-lsp`, then `target/release`, in this repository, then in the ancestors of the workspace folder; a missing file is an error message, not a silent fallback) and `outou.trace.server` (`off` / `messages` / `verbose`). The channel "Outou Language Server" is a VS Code log channel, so the LSP trace appears only after its log level is set to `Trace` (`Developer: Set Log Level...` > Outou Language Server > Trace); the setting then selects `messages` or `verbose`.

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
- Code changes to `extension.js` need `Developer: Reload Window` in the Extension Development Host (or restart it); changing only `outou.server.path` needs a reload too. `Failed to start: TypeError: ... is not a function` in the Output channel means the client library was given the wrong kind of object (it needs `createOutputChannel(name, { log: true })`); `node --test test/*.test.js` in this package reproduces that without VS Code.
- Open the crate directory `examples/dogfood-board/app`, not the Cargo workspace root `examples/dogfood-board`: from the root, `outou-lsp` runs in its degraded mode (syntax diagnostics only).
- A workspace opened through a symbolic link works (the server maps the editor's spelling of each URI); if something still looks wrong there, open the real path instead and say so when reporting.
- If `Ctrl+Space` does nothing on macOS, it is probably taken by the input method (Google Japanese Input): use the Command Palette, "Trigger Suggest". `Shift+F12` and `F2` are function keys on a Mac keyboard; use the context menu ("Find All References", "Rename Symbol") or the Command Palette instead.
- Grey inline suggestions are an AI feature, not `outou-lsp` completion (completion is the popup list); turn `editor.inlineSuggest.enabled` off to avoid confusing them. VS Code's word-based suggestions may also appear in the same list.
- The Output channel currently also shows rust-analyzer's standard error, including generated Rust (ledger row 53, #31); that is a known leak, not something you did.
- Where to look when it does not work: Output panel, channel "Outou Language Server" (server stderr and, with `outou.trace.server` set, the LSP traffic); Help > Toggle Developer Tools > Console of the Extension Development Host; the Extensions view (is "Outou" activated? it activates on `onLanguage:outou-rsx`, so open a `.rsx` file).

Checklist (open `app/src/task_list.rsx` unless noted; record gaps in `examples/dogfood-board/README.md`, F17). After checking each item, undo the edit with `Cmd+Z`. Line numbers are based on the current contents of `task_list.rsx`.

| # | What to try | Expected |
|---|---|---|
| 1 | Starts: open a `.rsx` file | Output "Outou Language Server" shows `Starting .../outou-lsp`; no error toast. |
| 2 | Progress: right after opening, watch the status bar and notifications | `Outou: indexing`, then `Outou: checking`, appear and eventually disappear. No `rust-analyzer` or crate name appears. |
| 3a | Syntax diagnostic: change `</p>` on line 17 to `</q>` | A red squiggle at that position for the mismatched closing tag. Restoring it clears the squiggle. |
| 3b | Remove the `/>` on line 25 | An error saying the tag is not closed. Restoring it clears the error. |
| 3c | Type error: change line 38 to `let id: u8 = task.id;` and save | After the save, `mismatched types` appears on line 38. No `dioxus`, `rsx!` or `.generated` wording. |
| 3d | Old API: change `get()` on line 9 to `read()` and save | An error saying `State` has no method `read`. The word `Signal` does not appear. |
| 4a | Tag completion: add a new line between lines 14 and 15 (inside `<section>`) and type `<Ta` | `TaskList`, `TaskRow`. Typing `<d` offers HTML elements such as `div`. |
| 4b | Attribute completion: add a new line between lines 52 and 53 (inside `<input`) and type `ch` | `checked`. Typing `on` offers `onchange`, `onclick`, and so on. |
| 4c | Rust completion: on line 36 (`let board = app.board;`), stop right after `app.` | `board`, `filter`, `panel`. No name starting with `__outou_`. (Unconfirmed expectation: methods such as `clone` may also be listed.) |
| 4d | State completion: on line 9 (`app.board.get()`), stop right after `app.board.` | `get`, `with`, `set`, `update`; no `read` or `write`. (Unconfirmed expectation: whether anything beyond these four, such as `clone`, is listed.) |
| 5 | Hover: hover `board` on line 9 and `filter` on line 10 | A `State<...>` type. No `Signal<...>`. Hovering a tag name such as `section` shows nothing. |
| 6a | Go to definition: `Cmd+click` `use_app_state` on line 8 | Jumps to line 43 of `state.rs`. |
| 6b | `Cmd+click` `TaskRow` on line 21 | Jumps to line 34 of the same file. |
| 6c | `Cmd+click` `Panel` on line 4 | Jumps to line 9 of `state.rs`. It must never open anything under `src/.generated/`. |
| 7 | References: `Shift+F12` on `TaskRow` on line 34 | Lines 21 and 34. Nothing from `.generated`. |
| 8a | Rename: `F2` on `TaskRow` on line 34, enter `TaskItem` | Both line 21 and line 34 change. Undo afterwards. |
| 8b | Rename (known limitation): `F2` on `type` on line 51 | Refused, or nothing happens. Known (#31, ledger row 29). |
| 9 | Formatting: add 8 spaces at the start of line 60, then `Option+Shift+F` | The original indentation returns. A second run changes nothing. |
| 10 | Semantic tokens: run "Developer: Inspect Editor Tokens and Scopes", then click `TaskRow` on line 21 and `checked` on line 52 | A semantic token type entry (such as component / property) is shown. (Unconfirmed expectation: the exact type names.) |
| 11 | serverStatus: add a deliberately invalid line to `app/Cargo.toml` and save | A single `Outou: ...` toast within a few seconds. (Unconfirmed expectation: timing and wording.) Afterwards run `git checkout -- examples/dogfood-board/app/Cargo.toml`. |

Notes for the checklist:

- The progress in item 2 may not be visible when the server is fast.
- Also hover `crate` in `use crate::state::...` (line 4) and report what it shows.
- Reporting: for each item write OK, or NG with what you saw, or "could not try". For NG, include the relevant part of the Output channel (with `outou.trace.server` set and the channel's log level at `Trace`, as described above, the LSP traffic is there too).
