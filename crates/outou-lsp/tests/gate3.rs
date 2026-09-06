//! Gate 3 (issue #9, `docs/phase0/issues/09-integrated-lsp.md`): "through
//! the real parser, completion / hover / definition / diagnostics do not
//! work for `let user = load_user(); <UserCard user={user} />` -> STOP."
//!
//! Drives the real `outou-lsp` binary against Gate 3's fixed target
//! program, `examples/phase0-app`, through
//! `spikes/rust-analyzer/client/outou-lsp-client.mjs` (a sibling of the
//! Week 1 spike's `ra-client.mjs`, reusing its framing/readiness design
//! but adapted to `outou-lsp`'s own protocol shape — see that file's own
//! doc comment for why it is a separate script rather than a `ra-client.mjs`
//! flag).
//!
//! Every probe runs against a **fresh temporary copy** of
//! `examples/phase0-app`, never the repository tree itself (issue #9 Gate
//! 3 review, M8(i)): several probes deliberately save broken content or
//! pre-corrupt the source before startup, and the repository's own
//! `examples/phase0-app` must never be left in that state, not even
//! transiently, regardless of how the test run ends.
//!
//! Ignored by default: the full run spawns `outou-lsp` (which itself
//! spawns rust-analyzer) once per probe, each waiting out rust-analyzer's
//! own indexing, well over the ~60s the repository's other tests expect
//! to finish in. Run explicitly:
//!
//! ```text
//! cargo test -p outou-lsp --test gate3 -- --ignored --nocapture
//! ```
//!
//! Skips (printing why, rather than failing) if `node` or a
//! `rust-analyzer` binary is not available, since neither is a Rust
//! toolchain component this repository can assume in every environment
//! that runs `cargo test --workspace`.
//!
//! Saving raw probe outputs to `spikes/rust-analyzer/results/gate3-*.json.gz`
//! (the evidence `docs/gate3-results.md`'s and
//! `docs/phase0/issues/09-integrated-lsp.md`'s latency tables are generated
//! from, via `spikes/rust-analyzer/client/gate3-latency-table.mjs`) is
//! **opt-in**: set `GATE3_SAVE_ARTIFACTS=1` to write there. Without it, this
//! run still executes every probe and every assertion exactly as before —
//! it only writes its JSON output to a temporary directory instead, so a
//! routine or CI run of this test never silently overwrites the checked-in
//! evidence a docs table was generated from.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// One probe per Gate 3 criterion, matching
/// `outou-lsp-client.mjs`'s `--probe` names.
const PROBES: &[&str] = &[
    "progress-before-hover",
    "hover-user",
    "hover-nonascii",
    "hover-element-tag",
    "hover-closing-tag",
    "hover-props-named-type",
    "definition-load-user",
    "definition-user-card",
    "completion-member",
    "completion-tag-component",
    "completion-tag-element",
    "completion-closing-tag",
    "completion-prop-name",
    "completion-attr-value",
    "completion-prop-value",
    "diagnostic-type-error",
    "diagnostic-syntax-error",
    "diagnostic-missing-prop",
    "stale-diagnostics-cleared",
    "save-with-syntax-error",
    "startup-broken-source",
];

/// Substrings that must never appear anywhere in a payload sent to the
/// editor (issue #9 Gate 3 review, M4): backend vocabulary
/// (`AGENTS.md`'s Phase 0 failure condition), a generated-file path, or
/// an internal marker this crate's own sanitizers are supposed to strip.
/// Deliberately broader than `crate::translate`'s own `BACKEND_MARKERS`
/// (which this test does not have access to, being a separate binary's
/// integration test) — false positives here would only ever make the
/// test stricter, never miss a real leak.
const LEAKAGE_MARKERS: &[&str] = &[
    "PropsBuilder",
    "dioxus",
    ".generated",
    "VNode",
    "RenderError",
    "__private",
    "__template",
    "rsx!",
    // H2 (issue #9 Gate 3 review): an HTML element's rustdoc "## Usage in
    // rsx" section named no `dioxus_*` path directly, so it survived
    // every marker above for a closing tag whose hover was not otherwise
    // classified locally.
    "Usage in rsx",
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root exists")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Resolves a rust-analyzer binary the same way `outou-lsp` itself does:
/// `OUTOU_RUST_ANALYZER` if set, otherwise `PATH`.
fn rust_analyzer_binary() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("OUTOU_RUST_ANALYZER") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join("rust-analyzer"))
        .find(|candidate| candidate.is_file())
}

