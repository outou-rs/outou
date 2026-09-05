# Raw results, Week 1 rust-analyzer spike

Layout (a) (`OUT_DIR` + `include!`) was rejected and its fixture code (`build.rs`, the `gen-outdir` feature, `virtual/App.rs`) was removed after this spike, per [ADR 0009](../../../docs/adr/0009-generated-source-location.md). References below to those files, to `$OUT_DIR`, or to `virtual/App.rs` describe the fixture as it existed at commit `9cde9a9`, before that removal; they are not reproducible against the current fixture.

Every file here is the unmodified JSON printed by `ra-client.mjs` (stdout only;
its `console.error`/`WARN notify` lines were redirected away, not edited) for
one probe. Findings drawn from these files are written up in
[`docs/ra-spike-results.md`](../../../docs/ra-spike-results.md).

Raw outputs are gzip-compressed (`gzip -9`) to keep the repository small; the
`source-map-a-adjusted.json` and `.txt` files are kept readable as-is. To
inspect a compressed file: `gzcat b-hover-user-cold.json.gz | jq .` (macOS) /
`zcat b-hover-user-cold.json.gz | jq .` (Linux).

All commands were run from `spikes/rust-analyzer/fixture` with rust-analyzer
1.98.1, rustc/cargo 1.98.1 (see the Environment table in
`docs/ra-spike-results.md`). Overlay files referenced below were built from
`fixture/src/.generated/App.rs` (layout b) and `virtual/App.rs` (layout a,
copied into `OUT_DIR/outou/App.rs` by `build.rs`); their content is described
inline. `$OUT_DIR` is the path printed by:

```bash
cargo check --no-default-features --features gen-outdir --message-format=json \
  | jq -r 'select(.reason=="build-script-executed" and (.package_id | contains("ra-spike-fixture"))) | .out_dir'
```

Note the `package_id` filter: `cargo check --message-format=json` reports a
`build-script-executed` event for every dependency with a build script (this
fixture pulls in ~13), so the plain filter from the README/issue text
(`select(.reason=="build-script-executed")`) returns 13 lines; only the one
whose `package_id` contains `ra-spike-fixture` is ours.

**Line-number caveat for layout (a):** `source-map.json` and the README's
"positions to probe" table were written against `fixture/src/.generated/App.rs`
(layout b). `virtual/App.rs` (what `build.rs` copies into `OUT_DIR` for layout
a) is missing that file's `use super::*;` line and the blank line after it
(both files have the same three-line header comment), so every
generated-line position is offset by **-2** for layout (a): `let user = ...`
is at line 6 there, not line 8; `UserCard {` is at line 9, not 11; `user:
user` is at line 10, not 12. The `a-*` commands below use the corrected
offsets. `character` offsets are unaffected (same line content).

## Layout (b): `src/.generated/App.rs`

| File | Command |
|---|---|
| `b-hover-user-cold.json.gz` / `-warm.json.gz` | `node ../client/ra-client.mjs --root . --file src/.generated/App.rs --line 8 --char 9 --timeout 180000` (run twice) |
| `b-definition-load_user-cold.json.gz` / `-warm.json.gz` | `node ../client/ra-client.mjs --root . --file src/.generated/App.rs --line 8 --char 17 --timeout 180000` (run twice) |
| `b-completion-member-cold.json.gz` / `-warm.json.gz` | `node ../client/ra-client.mjs --root . --file src/.generated/App.rs --overlay <member-b.rs> --line 9 --char 9 --timeout 180000` — `member-b.rs` is the generated file with a new line `    user.` inserted right after the `let user = load_user();` line |
| `b-completion-component-cold.json.gz` / `-warm.json.gz` | `node ../client/ra-client.mjs --root . --file src/.generated/App.rs --line 11 --char 9 --timeout 180000` (plain, no overlay) |
| `b-completion-prop-cold.json.gz` / `-warm.json.gz` | `node ../client/ra-client.mjs --root . --file src/.generated/App.rs --line 12 --char 13 --timeout 180000` (plain) |
| `b-overlay-change-cold.json.gz` / `-warm.json.gz` | `node ../client/ra-client.mjs --root . --file src/.generated/App.rs --line 8 --char 9 --overlay2 <overlay2-b.rs> --line2 8 --char2 9 --timeout 180000` — `overlay2-b.rs` changes `let user = load_user();` to `let user = load_user().name;` |
| `b-diagnostics-native-typeerror.json.gz` | `node ../client/ra-client.mjs --root . --file src/.generated/App.rs --overlay <typeerr-b.rs> --line 8 --char 9 --source-map ../source-map.json --settle 20000 --timeout 180000` — `typeerr-b.rs` changes the `let` to `let user: u32 = load_user();` |
| `b-diagnostics-native-syntaxerror.json.gz` | Same, with `syntaxerr-b.rs` (removes the semicolon from the `let` line) |
| `b-diagnostics-flycheck-typeerror.json.gz` | `typeerr-b.rs` content was copied onto disk at `src/.generated/App.rs`, then: `node ../client/ra-client.mjs --root . --file src/.generated/App.rs --line 8 --char 9 --source-map ../source-map.json --settle 30000 --timeout 180000`; the file was restored afterwards with `git checkout -- src/.generated/App.rs` |
| `b-definition-latest-buffer-cold.json.gz` / `-warm.json.gz` | `node ../client/ra-client.mjs --root . --file src/.generated/App.rs --overlay ../results/overlays/defmove-1-b.rs --line 9 --char 18 --overlay2 ../results/overlays/defmove-2-b.rs --line2 10 --char2 18 --settle 500 --timeout 180000` (run twice) — `defmove-1-b.rs` adds a use-site `let _probe = user.age;` after the `let user = ...` line; `defmove-2-b.rs` additionally inserts a comment line above the declaration, shifting it down by one line. Criterion-6 evidence: `definition` (before) 8:8–8:12, `definition2` (after) 9:8–9:12 — the use-site definition tracks the edited buffer. |

