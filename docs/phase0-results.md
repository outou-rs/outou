# Phase 0 results and decision (Gate 4)

This is the Step 8 report required by `docs/phase0.md` and issue #16. It answers the Phase 0
question — can `.rsx` feel like a first-class Rust file inside a real Rust project? — from evidence,
not impressions. Every claim below cites a file, a fixture, or a command a reader can re-run.

## 1. Summary and verdict

Across eight weeks, Outou built a mode-aware lexer and recovering parser for a JSX expression
grammar layered onto Rust (`outou-syntax`), a many-to-many source-map model (`outou-sourcemap`), a
module resolver for mixed `.rs`/ `.rsx` crates (`outou-modules`), a Dioxus-backed code generator
behind a `Backend` trait (`outou-codegen`, `outou-backend-dioxus`), a facade crate that hides the
backend from user code (`outou`), a `build`/`check` CLI (`outou-cli`), and a language server that
proxies rust-analyzer with full position mapping and backend-vocabulary sanitization (`outou-lsp`).
Gates 0, 1 and 2 were reached without changing strategy, each after its own review-and-fix pass
(`c898ac4`, `3937eb3`); Gate 3 required two review-and-fix cycles before its evidence could be
trusted (`docs/gate3-results.md`), and the parser corpus run found and fixed one CRITICAL,
currently-affecting defect (issue #12, F1) before reaching its final 0-panic, 0.10%-false-positive
result. Every "must not be cut" item in `docs/phase0.md` is delivered; every explicitly droppable
item (#13, #14, #15) was in fact dropped, as planned, and each is a stub or absent rather than
partially built. The Phase 0 criteria, read literally against `docs/phase0.md` and `docs/design.md`,
are met.

| Gate | Verdict | Evidence |
|---|---|---|
| Gate 0 — rust-analyzer feasibility | **PASS** | `docs/ra-spike-results.md` ("Decision" section); `docs/phase0/issues/01-ra-feasibility-spike.md` |
| Gate 1 — parser and recovery | **PASS** | `docs/phase0/issues/04-lexer-parser-recovery.md` (fully ticked); `crates/outou-syntax/tests/{never_panics,fuzz}.rs`; corpus run below (0 panics, 0 timeouts over 20,722 files) |
| Gate 2 — Cargo build | **PASS**, not triggered | `docs/phase0/issues/08-cargo-build-determinism.md` ("Not triggered" statement); §4 below |
| Gate 3 — integrated language server | **PASS**, twice re-verified | `docs/gate3-results.md` (full write-up, two review cycles, H1-H3 fixed) |
| Gate 4 — decision | **the Phase 0 criteria are met** | this document |

Section 10 states the verdict formally, in the vocabulary AGENTS.md permits for a public document
(no project-continuation judgment), together with the conditions and risks a front-end alpha would
carry, and a paragraph on what the `outou::jsx!` fallback would have cost.

## 2. Parser

### Grammar coverage

`docs/grammar.md` §§1-8 are all implemented and tested: the three-mode lexer (§2), opaque
macro/attribute token trees (§3), the `<`-ambiguity rules (§4, including the bounded angle scan for
`<Name<`), elements and attributes (§5), Rust expression islands (§6), the Phase 0 restrictions (§7,
no postfix on a JSX expression, no generics in tags), and React/Babel-matching whitespace handling
(§8, 13 golden fixtures under `tests/fixtures/formatting/`, each paired with an equivalent React
input). §9 (recovery) and §10 (reserved constructs) are covered next.

### Recovery quality

The parser never panics on any input, including every byte-boundary prefix of every fixture
(`crates/outou-syntax/tests/never_panics.rs`) and a 46-symbol-alphabet, fixed-seed fuzz test
(`crates/outou-syntax/tests/fuzz.rs`, `FUZZ_ALPHABET`, LCG seed `0x9E3779B97F4A7C15`). Of
`docs/grammar.md` §9's 15 required recovery cases, six are asserted verbatim against a fixture's
sibling `.expected` file (`docs/phase0/issues/04-lexer-parser-recovery.md`):

- a truncated tag/attribute name at end of input (`unterminated-attribute-name.rsx`)
- a tag terminated by an enclosing block's `}` and the resulting unclosed element
  (`unclosed-nested-tag.rsx`)
- a mismatched closing tag, reported once, not cascaded (`mismatched-closing-tag.rsx`)
- an unterminated attribute-value island (`unterminated-attribute-value.rsx`)
- a stray `}` in element content (`stray-rbrace-in-text.rsx`)

Five more are asserted elsewhere: an unterminated attribute-value string
(`unterminated-string-swallows-block-close`), an empty attribute-value island
(`class-attribute-value-mid-file`, `crates/outou-syntax/tests/islands.rs`), the nesting-cap pair —
JSX and inline `mod` past 128 levels — in `never_panics.rs`, and reserved syntax via
`tests/ui/reserved-fragment`. **Four have no fixture and no test asserting their diagnostic text at
all** — a stray closing tag, a truncated closing tag at end of input, a closing tag interrupted by
`<`, and an unclosed island at end of file; the strings exist only in
`crates/outou-syntax/src/parser/diag.rs`. Recorded as a Phase 0 test gap, not fixed here; writing
the missing fixtures is issue #4 work.

**The swallowed-tail limitation.** A body-less function whose parameter list never reaches a closing
`)` (`fn App(` cut off mid-file) is not always diagnosed as an error: the parser can swallow the
rest of the file into that function's own signature text with zero diagnostics
(`crates/outou-backend-dioxus/README.md:30`, "Recovery-mode placeholders", CRITICAL-1). Strict-mode
codegen splices the whole span verbatim so rustc reports the real unclosed-delimiter error at the
right place rather than silently compiling a truncated file; Recovery mode keeps the function's
*symbol* resolvable (name mapped, body replaced with a placeholder) so outline/completion/definition
survive, but a swallowed tail can still take following items with it before that symbol-preservation
gets a chance to help
(`tests/fixtures/incomplete/{bodyless-fn-mid-file,unclosed-island-mid-file}.rsx`,
`crates/outou-backend-dioxus/tests/recovery.rs`). This is recorded as a parser/recovery limitation
(`crates/outou-backend-dioxus/README.md:30`, LOW-17), not fixed in Phase 0.

**Nesting cap.** JSX elements and inline modules are each capped at 128 levels of nesting
(`docs/grammar.md` §9, decision D4). Past the cap the parser diagnoses `this element is nested too
deeply` / `modules are nested too deeply (Outou supports at most 128 levels)` instead of recursing
further; 128 was chosen with roughly 3x margin over the deepest nesting (500 JSX levels, 1000
inline-module levels) observed to overflow a 2 MiB debug-build thread stack.

### Known ambiguities and reserved constructs (`docs/grammar.md` §10, plus the corpus-found residual)

| Construct | Status |
|---|---|
| Fragments (`<>`), spread attributes, namespaced names, dotted tag names, generic arguments in tags, postfix on a JSX expression, single-quoted attribute values, `<Self />` | Rejected, diagnosed by Outou |
| Tag names `dyn`/`impl`/`fn`/`unsafe`/`extern`/`for` and hyphenated names beginning with one | Reserved; read as Rust, no Outou diagnostic (§4 rule 2's `t1` keyword row) |
| Text starting with `::` after an opening tag (`<A>::B</A>`) | Reserved; read as Rust — write `<A>{"::B"}</A>` |
| A bare attribute named `as` (`<link as />`) | Reserved; read as Rust — write `as="…"` |
| HTML entities (`&amp;`, `&nbsp;`, …) | Not decoded in Phase 0; pass through `JsxText` verbatim as literal text |
| Comments inside JSX (`{/* … */}`) and empty islands (`{}`) | Allowed — produce no child and no diagnostic, matching React |
| Conditional / `{if}`-style syntax | Not reserved — Rust islands already cover it; nothing to reserve |
| A bare `:` immediately before `<` (`x:::<T`-shaped input) | Documented residual risk (D2/H4, `docs/phase0/issues/12-corpus.md`): `:` is deliberately not a type-only position in the disambiguator, so a handful of deliberately-invalid rustc torture fixtures (`triple-colon.rs`, `issue-57819.rs`) misparse; accepted trade-off, see the corpus breakdown below |
| JSX inside a macro or attribute token tree | Not recognized; no Outou diagnostic is possible (§3) — an accepted gap |

### Corpus results

`cargo xtask corpus test` (run once, this session) against `rust-lang/rust` tag `1.98.1` (commit
`48a229ceaefd4985c50990b14116b6d856af0985`, pinned in `corpus.lock`), `tests/ui`, 20,722 files:

```text
panics: 0
timeouts: 0
false positives (Outou diagnostics on plain Rust): 20 (0 on valid Rust, 20 on Rust rustc itself rejects)
JSX mis-detections (a JSX element found on plain Rust): 20
splice round-trip mismatches: 0
```

This matches `docs/phase0/issues/12-corpus.md`'s committed result exactly. The 20 false positives
break down as: 13 literal `<<<<<<< HEAD` merge-conflict marker fixtures under
`tests/ui/parser/diff-markers/`, 4 parser-torture fixtures deliberately testing rustc's own error
recovery on invalid input (`eq-less-to-less-eq.rs`, `range-exclusive-dotdotlt.rs`,
`unmatched-langle-2.rs`, `type-ascription-instead-of-path-2.rs`), 2 bare-`:`- before-`<` cases (the
documented D2/H4 residual), and 1 bare `<>` after a doc comment (arguably spec-correct per the
fragment-rejection rule). **Zero** of the 20,722 files of ordinary, valid Rust produced a false
positive.

**The parser defect the corpus found and fixed (issue #12, F1, CRITICAL).** At item level, the
disambiguator's "previous significant token" context was reset whenever *any* trivia (including
plain whitespace or a comment with no attribute) was skipped, not only when an attribute was
actually consumed. With that context lost, the classifier fell back to a permissive start-of-file
default and misread the next `<` as JSX — on ordinary, current Rust: `impl <T> Foo<T> {}`, `struct X
<T> {}`, `const C: bool = A < B;`, `type Alias = Vec <u8>;`. This is what an earlier,
since-corrected version of the corpus writeup had mischaracterized as "pre-2018-syntax constructs" —
a wrong root cause that would have hidden a live, current-Rust-affecting bug from Gate 4. Fixed in
`crates/outou-syntax/src/parser/item.rs`; regression tests in
`crates/outou-syntax/tests/item_level_less_than.rs`.

## 3. Modules

Implemented (`outou-modules`, issue #07, all items ticked):

- candidate resolution for `mod foo;` — `foo.rsx`, `foo.rs`, `foo/mod.rsx`, `foo/mod.rs` — with more
  than one existing file an error (`ambiguous Outou module`), never resolved by priority (ADR 0006);
  a same-named directory is not itself a candidate
- `#[path = "…"]` honored, directory ownership validated directly against rustc 1.98.1
  (`tests/fixtures/modules/{path-attr,path-dirs}`)
- `#[cfg(…)]` preserved verbatim, never evaluated — every module is generated regardless of its
  `cfg` (`tests/fixtures/modules/cfg/`)
- inline modules (`mod name { … }`) lowered in place, no separate generated file
- cycle detection (`ModuleError::Circular`) and a 128-level depth cap (`ModuleError::TooDeep`) on
  `#[path]` chains, replacing what was previously a stack-overflow SIGABRT

### Limitations

- **A plain `.rs` file may not declare a `.rsx` child module.** Codegen only ever rewrites a `mod`
  declaration's `#[path]` inside a *generated* file; a `.rs` file's own source is never touched.
  `outou build` detects this shape and reports an Outou diagnostic rather than silently failing
  (`crates/outou-cli/src/build/plan.rs`, `PlanError::RustDeclaresRsxChild`;
  `docs/phase0/issues/08-cargo-build-determinism.md`). `.rsx` modules must be declared from another
  `.rsx` file or the crate root.
- **`#[cfg_attr(condition, path = "…")]` is rejected** (`ModuleError::ConditionalPath`): Phase 0
  does not evaluate `cfg`, so it cannot choose which of a conditional path's targets to resolve.
- **Out-of-crate `#[path]` targets are rejected** (`ModuleError::OutsideCrate`): an absolute path,
  or enough `..` segments to escape the crate root, is an error rather than a silently-resolved
  external file.
- **Generated-path scheme:** every module's generated file lives under `src/.generated/`, mirroring
  its module path; the crate root always uses the reserved stem `crate-root` regardless of
  `main`/`lib` naming, so a child module named `main` cannot collide with it. A same-named sibling
  (the `cfg`-exclusive `mod imp;` idiom) is disambiguated with a `-{n}` suffix in source order
  (`crates/outou-modules/README.md`, "Generated-path convention").

## 4. Cargo workflow

**`outou build && cargo build`.** `outou build` resolves the module graph, generates one Rust file
per module under `src/.generated/` (ADR 0009 layout (b): fixed path + `#[path]`, no build script, no
`OUT_DIR`), and writes a sibling `<name>.rs.map.json`. The `[[bin]]`/`[lib]` target points directly
at `src/.generated/crate-root.rs`; `cargo build` needs nothing beyond that.

**Layout (b) and why.** Layout (a) (`OUT_DIR` + `include!`) was tried first and rejected: it failed
the one non-droppable "completion while input is incomplete or broken" criterion, specifically
inside the generated `rsx!` macro call and under incomplete input — precisely the two shapes Outou's
generated code always has — while returning correct completion everywhere else
(`docs/ra-spike-results.md`, "Known blockers"; `docs/adr/0009-generated-source-location.md`). Layout
(b) satisfied all seven Strategy A criteria.

**The Cargo command matrix** (issue #10, `crates/outou-cli/tests/matrix.rs`): `cargo
build`/`check`/`test`/`clippy --all-targets -- -D warnings` all pass for `examples/phase0-app`;
`cargo build`/`test`/`clippy --workspace --all-targets -- -D warnings`, plus `cargo package --list`
and `cargo publish --dry-run`, all pass for the two-crate workspace fixture
(`tests/fixtures/workspace/{ui-kit,app}`; `cargo check` is not separately run against the
workspace). `#[cfg(test)] mod tests` inside a `.rsx` file runs under `cargo test` exactly as it
would in a `.rs` file. Plain (JSX-free) `///` doc tests in a `.rsx` file with a `[lib]` target run
under `cargo test --doc`; a `[[bin]]`-only crate (`examples/phase0-app`, by ADR 0009's application
layout) cannot run *any* doc test, plain or JSX — an ordinary Cargo constraint (a doc test links
against the crate as a library), not an Outou gap.

**MSRV.** `Cargo.toml` sets `rust-version = "1.85"` (clap 4.6 requires it);
`.github/workflows/ci.yml` runs the test job on a `[stable, "1.85"]` matrix, which is where `cargo
+1.85 check --workspace` (AGENTS.md's fourth required check) is exercised — not re-run locally for
this report.

**Workspace support.** `outou build --manifest-dir <workspace root>` reads `[workspace]
members`/`exclude` and builds every member with an `.rsx` crate root in one pass
(`crates/outou-cli/src/build/workspace.rs`). Member-path globbing is limited to a single trailing
`/*` segment (`TODO(phase0)`, §9 below); `default-members` is not read.

**Determinism.** `cargo xtask determinism` (run once, this session):

```text
determinism: 10 target(s) OK
```

Every fixture is generated through the build path and the (until issue #9, a stand-in)
language-server path, normalized, and compared byte-for-byte for both the Rust output and the source
map.

**Cold/incremental build times** (measured this session; machine as recorded in
`docs/ra-spike-results.md`'s Environment table — macOS, Darwin 25.6.0, arm64; rustc/cargo 1.98.1):

`examples/phase0-app` (`cargo clean`, then `outou build && cargo build`; then `touch src/main.rsx`,
then `outou build && cargo build` again):

```text
$ cargo clean && rm -rf src/.generated
$ time (target/debug/outou build && cargo build)
   ... full dioxus dependency tree compiles from scratch ...
real 17.30s   user 31.81s   sys 6.86s

$ touch src/main.rsx
$ time (target/debug/outou build && cargo build)
outou build: generated 2 file(s), removed 0 stale file(s)
   Compiling phase0-app v0.0.0 (...)
real 1.55s   user 0.22s   sys 0.34s
```

`tests/fixtures/workspace` (same recipe, `--manifest-dir .`, `cargo build --workspace`, then `touch
app/src/main.rsx`):

```text
$ cargo clean && rm -rf ui-kit/src/.generated app/src/.generated
$ time (target/debug/outou build --manifest-dir . && cargo build --workspace)
   ... dioxus dependency tree + ui-kit + app ...
real 19.83s   user 32.87s   sys 7.62s

$ touch app/src/main.rsx
$ time (target/debug/outou build --manifest-dir . && cargo build --workspace)
outou build: generated 6 file(s), removed 0 stale file(s)
   Compiling ui-kit v0.0.1 (...)
   Compiling app v0.0.0 (...)
real 1.37s   user 0.23s   sys 0.41s
```

Both cold numbers are dominated entirely by compiling the `dioxus` dependency tree once (the same
cost any Dioxus project pays; Outou's own front end contributed well under 100ms of that, per the
corpus/unit-test timings above) and are not repeated on a warm `target/`. The incremental numbers
(~1.3-1.6s) are dominated by `cargo build`'s own recompilation of the touched crate(s), not by
`outou build`, which itself completes in a small fraction of a second. One honest caveat found while
measuring: **`outou build` itself is not incremental** — every invocation regenerates every unit in
the plan (the workspace run above rewrote all five of `ui-kit`'s generated files even though only
`app/src/main.rsx` changed), unlike `outou-lsp`'s single-unit regeneration for the editor overlay —
recorded in §6 as a budget miss on the CLI path. Because generation is deterministic, this did not
force a needless recompilation here (`git status --short` stayed clean throughout — the regenerated
bytes matched what was already committed for `ui-kit`, and `examples/phase0-app`'s `.generated/` is
gitignored), but a build system that treats mtime rather than content as its staleness signal would
pay for regenerating unrelated files this way.

## 5. IDE feature matrix

| Feature | Status | Evidence |
|---|---|---|
| Completion — member/expression (via rust-analyzer) | Works | `docs/gate3-results.md`, `completion (member, user.)`: 122 items |
| Completion — tag/component/element names (via Outou) | Works, answered locally | `docs/gate3-results.md` M3: `<UserC` -> exactly `UserCard`; `<div` -> exactly `div`; never forwarded to rust-analyzer |
| Completion — closing-tag name | Works, answered locally | `docs/gate3-results.md` H1 fix |
| Completion — attribute name on a known component | Works, answered locally | `docs/gate3-results.md` M3: `<UserCard us` -> exactly `user` |
| Completion — prop value (`user={us}`) | Works (forwarded) | `gate3-completion-prop-value.json.gz`: 129 items |
| Completion — HTML attribute value (`<div class=`) | Returns nothing, by design | `gate3-completion-attr-value.json.gz`: `completion: null` — every forwarded item's mapped edit range failed the cursor-containment check. The underlying recovery shape is a known gap, made editor-safe rather than fixed (`docs/gate3-results.md`, "Not applied as reviewed") |
| Hover | Works (forwarded, sanitized) | `let user: Option<User>`, exact range asserted; non-ASCII position mapping verified (M7) |
| Hover — JSX tag name (element or component) | Correctly suppressed | Answered `null` locally, opening and closing tag (M4, H2) — a `dioxus_html` rustdoc leak fixed |
| Hover — user type coincidentally named `…Props` | Works normally | H3 fix, anchored to the plan's real `#[component]` functions |
| Definition — same file | Works | `load_user()` -> `main.rsx:42:3-12` |
| Definition — cross-file | Works | `UserCard` -> `components.rsx:30:7-15` |
| Diagnostics — Outou syntax, live | Works, no round trip | `didChange` -> syntax diagnostic in ~100ms |
| Diagnostics — rustc semantic/backend, on save | Works via flycheck only | Native (pre-save) diagnostics never report semantic errors (rust-analyzer limitation, not Outou's, per the Week 1 spike); `didSave` -> flycheck maps back correctly |
| Semantic tokens | **Not implemented** | Droppable #14; a TextMate grammar was meant to stand in (`packages/vscode-outou/`) — also not built |
| Formatting | **Not implemented** | Droppable #13 |
| Rename / references | **Not implemented** | Droppable #14 |

Known limitations from `docs/gate3-results.md`: two separate rust-analyzer processes per workspace
(this server's own, plus the editor's for ordinary `.rs` files); semantic diagnostics require a
save; a transformed (non-verbatim) source mapping returns `null` rather than a guessed position;
recovery for `<div class=` (a broken tag swallowing a following sibling element) is not fixed, only
made editor-safe (an honest empty completion, never a corrupting edit).

## 6. Latency

Compared against `docs/phase0.md`'s provisional budget:

- **"incremental `.rsx` -> generated Rust: perceived as instantaneous."** Met:
  `regenerating_one_unit_is_fast` asserts well under 50ms (in practice a small fraction of a
  millisecond) for the LSP's single-unit regeneration.
- **"extra proxy overhead on completion: not perceptible." Missed as literally worded for forwarded
  completion; met, and better than met, for the four positions answered locally.** Revised after
  actually measuring it (`docs/gate3-results.md`, "Proxy overhead (S8)"): for the four positions
  answered locally (tag name, element tag name, closing-tag name, attribute name) the overhead is a
  few milliseconds of in-process classification — better than "not perceptible" since there is no
  round trip at all. For positions still forwarded to rust-analyzer, a same-session A/B measurement
  against rust-analyzer directly (N=10, three repeated runs) found hover/definition overhead within
  noise (-1 to +3ms), but completion overhead **real and attributable**: roughly 10-25ms median,
  proportional to the forwarded candidate list's size (leak-scanning and reverse-mapping every
  item), not a fixed proxy tax — still well under human-perceptible lag for one keystroke, but not
  "not perceptible" as the budget line had, unmeasured, claimed.
- **"a single-file edit never regenerates the whole crate." Met on the language-server path; missed
  on the `outou build` CLI path.** `regenerating_one_unit_does_not_touch_another` asserts a sibling
  unit's generated text is byte-identical after another file is edited. `outou build` gives no such
  guarantee — it regenerates every unit in the plan on every invocation (§4); the workspace
  measurement above rewrote all five of `ui-kit`'s generated files for an edit confined to
  `app/src/main.rsx`. `docs/phase0.md` does not scope this line to either path, so it is recorded as
  met on one of the two paths Outou ships, not as met outright.

Editing-to-diagnostic latency: Outou's own syntax diagnostics publish in ~100ms after `didChange`
(no round trip); a Rust type error needs `didSave` plus flycheck, measured at 4.7-8.9 seconds end to
end across the Gate 3 probes (`docs/gate3-results.md`, "Measured latency") — this is a rust-analyzer
limitation (native diagnostics never report semantic errors, confirmed even in plain non-macro code
by the Week 1 spike's `supplementary-diagnostics-nonmacro-*` probes), not something `outou-lsp` can
shorten. Cold `initialize`-to-first-usable-hover/completion/definition figures in the same table run
1.3-26 seconds depending on server load and request shape (the 26 s outlier is `completion (prop
value)`, which that table itself flags as a slow moment); those numbers reflect a cold,
still-indexing rust-analyzer and are explicitly not treated as the honest overhead figure — the S8
same-session comparison above is.

## 7. Diagnostic leakage and backend leakage

`docs/backend-leakage.md` has 28 rows. By classification: **17** pure "Dioxus leakage", **1** more
"Dioxus leakage (fixed)" (row 28) and **1** "Dioxus leakage / Temporary limitation" (row 24) — 19
rows total naming Dioxus leakage; **8** pure "Temporary limitation" rows plus the one shared with
row 24 — 9 rows total; **1** "Outou intrinsic (marker) / Dioxus leakage (lowering)" (row 6); **0**
rows classified "Accepted permanent behavior" — Phase 0 never decided to keep a Dioxus detail on its
own merits; every leak recorded is either provisional or a known gap.

**Rows that make Outou semantics unnatural** (the ones a runtime decision would most want to
remove): props must be `Clone + 'static + PartialEq` (rows 1-3); `Element` is Dioxus's
`Option<VNode>` (row 4); borrowed (`&str`) props do not compile (row 10); a non-`IntoDynNode` island
child (a bare `i32`, unlike React) does not compile (row 20); a hyphenated attribute name on a
*component* has no legal Rust field-key spelling at all and reaches the user as a raw `PropsBuilder`
compile error today (rows 17-18, no fix applied); rustc's own path-printing surfaces
`outou::prelude::dioxus_core` paths and `PropsBuilder` machinery directly in ordinary compiler
output, not just in prose (row 19) — not fixable by removing the re-export, since rustc would print
the unqualified `dioxus_core::…` path instead.

**Translation/sanitization coverage.** Three UI cases exercise the "backend, translated" diagnostic
layer end to end
(`tests/ui/{backend-unknown-attribute,backend-missing-required-prop,backend-non-into-dyn-node-child}`,
issue #11), plus a `crates/outou-cli/tests/leak.rs` scan that fails the suite if any user-facing
output contains `rsx! macro`, `PropsBuilder`, `dioxus_rsx`, `GeneratedNode` or similar.
`docs/phase0/issues/11-diagnostics-ui-tests.md` surveys every leakage-ledger row and finds one
genuine, deliberate gap: row 18 (the hyphenated-component-attribute compile error) has no UI case,
because a case reproducing it verbatim would itself fail the leak scan it is meant to pass. Issue
#11's own checklist item for this ("every row in `docs/backend-leakage.md` that is a diagnostic has
a UI case") is left unticked as a result, rather than the acceptance text being reworded to paper
over it. On the LSP side, `crates/outou-lsp/tests/gate3.rs`'s `assert_no_leakage` recursively scans
every field of every probe's full JSON payload; the **first** Gate 3 review
(`docs/backend-leakage.md` rows 25-26; `docs/gate3-results.md`, M4/HIGH-9) found and fixed live
leaks the pre-review probes had never checked for at all — recursive `assert_no_leakage` did not
exist before it (diagnostic `data`, `relatedInformation` and several completion-item fields all
previously carried raw backend text). The second review (H1-H3) then found the closing-tag
hover/completion and `…Props`-named-type cases.

**Documentation staleness found while measuring, and fixed.** `crates/outou-lsp/README.md`'s
forwarding table stated `$/progress` was "Not forwarded — dropped". `docs/gate3-results.md` ("Known
blockers", S6/L12) and `crates/outou-lsp/src/dispatch/responses.rs::handle_ra_notification` both
show it is forwarded, gated on `client_supports_work_done_progress`, since the Step 7 contingency
pass. The README's forwarding table and Known-limitations section are corrected in this change.

## 8. Publish feasibility

ADR 0008 (published crates ship pre-generated Rust) is Accepted and its mechanics work: `cargo
package --list -p ui-kit` includes every file under `src/.generated/` with no `[package] include`
needed, because `ui-kit` is a tracked path inside this repository's git working tree and the root
`.gitignore` negates `**/.generated/` for exactly that path — Cargo's default packaged-file list for
a crate inside a git tree is VCS-aware (tracked files, honoring `.gitignore`), so the negated files
are treated like any other tracked source (`crates/outou-cli/README.md`, "The Cargo command
matrix"). This is worth stating precisely because Cargo's behavior differs outside a git tree: a
copy with no `.git` at all falls onto a separate "no VCS found" file-listing path that drops
dot-prefixed paths *regardless of `include`* — an earlier draft of that document was wrong about
this exact point, traced to a test helper that had (at the time) copied the fixture into a bare,
non-git directory before packaging.

**The `outou`-unpublished blocker.** `cargo publish --dry-run --allow-dirty -p ui-kit` fails: `` no
matching package named `outou` found ... location searched: crates.io index ``. `ui-kit`'s generated
code depends on `outou` (`::outou::__private::*`) as an ordinary Cargo dependency, and `outou` has
not been published to crates.io. This is an ordinary Cargo constraint (any crate with an unpublished
`path` dependency fails to publish the same way), not a dot-directory or JSX-specific one, and it is
orthogonal to ADR 0009's `src/.generated/` layout, which packages correctly on its own.

**What `outou package` (#15, droppable, not implemented) still needs:**
`crates/outou-cli/src/main.rs` has `Command::Package => todo!(...)`. Per issue #15's checklist, none
of the three items are built: (1) generating Rust and including it in the published crate — the
design is fixed (ADR 0008) and `outou build`'s own generation is reusable, but no `package`
subcommand calls it; (2) a CI job regenerating from `.rsx` and failing on a difference from the
committed/packaged Rust; (3) `cargo publish --dry-run` on a library fixture as an automated check
(done manually in this report and in issue #10's matrix, not wired into a `package` command). The
`outou`-unpublished blocker above is also unresolved: a real `outou package` needs either to require
`outou` published first, or to resolve that dependency some other way at publish time
(`TODO(phase0)`, `crates/outou-cli/README.md`).

## 9. What was cut or deferred

Per `docs/phase0.md`'s "what is cut first" list, which orders four items (Formatter,
Rename/references, Semantic tokens, Publish automation); issues #13-#15 track them as three, merging
rename/references and semantic tokens into #14 and listing that pair in the reverse of
`docs/phase0.md`'s order:

1. **Formatter (#13)** — not implemented. No placeholder-substitution pipeline, no
   `textDocument/formatting`.
2. **Semantic tokens and rename/references (#14)** — not implemented. No TextMate grammar stand-in
   either (`packages/vscode-outou/` has none).
3. **`outou package` / publish automation (#15)** — not implemented (`todo!()`); the design (ADR
   0008) is fixed and verified manually (§8).

Every `TODO(phase0)` in the tree (`grep -rn "TODO(phase0)" --include=*.rs --include=*.md .`,
excluding `target/`, `.corpus/` and this document: **31 matches**), by file:

| File | What is deferred |
|---|---|
| `docs/grammar.md`, `docs/phase0/issues/03-grammar-spec.md` (1 deferred item, 3 mentions) | `#[react_import(...)]`'s payload/semantics, pushed to the React-interop phase (only `docs/grammar.md:15` is the deferred item itself; the other two mentions, in `docs/phase0/issues/03-grammar-spec.md`, describe it) |
| `docs/backend-leakage.md` (row 11, resolved by row 16) | Whether codegen could emit fully-qualified backend paths instead of re-exporting them — investigated and rejected, not open |
| `crates/outou-syntax/src/lexer/mod.rs` (2 sites) | Non-XID-continue bytes above 0x7F accepted in a `JsxName`; a `tokenize` doc note |
| `crates/outou-syntax/src/parser/jsx/children.rs` | A byte-precise diagnostic message for one recovery shape |
| `crates/outou-backend-dioxus/tests/golden/components/expected.rs` | Whether a JSX-bearing doc test is required (it is not) |
| `crates/outou-lsp/src/uri.rs`, `docs/gate3-results.md` (LOW-16 reference) | Windows/UNC file URIs — not reachable on Phase 0's macOS/Linux platforms |
| `crates/outou-lsp/src/mapping.rs` (2 sites), `docs/gate3-results.md` (MEDIUM-15 reference) | A multi-source diagnostic always uses only its first source; a fixture/test-module duplication (`L15`) |
| `crates/outou-lsp/src/documents/workspace.rs` (3 sites), `docs/gate3-results.md` (L15 reference) | `load_unit`/`load_unit_with_text` near-duplication; `HIGH-2`'s narrower SKIP half |
| `crates/outou-lsp/src/complete.rs` (2 sites) | Recovery quality for `<div class=` swallowing a following sibling element; re-parsing every `.rsx` file on every tag/attribute-name keystroke (`L15`) |
| `crates/outou-lsp/README.md` | The SKIP items recorded at their own call sites (Windows/UNC file URIs, first-source-only multi-source diagnostics, `completionItem/resolve` not advertised) |
| `crates/outou-cli/tests/ui.rs`, `docs/phase0/issues/11-diagnostics-ui-tests.md` | `outou check --semantic` — the UI harness's semantic/backend mapping machinery is test-only, not a real subcommand |
| `crates/outou-cli/src/build/workspace.rs` (2 sites) | Member-path globbing limited to a single trailing `/*` segment; `[workspace] default-members` not read |
| `crates/outou-cli/README.md` (3 sites), `docs/phase0/issues/08-cargo-build-determinism.md` | The `.rs`-declares-`.rsx`-child limitation (§3); the globbing limitation above; the `outou`-unpublished publish blocker (§8) |
| `xtask/src/corpus/lock.rs` | A `toml`-parsing simplification (`F15`/LOW, issue #12 review) |
| `AGENTS.md` | States the `TODO(phase0)` convention itself (not a deferred item) |

**Follow-ups filed by this report** (out of this change's scope; not edited here):

- `docs/grammar.md:328` / `crates/outou-syntax/src/parser/diag.rs`'s nesting-cap margin: "roughly 3x
  margin" is really ~3.9x (JSX) and ~7.8x (modules).
- `tests/fixtures/formatting/README.md`: its prose says 12 fixtures, its own table lists 13.
- `crates/outou-cli/README.md:35`: carries the same over-broad Cargo-matrix `cargo check` claim
  corrected in §4 of this document.
- `docs/design.md:86`: pre-existing competitor-comparison and project-continuation language.

## 10. Decision — the Phase 0 criteria are met

Applying `docs/phase0.md`'s criteria literally: all four prior gates passed (Gate 3 after two review
cycles, both documented rather than hidden), and every item under "what must not be cut" is
delivered — rust-analyzer integration (Gate 3), completion under incomplete/broken input
(`docs/gate3-results.md`'s M3/H1 fixes), multi-file module resolution (`outou-modules`, §3), `cargo
build` succeeding with no build script, no `OUT_DIR` and no extra Cargo flags or env vars, once
`src/.generated/` exists — written either by `outou build` or by `outou-lsp` on save (§4,
`docs/phase0/issues/08-cargo-build-determinism.md`) — diagnostic/definition mapping through source
maps (`outou-sourcemap`, §5), and Outou syntax diagnostics (§2). Every explicitly droppable item was
in fact dropped as designed, not half-built.

`docs/phase0.md`'s nine success criteria, walked against this report's evidence:

| # | Success criterion (`docs/phase0.md`) | Status | Evidence |
|---|---|---|---|
| 1 | Go to definition on `load_user()` | Met | `load_user()` -> `main.rsx:42:3-12` (§5) |
| 2 | The type of `user` on hover | Met | `let user: Option<User>`, exact range asserted (§5) |
| 3 | Rust completion after `user.` | Met | `completion (member, user.)`: 122 items (§5) |
| 4 | Component and prop completion | Met | Answered locally, not forwarded: `<UserC` -> exactly `UserCard`; `<UserCard us` -> exactly `user` (§5, M3) |
| 5 | Rust type errors reported at the right position in the `.rsx` file | **Met, only through flycheck, not instantly** | `didSave` -> flycheck maps back correctly (§5); native diagnostics never report semantic errors, confirmed even in plain non-macro code (`docs/ra-spike-results.md:59`, worded exactly this way) |
| 6 | JSX syntax errors reported by Outou itself | Met | `didChange` -> syntax diagnostic in ~100ms, no round trip (§5) |
| 7 | Completion that keeps working while the file is half-typed | Met | M3 (tag/attribute-name completion answered locally under an unclosed tag) and H1 (closing-tag completion fixed without corrupting the opening tag) |
| 8 | Go to definition across several `.rsx` files | Met | `UserCard` -> `components.rsx:30:7-15` (§5) |
| 9 | A build using `cargo build` and nothing else | **Met with a qualifier** | The workflow this report ships is `outou build && cargo build` (`docs/phase0/issues/08-cargo-build-determinism.md:15`; `examples/phase0-app/README.md`; `.github/workflows/ci.yml:80-95`) — `cargo build` alone does not build a cold checkout. With `outou-lsp` running, `didSave` writes generated Rust through the same `outou_cli::build::emit::emit` path (`crates/outou-lsp/src/dispatch/notifications.rs`), so once an editor session has saved once, in-editor `cargo build` alone does suffice; `outou build` is the no-LSP fallback |

Rows 5 and 9 carry the report's only two qualifiers; every other row is met outright. On that basis:
**the Phase 0 criteria, as stated in `docs/phase0.md` and demonstrated in this report, are met.**

A front-end alpha built on this result would carry the following conditions and risks, stated
plainly so they are not discovered later:

- **The runtime decision is still open**, to be made from the backend leakage ledger (§7, 19 of 28
  rows are Dioxus leakage) — the ledger documents props/`Element`/event-model constraints that are
  not Outou's own semantics and would need re-litigating before a 1.0 API is fixed.
- **Two rust-analyzer processes per workspace** run against the same crate whenever an editor also
  runs its own for `.rs` files (`crates/outou-lsp/README.md`, "Two processes, not one") — a real,
  measurable resource cost this report has not quantified. Qualitatively, it means a second full
  index of the same crate graph (memory and CPU roughly doubled during initial indexing); an alpha
  would want this measured directly, by sampling both processes' RSS and CPU time on a
  representative workspace under `outou-lsp` versus a single ordinary `rust-analyzer` session on the
  same code.
- **Semantic (type-mismatch) diagnostics are flycheck-only**, needing a save and several seconds'
  latency, because rust-analyzer's own native diagnostics never report them (confirmed general, not
  Outou-specific, by the Week 1 spike) — an alpha user will see a lag between typing a type error
  and seeing it that a `.rs` file in the same editor would not have.
- **No formatter, semantic tokens, rename, or publish automation** exist at all (§9); an alpha user
  gets a TextMate-free, rustfmt-free editing experience for `.rsx` and cannot `cargo publish` a
  library without first hand-rolling generation into the package.
- **The swallowed-tail parser recovery gap** (§2) means one shape of half-typed input (a truncated
  function signature) can silently eat following code in the editor overlay, recovered only when the
  user finishes or abandons that edit.

**What the fallback would have cost, had this been a NO-GO.** Issue #16 asks this to be recorded
regardless of the outcome. Falling back to `outou::jsx!` as an ordinary procedural macro (ADR 0001's
fallback, `docs/design.md` "If the bet fails") would have kept the same grammar and parser —
`outou-syntax` does not depend on `.rsx` being a standalone file format — but would have discarded
essentially everything that made this report's evidence nontrivial to produce: `outou-sourcemap`'s
many-to-many model exists because a standalone file needs its own diagnostics mapped back to itself,
which a macro gets for free from `proc_macro2::Span`; `outou-modules` exists only because rustc does
not know `.rsx` module files, which is moot inside an ordinary `.rs` file; the entire rust-analyzer
proxy in `outou-lsp` — two processes, editor overlays, position mapping, backend-vocabulary
sanitization on every payload — exists to solve a problem (getting IDE features for a file type
rustc's toolchain does not know) that a macro-based `outou::jsx!` does not have, since ordinary
`rust-analyzer` already expands and analyzes proc-macro input in place. Roughly: the parser and
grammar work (Weeks 2-3) would have carried over nearly unchanged; the source-map, module-resolver,
and language-server work (Weeks 4-5 — issues #05, #07 and #09) would not have been needed at all, at
the cost of the property Phase 0 exists to test: JSX as a first-class Rust expression rather than
macro input.