/// Recursively copies `src` into `dst` (which must already exist),
/// skipping `target/` (a build-artifact directory that may exist locally
/// and would otherwise be copied wholesale for nothing).
fn copy_recursive(src: &Path, dst: &Path) {
    for entry in std::fs::read_dir(src).unwrap_or_else(|e| panic!("reading {}: {e}", src.display()))
    {
        let entry = entry.expect("reading a directory entry");
        let name = entry.file_name();
        if name == "target" {
            continue;
        }
        let file_type = entry.file_type().expect("reading file type");
        let dst_path = dst.join(&name);
        if file_type.is_dir() {
            std::fs::create_dir_all(&dst_path).expect("creating a subdirectory");
            copy_recursive(&entry.path(), &dst_path);
        } else {
            std::fs::copy(entry.path(), &dst_path)
                .unwrap_or_else(|e| panic!("copying {}: {e}", entry.path().display()));
        }
    }
}

static COPY_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Creates a fresh temporary copy of `examples/phase0-app`, with its
/// `Cargo.toml` path dependency on `outou` rewritten to an absolute path
/// (the checked-in `../../crates/outou` only resolves from
/// `examples/phase0-app`'s own location). Every probe gets its own copy:
/// M1's save-with-syntax-error and startup-broken-source probes
/// deliberately write broken content to disk, and must never be able to
/// affect another probe or the repository tree itself.
fn fresh_copy(root: &Path, tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let n = COPY_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dest = std::env::temp_dir().join(format!(
        "outou-gate3-{tag}-{}-{nanos}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dest).expect("creating the temp copy's root");
    // Canonicalized so the URI this test's node client builds from
    // `--root` (a plain string-to-URL conversion, no symlink resolution)
    // matches the URI `outou-lsp` itself builds after canonicalizing the
    // workspace root (`crate::plan::resolve`'s own `canonical_manifest_dir`)
    // — on macOS, `/tmp` is a symlink to `/var/folders/.../T`, so an
    // uncanonicalized temp path would otherwise make every `.rsx`
    // document URI the client sends look like a file this server has
    // never heard of.
    let dest = dest
        .canonicalize()
        .expect("canonicalizing the temp copy's root");
    copy_recursive(&root.join("examples/phase0-app"), &dest);

    let cargo_toml_path = dest.join("Cargo.toml");
    let cargo_toml = std::fs::read_to_string(&cargo_toml_path).expect("reading Cargo.toml");
    let outou_crate_dir = root
        .join("crates/outou")
        .canonicalize()
        .expect("crates/outou exists");
    let rewritten = cargo_toml.replace(
        "path = \"../../crates/outou\"",
        &format!("path = {:?}", outou_crate_dir),
    );
    assert_ne!(
        rewritten, cargo_toml,
        "expected to find the outou path dependency in examples/phase0-app/Cargo.toml"
    );
    std::fs::write(&cargo_toml_path, rewritten).expect("rewriting Cargo.toml");

    dest
}

/// Seeds `crate_dir`'s `src/.generated/` with a fresh, correct Strict
/// build — using `outou_cli::build::{plan, emit}` directly, the exact
/// same library `outou-lsp` itself calls ("there is one compiler",
/// `AGENTS.md`) — so every probe starts from known-good generated Rust
/// regardless of whatever this machine's own `examples/phase0-app/`
/// happens to have lying around locally (`.generated/` is gitignored for
/// applications, so a fresh checkout has none at all, and a local dev
/// checkout may have an arbitrarily stale one). `save-with-syntax-error`
/// in particular needs a reliable, known "before" snapshot to compare
/// against.
fn seed_build(crate_dir: &Path) {
    use outou_cli::build::{emit, plan};
    let canonical =
        plan::canonical_manifest_dir(crate_dir).expect("canonicalizing the temp crate dir");
    let root = match plan::find_crate_root(&canonical).expect("finding the temp crate's root") {
        plan::CrateRoot::Rsx(root) => root,
        plan::CrateRoot::NoRsxRoot => {
            panic!("expected examples/phase0-app to have a `.rsx` crate root")
        }
    };
    let planned = plan::plan(&canonical, &root).expect("planning the temp crate");
    emit::emit(&planned).expect("seeding the temp crate's generated Rust");
}

/// A `CARGO_TARGET_DIR` shared across every probe's own temp copy,
/// outside the repository tree entirely (never under `examples/
/// phase0-app` or any path `fresh_copy` produces): a fresh temp copy has
/// no build cache of its own, so without this, every probe that triggers
/// `cargo check` (rust-analyzer's `checkOnSave` flycheck) would
/// recompile the whole dependency graph (`dioxus` and everything under
/// it) from scratch — minutes, not seconds, and multiplied by every such
/// probe. A stable (non-randomized) path so repeated local runs keep
/// benefiting from the cache across invocations of this test, not just
/// within one.
fn shared_cargo_target_dir() -> PathBuf {
    std::env::temp_dir().join("outou-gate3-shared-target")
}

/// Runs one probe through `outou-lsp-client.mjs` against `crate_dir` and
/// returns its parsed JSON output.
fn run_probe(
    repo: &Path,
    crate_dir: &Path,
    outou_lsp: &Path,
    ra: &Path,
    probe: &str,
) -> serde_json::Value {
    let client = repo.join("spikes/rust-analyzer/client/outou-lsp-client.mjs");
    let output = Command::new("node")
        .arg(&client)
        .arg("--root")
        .arg(crate_dir)
        .arg("--outou-lsp")
        .arg(outou_lsp)
        .arg("--ra")
        .arg(ra)
        .arg("--probe")
        .arg(probe)
        .arg("--timeout")
        .arg("240000")
        .env("CARGO_TARGET_DIR", shared_cargo_target_dir())
        .output()
        .unwrap_or_else(|e| panic!("running outou-lsp-client.mjs for probe {probe}: {e}"));

    if !output.status.success() {
        panic!(
            "outou-lsp-client.mjs failed for probe {probe} (status {}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("parsing outou-lsp-client.mjs output for probe {probe}: {e}"))
}

/// The directory `save_gzipped` writes its artifacts to: the checked-in
/// `spikes/rust-analyzer/results/` only when the environment variable
/// `GATE3_SAVE_ARTIFACTS=1` is set (opt-in, since those files are the
/// evidence `docs/gate3-results.md`'s and
/// `docs/phase0/issues/09-integrated-lsp.md`'s latency tables are generated
/// from, and a routine test run must never silently overwrite them);
/// otherwise a directory under the system temp dir, so the probes and their
/// assertions still run exactly the same either way.
fn artifacts_dir(root: &Path) -> PathBuf {
    if std::env::var("GATE3_SAVE_ARTIFACTS").as_deref() == Ok("1") {
        root.join("spikes/rust-analyzer/results")
    } else {
        std::env::temp_dir().join("outou-gate3-results")
    }
}

/// Writes `json` gzip-compressed to `gate3-<probe>.json.gz` under
/// [`artifacts_dir`], matching the Gate 0 results' own convention
/// (`spikes/rust-analyzer/results/README.md`): shells out to the system
/// `gzip`, since this repository has no gzip crate dependency and the Gate 0
/// raw results were produced the same way.
fn save_gzipped(root: &Path, probe: &str, json: &serde_json::Value) {
    let results_dir = artifacts_dir(root);
    let pretty = serde_json::to_vec_pretty(json).expect("gate3 results always serialize");

    let tmp_path =
        std::env::temp_dir().join(format!("outou-gate3-{probe}-{}.json", std::process::id()));
    std::fs::write(&tmp_path, &pretty).expect("writing temp file before gzip");

    let status = Command::new("gzip")
        .arg("-9")
        .arg("-f")
        .arg(&tmp_path)
        .status()
        .expect("running gzip");
    assert!(status.success(), "gzip failed for probe {probe}");

    let gz_tmp = tmp_path.with_extension("json.gz");
    let dest = results_dir.join(format!("gate3-{probe}.json.gz"));
    std::fs::create_dir_all(&results_dir).expect("results dir exists");
    std::fs::rename(&gz_tmp, &dest)
        .unwrap_or_else(|e| panic!("moving {} to {}: {e}", gz_tmp.display(), dest.display()));
}

/// Finds the (0-based line, 0-based UTF-16 `character`) of the start of the
/// `occurrence`-th (0-indexed) match of `needle` in `text`, scanning line by
/// line and counting matches across the whole text — mirroring
/// `outou-lsp-client.mjs`'s own `positionOf` helper (same semantics: a
/// global occurrence count, not per-line).
///
/// Every hard-coded expected line/column in [`check_probe`] used to be a
/// literal computed once, by hand, against a specific revision of
/// `examples/phase0-app`; any later edit to that fixture (e.g. issue #10's
/// doc comment and `TagList`/`Field`/`MyProps`/`Card2` additions) silently
/// shifted every line after the edit and broke this gate for a reason
/// that had nothing to do with the LSP behavior under test. Deriving the
/// expected position from the fixture text itself, at test time, makes the
/// assertion track the fixture instead of a stale snapshot of it.
///
/// `character` is in UTF-16 code units (the LSP default position encoding,
/// and what `outou-lsp-client.mjs` assumes too), not bytes or Unicode
/// scalar values — the needles this test uses are all ASCII, so the three
/// encodings agree here, but computing it correctly costs nothing and
/// avoids a subtle trap for a future, non-ASCII needle.
fn position_of(text: &str, needle: &str, occurrence: usize) -> (i64, i64) {
    let mut seen = 0usize;
    for (line_no, line) in text.split('\n').enumerate() {
        for (byte_idx, _) in line.match_indices(needle) {
            if seen == occurrence {
                let utf16_col = line[..byte_idx].encode_utf16().count();
                return (line_no as i64, utf16_col as i64);
            }
            seen += 1;
        }
    }
    panic!("position_of: {needle:?} (occurrence {occurrence}) not found in text");
}

/// Recursively asserts that no string anywhere in `value` contains a
/// [`LEAKAGE_MARKERS`] substring (issue #9 Gate 3 review, M4/M8(iv)):
/// applied to every probe's *entire* JSON result, not just the field the
/// probe happens to be about, since a leak has shown up in `data`,
/// `relatedInformation`, `detail`, and `documentation` — fields no
/// existing assertion was looking at.
fn assert_no_leakage(probe: &str, value: &serde_json::Value) {
    walk_no_leakage(probe, "$", value);
}

fn walk_no_leakage(probe: &str, path: &str, value: &serde_json::Value) {
    match value {
        serde_json::Value::String(s) => {
            for marker in LEAKAGE_MARKERS {
                assert!(
                    !s.contains(marker),
                    "probe {probe}: leakage marker {marker:?} found at {path}: {s:?}"
                );
            }
        }
        serde_json::Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                walk_no_leakage(probe, &format!("{path}[{i}]"), item);
            }
        }
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                walk_no_leakage(probe, &format!("{path}.{k}"), v);
            }
        }
        _ => {}
    }
}