## Layout (a): `$OUT_DIR/outou/App.rs`

All commands add `--no-default-features --cargo-features gen-outdir`. `$OUT_DIR`
is obtained as shown at the top of this file; each command below assumes it is
already exported in the shell.

| File | Command |
|---|---|
| `a-hover-user-cold.json.gz` / `-warm.json.gz` | `node ../client/ra-client.mjs --root . --no-default-features --cargo-features gen-outdir --file "$OUT_DIR/outou/App.rs" --line 6 --char 9 --timeout 180000` (run twice) |
| `a-definition-load_user-cold.json.gz` / `-warm.json.gz` | `node ../client/ra-client.mjs --root . --no-default-features --cargo-features gen-outdir --file "$OUT_DIR/outou/App.rs" --line 6 --char 17 --timeout 180000` (run twice) |
| `a-completion-member-cold.json.gz` / `-warm.json.gz` | `node ../client/ra-client.mjs --root . --no-default-features --cargo-features gen-outdir --file "$OUT_DIR/outou/App.rs" --overlay <member-a.rs> --line 7 --char 9 --timeout 180000` — `member-a.rs` is `virtual/App.rs` with `    user.` inserted after the `let` line; overlay not retained (reconstructable from `virtual/App.rs` by the edit described) |
| `a-completion-component-cold.json.gz` / `-warm.json.gz` | `node ../client/ra-client.mjs --root . --no-default-features --cargo-features gen-outdir --file "$OUT_DIR/outou/App.rs" --line 9 --char 9 --timeout 180000` (plain) |
| `a-completion-prop-cold.json.gz` / `-warm.json.gz` | `node ../client/ra-client.mjs --root . --no-default-features --cargo-features gen-outdir --file "$OUT_DIR/outou/App.rs" --line 10 --char 13 --timeout 180000` (plain) |
| `a-overlay-change-cold.json.gz` / `-warm.json.gz` | `node ../client/ra-client.mjs --root . --no-default-features --cargo-features gen-outdir --file "$OUT_DIR/outou/App.rs" --line 6 --char 9 --overlay2 ../results/overlays/overlay2-a.rs --line2 6 --char2 9 --timeout 180000` — `overlay2-a.rs` changes `let user = load_user();` to `let user = load_user().name;` (same `.name` edit as layout (b), applied to `virtual/App.rs`'s line offsets); retained at `results/overlays/overlay2-a.rs` |
| `a-diagnostics-native-typeerror.json.gz` | `node ../client/ra-client.mjs --root . --no-default-features --cargo-features gen-outdir --file "$OUT_DIR/outou/App.rs" --overlay <typeerr-a.rs> --line 6 --char 9 --source-map ../source-map.json --settle 20000 --timeout 180000` (uses the layout-b source map as-is, on purpose — see notes); overlay not retained (reconstructable: change the `let` line in `virtual/App.rs` to `let user: u32 = load_user();`) |
| `a-diagnostics-native-syntaxerror.json.gz` | Same, with `syntaxerr-a.rs` (removes the semicolon from the `let` line), layout-b source map; overlay not retained (reconstructable as described) |
| `a-diagnostics-native-syntaxerror-adjusted-sourcemap.json.gz` | Same overlay, but `--source-map ../results/source-map-a-adjusted.json`, a copy of `source-map.json` with every `generated` line shifted by -2 to match layout (a)'s actual offsets — built only to prove the mapping *mechanism* is layout-agnostic once the map itself accounts for the offset; retained at `results/source-map-a-adjusted.json` |
| `a-diagnostics-flycheck-typeerror.json.gz` | `typeerr-a.rs` content copied onto `virtual/App.rs`, then `cargo check --no-default-features --features gen-outdir` re-run to refresh `$OUT_DIR`, then `node ../client/ra-client.mjs --root . --no-default-features --cargo-features gen-outdir --file "$OUT_DIR/outou/App.rs" --line 6 --char 9 --source-map ../results/source-map-a-adjusted.json --settle 30000 --timeout 180000`; `virtual/App.rs` restored with `git checkout` and `$OUT_DIR` refreshed again with a clean `cargo check` afterwards |
| `a-definition-latest-buffer-cold.json.gz` / `-warm.json.gz` | `node ../client/ra-client.mjs --root . --no-default-features --cargo-features gen-outdir --file "$OUT_DIR/outou/App.rs" --overlay ../results/overlays/defmove-1-a.rs --line 7 --char 18 --overlay2 ../results/overlays/defmove-2-a.rs --line2 8 --char2 18 --settle 500 --timeout 180000` (run twice) — same probe as `b-definition-latest-buffer-*`, applied to `virtual/App.rs`'s line offsets. Criterion-6 evidence: `definition` (before) 6:8–6:12, `definition2` (after) 7:8–7:12. |
| `a-overlay-change-mtimes.txt` (+ `-run.json.gz`) | Full shell transcript for criterion 3: records `stat`/`shasum` of `$OUT_DIR/outou/App.rs` and `target/debug/build/<pkg>/output` before and after a `node ../client/ra-client.mjs` run with `--overlay2 ../results/overlays/overlay2-a.rs`, staying in the `gen-outdir` feature state throughout (no intervening default-feature `cargo check`). `-run.json.gz` is that run's raw client output; `.txt` is the full `tee`d transcript including the `stat`/`shasum` output before and after. Result: mtimes and sha256 identical before/after; `hover2` reports `String`. |

## Supplementary (not part of the required seven-criteria matrix)

| File | Purpose |
|---|---|
| `supplementary-diagnostics-nonmacro-typeerror.json.gz` | Same `let x: u32 = load_user();` type error, overlaid onto **`src/main.rs`** (`fn main`, no `#[component]` macro involved) to check whether the missing native type-mismatch diagnostic is specific to macro-generated code. Command: `--file src/main.rs --overlay <main-typeerr.rs> --line 42 --char 9 --settle 20000`. Result: still no native diagnostic — the limitation is general, not `#[component]`-specific. |
| `supplementary-diagnostics-nonmacro-badimport.json.gz` | Same check with an unresolved `use` path in `src/main.rs` (`--line 0 --char 0 --settle 15000`). No native diagnostic either. |
| `supplementary-flycheck-timing-bracket-b-settle500.json.gz` | Layout (b), type-error on disk, `--settle 500`: only 2 of the eventual 4 diagnostics for the generated file had arrived. |
| `supplementary-flycheck-timing-bracket-b-settle3000.json.gz` | Same, `--settle 3000`: all 4 diagnostics (plus the related one on `src/main.rs`) present. Used to bracket the flycheck latency reported in `docs/ra-spike-results.md`. |
| `supplementary-flycheck-timing-bracket-a-settle3000.json.gz` | Same bracket for layout (a). |

`ra-client.mjs` has no dedicated "time until flycheck diagnostics arrived"
field: it always sleeps the full `--settle` duration before checking, rather
than returning as soon as a diagnostic lands. The bracket files above were
used to approximate that latency instead of inventing a number.

## Environment and other artifacts

| File | Purpose |
|---|---|
| `environment-and-cargo.txt` | Transcript of `date`, `rustc -V`, `cargo -V`, `rust-analyzer --version`, `node -v`, `sw_vers`, `uname -m`, a timed default-feature `cargo check` (warm cache), and the full, unfiltered list of `package_id`s from `cargo check --no-default-features --features gen-outdir --message-format=json \| jq -r 'select(.reason=="build-script-executed") \| .package_id'` (13 rows total: 12 dependency build scripts plus the fixture's own). Backs the `cargo check` timing and the "13 rows" claim in `docs/ra-spike-results.md`. |
| `source-map-a-adjusted.json` | Derived copy of `../source-map.json` with every mapping's `generated.start.line`/`generated.end.line` shifted by -2 (`jq '.mappings \|= map(.generated.start.line -= 2 \| .generated.end.line -= 2)'`), to match layout (a)'s -2 line offset from the caveat above. Not an independent input — it is mechanically derived from the committed `source-map.json` and exists only so `--source-map` can be pointed at layout (a)'s actual offsets. |
| `overlays/` | Overlay buffers built from `fixture/src/.generated/App.rs` (`*-b.rs`) and `virtual/App.rs` (`*-a.rs`): `defmove-1-{a,b}.rs` add a use-site `let _probe = user.age;` after the `let user = ...` line; `defmove-2-{a,b}.rs` additionally insert a comment line above the declaration (used as the `--overlay`/`--overlay2` pair for the `*-definition-latest-buffer-*` probes); `overlay2-a.rs` changes `load_user();` to `load_user().name;` (layout (a) counterpart of the pre-existing, not-retained layout (b) overlay used for `b-overlay-change-*`). |