#[test]
#[ignore = "spawns outou-lsp + rust-analyzer once per probe; run explicitly, see module docs"]
fn gate3_probes_pass_through_the_real_parser_and_pipeline() {
    if !node_available() {
        eprintln!("gate3: skipping — `node` is not available on PATH");
        return;
    }
    let Some(ra) = rust_analyzer_binary() else {
        eprintln!(
            "gate3: skipping — no rust-analyzer binary found (checked OUTOU_RUST_ANALYZER and PATH)"
        );
        return;
    };

    let root = repo_root();
    let outou_lsp = PathBuf::from(env!("CARGO_BIN_EXE_outou-lsp"));

    // The canonical, unmodified fixture text — never a probe's own temp
    // copy, which some probes deliberately corrupt or rewrite on disk
    // before `check_probe` runs — that every hard-coded expected
    // line/column in `check_probe` is derived from via [`position_of`].
    let main_rsx_text = std::fs::read_to_string(root.join("examples/phase0-app/src/main.rsx"))
        .expect("reading examples/phase0-app/src/main.rsx");
    let components_rsx_text =
        std::fs::read_to_string(root.join("examples/phase0-app/src/components.rsx"))
            .expect("reading examples/phase0-app/src/components.rsx");

    let mut summary = String::new();
    let mut failures: Vec<String> = Vec::new();
    let mut temp_dirs: Vec<PathBuf> = Vec::new();

    for &probe in PROBES {
        eprintln!("gate3: running probe `{probe}`...");
        let crate_dir = fresh_copy(&root, probe);
        temp_dirs.push(crate_dir.clone());
        seed_build(&crate_dir);

        if probe == "startup-broken-source" {
            prepare_startup_broken_source(&crate_dir);
        }

        let generated_root_before = crate_dir.join("src/.generated/crate-root.rs");
        let before_bytes = std::fs::read(&generated_root_before).ok();

        let result = run_probe(&root, &crate_dir, &outou_lsp, &ra, probe);
        save_gzipped(&root, probe, &result);
        assert_no_leakage(probe, &result);

        if let Err(message) = check_probe(
            probe,
            &result,
            &crate_dir,
            before_bytes.as_deref(),
            &main_rsx_text,
            &components_rsx_text,
        ) {
            failures.push(format!("{probe}: {message}"));
        }

        writeln!(
            summary,
            "{probe}: latencyMs={}",
            serde_json::to_string(result.get("latencyMs").unwrap_or(&serde_json::Value::Null))
                .unwrap_or_default()
        )
        .unwrap();
    }

    println!("Gate 3 probe summary:\n{summary}");

    for dir in &temp_dirs {
        let _ = std::fs::remove_dir_all(dir);
    }

    assert!(
        failures.is_empty(),
        "Gate 3 probe(s) failed:\n{}",
        failures.join("\n")
    );
}

/// M1's startup probe needs a broken `.rsx` root already on disk, with no
/// `.generated/` at all, *before* `outou-lsp` is ever spawned.
fn prepare_startup_broken_source(crate_dir: &Path) {
    let main_rsx = crate_dir.join("src/main.rsx");
    let text = std::fs::read_to_string(&main_rsx).expect("reading main.rsx");
    let broken = text.replace("<h1>Hello {name}</h1>", "<div cl");
    assert_ne!(broken, text, "expected to find the target element to break");
    std::fs::write(&main_rsx, broken).expect("writing the broken main.rsx");
    let generated_dir = crate_dir.join("src/.generated");
    if generated_dir.exists() {
        std::fs::remove_dir_all(&generated_dir).expect("removing .generated for the startup probe");
    }
}

/// Checks the one thing that matters for each probe's Gate 3 criterion,
/// returning `Err` with a short reason on failure rather than panicking
/// immediately, so a single run reports every failing probe at once.
fn check_probe(
    probe: &str,
    result: &serde_json::Value,
    crate_dir: &Path,
    generated_root_before: Option<&[u8]>,
    main_rsx_text: &str,
    components_rsx_text: &str,
) -> Result<(), String> {
    match probe {
        "progress-before-hover" => {
            // S6/L12 (issue #9 Gate 3 review): `outou-lsp` now forwards
            // rust-analyzer's `$/progress` to a client that advertised
            // `window.workDoneProgress` support; the probe client records
            // whether it actually saw a `$/progress` `end` notification
            // before its first hover request went out.
            let saw_progress_end = result
                .get("sawProgressEndBeforeFirstHover")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !saw_progress_end {
                return Err(
                    "no $/progress `end` notification was forwarded before the first hover (S6/L12)"
                        .to_string(),
                );
            }
            let value = result
                .pointer("/hover/contents/value")
                .and_then(|v| v.as_str())
                .ok_or("no hover contents")?;
            if !value.contains("Option") {
                return Err(format!("hover did not mention `Option`: {value}"));
            }
            Ok(())
        }
        "hover-user" => {
            let value = result
                .pointer("/hover/contents/value")
                .and_then(|v| v.as_str())
                .ok_or("no hover contents")?;
            if !value.contains("Option") {
                return Err(format!("hover did not mention `Option`: {value}"));
            }
            let range = result.pointer("/hover/range").ok_or("no hover range")?;
            let (line, _) = position_of(main_rsx_text, "let user = load_user();", 0);
            require_range_on_line(range, line)
        }
        "hover-nonascii" => {
            let value = result
                .pointer("/hover/contents/value")
                .and_then(|v| v.as_str())
                .ok_or("no hover contents for the non-ASCII probe")?;
            if !value.contains("Option") && !value.contains("User") {
                return Err(format!(
                    "hover after a non-ASCII prefix did not resolve `user`'s own type: {value}"
                ));
            }
            Ok(())
        }
        "hover-element-tag" => {
            let has_hover = result.pointer("/hover").is_some_and(|h| !h.is_null());
            // `assert_no_leakage` already rejects a `dioxus`/`rsx!`
            // mention anywhere in the result; this only confirms there
            // was something to sanitize in the first place; a `null`
            // hover (nothing left after sanitizing) is also acceptable.
            let _ = has_hover;
            Ok(())
        }
        "hover-closing-tag" => {
            // H2: a closing tag's own name is classified locally, exactly
            // like the opening tag's (`crate::complete::is_tag_name_position`),
            // and answered `null` directly — never forwarded to
            // rust-analyzer at all, so the result must be exactly `null`,
            // not merely leakage-free.
            let hover = result
                .get("hover")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            if !hover.is_null() {
                return Err(format!(
                    "hover on a closing tag name must be exactly null (answered locally): {hover}"
                ));
            }
            Ok(())
        }
        "hover-props-named-type" => {
            // H3: `MyProps` is an ordinary user struct, not backend
            // vocabulary, so hover on the `config` parameter must resolve
            // normally (mentioning `MyProps`, at the parameter's own
            // exact range) rather than come back `null`/fences-only.
            let value = result
                .pointer("/hover/contents/value")
                .and_then(|v| v.as_str())
                .ok_or("no hover contents for the `MyProps`-named-type probe")?;
            if !value.contains("MyProps") {
                return Err(format!(
                    "hover on `config: MyProps` did not mention `MyProps`: {value}"
                ));
            }
            let range = result
                .pointer("/hover/range")
                .ok_or("no hover range for the `MyProps`-named-type probe")?;
            // The hovered position sits inside the `config` parameter name
            // (`outou-lsp-client.mjs`'s own probe hovers one character into
            // it); the range rust-analyzer returns is that parameter
            // identifier's own span, not the `MyProps` type it names.
            let (line, start) = position_of(components_rsx_text, "config: MyProps", 0);
            let end = start + "config".len() as i64;
            require_range_matches(range, line, start, line, end)
        }
        "definition-load-user" => {
            let uri = result
                .pointer("/definition/0/uri")
                .and_then(|v| v.as_str())
                .ok_or("no definition location")?;
            if !uri.ends_with("main.rsx") {
                return Err(format!("definition did not land in main.rsx: {uri}"));
            }
            let range = result
                .pointer("/definition/0/range")
                .ok_or("no definition range")?;
            let (line, _) = position_of(main_rsx_text, "fn load_user(", 0);
            require_range_on_line(range, line)
        }
        "definition-user-card" => {
            let uri = result
                .pointer("/definition/0/uri")
                .and_then(|v| v.as_str())
                .ok_or("no cross-file definition location")?;
            if !uri.ends_with("components.rsx") {
                return Err(format!(
                    "cross-file definition did not land in components.rsx: {uri}"
                ));
            }
            let range = result
                .pointer("/definition/0/range")
                .ok_or("no cross-file definition range")?;
            let (line, _) = position_of(components_rsx_text, "pub fn UserCard(", 0);
            require_range_on_line(range, line)
        }
        "completion-member" => {
            let item = find_completion_item(result, "unwrap").ok_or("no `unwrap` item")?;
            require_edit_range_present(item)
        }
        "completion-tag-component" => {
            let item = find_completion_item(result, "UserCard").ok_or("no `UserCard` item")?;
            require_edit_range_matches(item, 9, 14)
        }
        "completion-tag-element" => {
            let item = find_completion_item(result, "div").ok_or("no `div` item")?;
            require_edit_range_matches(item, 9, 11)
        }
        "completion-closing-tag" => {
            // H1: answered locally (never forwarded), so the closing
            // `</p>` tag offers HTML element names starting with "p"
            // (`p`, `pre`), and — the actual regression this probe
            // guards — the edit range sits on the *closing* tag's own
            // "p" (derived from `</p>`'s position below), never the
            // opening tag's position the old reverse-mapping bug always
            // produced.
            let item = find_completion_item(result, "p")
                .ok_or("no `p` item for the closing tag completion")?;
            let range = item
                .pointer("/textEdit/range")
                .ok_or_else(|| format!("item has no textEdit.range: {item}"))?;
            // `</p>`'s own "p": two characters in from the needle's start
            // (past `</`), one character wide.
            let (line, tag_start) = position_of(main_rsx_text, "</p>", 0);
            let start = tag_start + 2;
            require_range_matches(range, line, start, line, start + 1)
        }
        "completion-prop-name" => {
            let item = find_completion_item(result, "user").ok_or("no `user` item")?;
            require_edit_range_present(item)
        }
        "completion-attr-value" => {
            // Empty, or every primary edit starts at the cursor — either
            // is honest; a wrongly-positioned edit is not (M4/HIGH-8).
            let items = completion_items(result);
            for item in &items {
                let Some(text_edit) = item.get("textEdit") else {
                    continue;
                };
                let Some(range) = text_edit.get("range") else {
                    continue;
                };
                let start = range.pointer("/start").ok_or("edit with no start")?;
                let end = range.pointer("/end").ok_or("edit with no end")?;
                if start != end {
                    return Err(format!(
                        "completion-attr-value returned a non-empty edit: {text_edit}"
                    ));
                }
            }
            Ok(())
        }
        "completion-prop-value" => find_completion_item(result, "user")
            .map(|_| ())
            .ok_or_else(|| "prop-value completion did not include `user`".to_string()),
        "diagnostic-type-error" => {
            let has_type_error =
                all_diagnostics(result).any(|diag| message_contains(diag, "mismatched types"));
            has_type_error
                .then_some(())
                .ok_or_else(|| "no mismatched-types diagnostic was published".to_string())
        }
        "diagnostic-syntax-error" => {
            let has_outou_diag = all_diagnostics(result)
                .any(|diag| diag.get("source").and_then(|s| s.as_str()) == Some("outou"));
            if !has_outou_diag {
                return Err("no Outou syntax diagnostic was published".to_string());
            }
            let definition_still_works = result
                .pointer("/definitionDespiteSyntaxError/0/uri")
                .and_then(|v| v.as_str())
                .is_some_and(|u| u.ends_with("main.rsx"));
            if !definition_still_works {
                return Err(
                    "definition elsewhere in the file stopped working after the syntax error"
                        .to_string(),
                );
            }
            Ok(())
        }
        "diagnostic-missing-prop" => {
            let diag = all_diagnostics(result)
                .find(|d| message_contains(d, "missing a required property"))
                .ok_or("no missing-required-property diagnostic was published")?;
            let severity = diag.get("severity").and_then(|s| s.as_i64());
            if severity != Some(1) {
                return Err(format!(
                    "M5: a missing required prop must be severity ERROR (1), got {severity:?}"
                ));
            }
            let range = diag
                .get("range")
                .ok_or("missing-prop diagnostic has no range")?;
            let (line, _) = position_of(main_rsx_text, "<UserCard user={user.unwrap()} />", 0);
            require_range_on_line(range, line)
        }
        "stale-diagnostics-cleared" => {
            let introduced = result
                .get("typeErrorIntroduced")
                .filter(|v| !v.is_null())
                .ok_or("the type error was never even published before the revert")?;
            let _ = introduced;
            let after = result
                .get("diagnosticsAfterRevert")
                .and_then(|v| v.as_array())
                .ok_or("no diagnosticsAfterRevert recorded")?;
            let still_has_stale_error = after
                .iter()
                .any(|d| message_contains(d, "mismatched types"));
            if still_has_stale_error {
                return Err(
                    "the stale `mismatched types` diagnostic survived a revert-only edit (M6)"
                        .to_string(),
                );
            }
            Ok(())
        }
        "save-with-syntax-error" => {
            let has_outou_diag = all_diagnostics(result)
                .any(|diag| diag.get("source").and_then(|s| s.as_str()) == Some("outou"));
            if !has_outou_diag {
                return Err(
                    "no Outou syntax diagnostic was published for the broken save".to_string(),
                );
            }
            let before = generated_root_before
                .ok_or("no pre-save snapshot of crate-root.rs was captured")?;
            let after = std::fs::read(crate_dir.join("src/.generated/crate-root.rs"))
                .map_err(|e| format!("reading crate-root.rs after the save: {e}"))?;
            if before != after.as_slice() {
                return Err(
                    "M1: crate-root.rs changed on disk after saving a syntactically broken buffer"
                        .to_string(),
                );
            }
            let after_text = String::from_utf8_lossy(&after);
            if after_text.contains("cl: true") {
                return Err(
                    "M1: crate-root.rs contains Recovery placeholder text (`cl: true`)".to_string(),
                );
            }
            Ok(())
        }
        "startup-broken-source" => {
            let has_outou_diag = all_diagnostics(result)
                .any(|diag| diag.get("source").and_then(|s| s.as_str()) == Some("outou"));
            if !has_outou_diag {
                return Err("no Outou syntax diagnostic was published at startup".to_string());
            }
            let generated_root = crate_dir.join("src/.generated/crate-root.rs");
            if generated_root.exists() {
                return Err(
                    "M1: crate-root.rs was created at startup despite a broken crate root"
                        .to_string(),
                );
            }
            Ok(())
        }
        other => Err(format!("unknown probe: {other}")),
    }
}

fn all_diagnostics(result: &serde_json::Value) -> impl Iterator<Item = &serde_json::Value> {
    result
        .get("diagnostics")
        .and_then(|d| d.as_object())
        .into_iter()
        .flat_map(|d| d.values())
        .flat_map(|v| v.as_array().into_iter().flatten())
}

fn message_contains(diagnostic: &serde_json::Value, needle: &str) -> bool {
    diagnostic
        .get("message")
        .and_then(|m| m.as_str())
        .is_some_and(|m| m.contains(needle))
}

fn completion_items(result: &serde_json::Value) -> Vec<serde_json::Value> {
    let completion = result.get("completion");
    let items = completion.and_then(|c| c.as_array()).or_else(|| {
        completion
            .and_then(|c| c.get("items"))
            .and_then(|i| i.as_array())
    });
    items.cloned().unwrap_or_default()
}

fn find_completion_item(result: &serde_json::Value, label: &str) -> Option<serde_json::Value> {
    completion_items(result)
        .into_iter()
        .find(|item| item.get("label").and_then(|l| l.as_str()) == Some(label))
}

fn require_edit_range_present(item: serde_json::Value) -> Result<(), String> {
    item.pointer("/textEdit/range")
        .map(|_| ())
        .ok_or_else(|| format!("item has no textEdit.range: {item}"))
}

/// Asserts the item's `textEdit.range` is exactly
/// `{line}:{start_char}..{line}:{end_char}` — the partial identifier's
/// own span, per M3's design (never a generated-file-derived guess).
fn require_edit_range_matches(
    item: serde_json::Value,
    start_char: i64,
    end_char: i64,
) -> Result<(), String> {
    let range = item
        .pointer("/textEdit/range")
        .ok_or_else(|| format!("item has no textEdit.range: {item}"))?;
    let start = range
        .pointer("/start/character")
        .and_then(|v| v.as_i64())
        .ok_or("range has no start.character")?;
    let end = range
        .pointer("/end/character")
        .and_then(|v| v.as_i64())
        .ok_or("range has no end.character")?;
    if start != start_char || end != end_char {
        return Err(format!(
            "expected textEdit.range character {start_char}..{end_char}, got {start}..{end} ({range})"
        ));
    }
    Ok(())
}

/// Asserts `range.start.line == range.end.line == line` (0-indexed).
fn require_range_on_line(range: &serde_json::Value, line: i64) -> Result<(), String> {
    let start_line = range
        .pointer("/start/line")
        .and_then(|v| v.as_i64())
        .ok_or("range has no start.line")?;
    let end_line = range
        .pointer("/end/line")
        .and_then(|v| v.as_i64())
        .ok_or("range has no end.line")?;
    if start_line != line || end_line != line {
        return Err(format!(
            "expected range on line {line}, got {start_line}..{end_line}"
        ));
    }
    Ok(())
}

/// Asserts a range is exactly `{start_line}:{start_char}..{end_line}:{end_char}`
/// (0-indexed) — stricter than [`require_range_on_line`] (which only
/// checks the line) or [`require_edit_range_matches`] (which only checks
/// the character columns), used where a probe's whole point is that the
/// range landed at one *specific* line and column, not merely somewhere
/// on the right line.
fn require_range_matches(
    range: &serde_json::Value,
    start_line: i64,
    start_char: i64,
    end_line: i64,
    end_char: i64,
) -> Result<(), String> {
    let got_start_line = range
        .pointer("/start/line")
        .and_then(|v| v.as_i64())
        .ok_or("range has no start.line")?;
    let got_start_char = range
        .pointer("/start/character")
        .and_then(|v| v.as_i64())
        .ok_or("range has no start.character")?;
    let got_end_line = range
        .pointer("/end/line")
        .and_then(|v| v.as_i64())
        .ok_or("range has no end.line")?;
    let got_end_char = range
        .pointer("/end/character")
        .and_then(|v| v.as_i64())
        .ok_or("range has no end.character")?;
    if (got_start_line, got_start_char, got_end_line, got_end_char)
        != (start_line, start_char, end_line, end_char)
    {
        return Err(format!(
            "expected range {start_line}:{start_char}..{end_line}:{end_char}, got {got_start_line}:{got_start_char}..{got_end_line}:{got_end_char}"
        ));
    }
    Ok(())
}
