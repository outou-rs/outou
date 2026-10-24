//! A1 measurement (issue #17, `docs/alpha/issues/17-measurement.md`;
//! method in `docs/alpha.md`, "A1: measurement, in detail"). Orchestrates
//! two kinds of measurement:
//!
//! - **Pure Rust, cheap:** the incremental `.rsx` -> generated Rust
//!   distribution ([`incremental_regeneration_distribution_on_both_workspaces`]),
//!   calling [`outou_cli::build::emit::generate_unit`] directly. That is the
//!   unit-generation step of `outou-lsp`'s `Workspace::regenerate`, not all
//!   of it (`regenerate` also re-parses for `declared_modules`, builds a
//!   `LineIndex` and updates the registry; see the doc comment of
//!   `regenerating_one_unit_is_fast` in
//!   `crates/outou-lsp/src/documents/workspace/tests.rs`). Both of A1's
//!   fixed workspaces (`examples/phase0-app`, `tests/fixtures/workspace`).
//!   No rust-analyzer involved; runs under the default `cargo test
//!   --workspace` budget.
//! - **Heavy, `#[ignore]`d:** cold init, flycheck latency, forwarded-
//!   completion overhead and RSS/CPU sampling, each driving
//!   `spikes/rust-analyzer/client/measure-a1.mjs` against a fresh
//!   temporary copy of a fixture ([`fresh_copy`]/[`seed_build`], mirroring
//!   `tests/gate3.rs`'s own helpers rather than sharing them:
//!   `tests/gate3.rs` also deliberately corrupts its copies mid-probe, and
//!   factoring that specific pairing out into a shared module was judged
//!   not worth the coupling for a handful of duplicated helper functions).
//!   Run explicitly, one at a time (never alongside `cargo clippy`/`cargo
//!   test`, and never two of these at once; CPU contention skews every
//!   number):
//!
//! ```text
//! cargo test -p outou-lsp --test measure_a1 -- --ignored --nocapture measure_a1_coldinit
//! cargo test -p outou-lsp --test measure_a1 -- --ignored --nocapture measure_a1_flycheck
//! cargo test -p outou-lsp --test measure_a1 -- --ignored --nocapture measure_a1_completion_overhead
//! cargo test -p outou-lsp --test measure_a1 -- --ignored --nocapture measure_a1_completion_stall
//! cargo test -p outou-lsp --test measure_a1 -- --ignored --nocapture measure_a1_rss_single
//! cargo test -p outou-lsp --test measure_a1 -- --ignored --nocapture measure_a1_rss_two_process
//! ```
//!
//! **Build profile.** `CARGO_BIN_EXE_outou-lsp` is whatever profile `cargo
//! test` builds: the debug `outou-lsp` by default, the release one under
//! `cargo test --release`. The completion threshold below is asserted in
//! the default (debug) profile; release runs are information only.
//!
//! **Environment.**
//!
//! - `MEASURE_A1_FIXTURE=phase0-app|workspace` (rss tests only; default
//!   `phase0-app`). `workspace` copies `tests/fixtures/workspace` whole,
//!   rewrites the `outou` path dependency in both `app/Cargo.toml` and
//!   `ui-kit/Cargo.toml`, runs the `outou build` step for both members and
//!   measures the `app` member.
//! - `MEASURE_A1_COMPLETION_N` (positive integer) and
//!   `MEASURE_A1_COMPLETION_SITE` (non-empty site label) restrict the
//!   completion run; both are validated before anything is spawned.
//! - `MEASURE_A1_STALL_SESSIONS` (positive integer, default 5),
//!   `MEASURE_A1_STALL_VIA=direct|outou-lsp` (default `direct`) and
//!   `MEASURE_A1_STALL_LOG_DIR` (existing directory; a direct session then
//!   runs rust-analyzer with `--log-file` and `RA_LOG=info`),
//!   `MEASURE_A1_STALL_SITE` (a completion site label),
//!   `MEASURE_A1_STALL_AUTOIMPORT=true|false` and
//!   `MEASURE_A1_STALL_WINDOW_MS` and `MEASURE_A1_STALL_TAG` (suffix of the
//!   saved report name) configure
//!   `measure_a1_completion_stall` (issue #21's stall investigation).
//! - `MEASURE_A1_SAVE_ARTIFACTS=1` writes each run's JSON to
//!   `spikes/rust-analyzer/results/` instead of a system temp directory.
//!   Raw JSON is not kept in the repository by default: the tables in
//!   `docs/alpha-results.md` are the record.
//!
//! **Missing prerequisites.** The heavy tests need `node` and a
//! `rust-analyzer` binary (`OUTOU_RUST_ANALYZER` or `PATH`). They are only
//! ever run explicitly with `--ignored`, so a missing prerequisite panics
//! (rather than returning a silent pass that looks like a measurement).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use outou_cli::build::emit::generate_unit;
use outou_cli::build::plan::{self, CrateRoot};
use outou_codegen::Mode;

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

// ---------------------------------------------------------------------
// (i) Incremental `.rsx` -> generated Rust distribution, both workspaces.
// Pure Rust, no rust-analyzer, cheap enough to run under the default
// `cargo test --workspace` budget.
// ---------------------------------------------------------------------

/// One [`generate_unit`] call's wall time, in a distribution of `n`
/// repeated calls against the same unit's own (unchanged) source text —
/// the generation step of a single-keystroke edit on the language-server
/// path once the module graph is already planned, i.e. the common case
/// (`Workspace::regenerate` only re-plans the whole crate when the set of
/// `mod` declarations changes, `crates/outou-lsp/README.md`). It excludes
/// the rest of `regenerate` (declared-modules re-parse, `LineIndex`,
/// registry update), which `regenerating_one_unit_is_fast` covers.
fn regen_distribution_ms(unit: &plan::PlannedUnit, source: &str, n: usize) -> Vec<f64> {
    let mut samples = Vec::with_capacity(n);
    for _ in 0..n {
        let start = Instant::now();
        generate_unit(unit, source, Mode::Recovery).expect("regenerating a valid unit succeeds");
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples
}

fn median(samples: &[f64]) -> f64 {
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[mid]
    } else {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    }
}

fn p95(samples: &[f64]) -> f64 {
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((0.95 * sorted.len() as f64).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len() - 1);
    sorted[idx]
}

fn max_of(samples: &[f64]) -> f64 {
    samples.iter().cloned().fold(f64::MIN, f64::max)
}

/// Plans `manifest_dir` and returns every planned unit's `(unit, source
/// text)` pair, for a crate that must already have a `.rsx` crate root.
fn planned_units_with_sources(manifest_dir: &Path) -> Vec<(plan::PlannedUnit, String)> {
    let canonical =
        plan::canonical_manifest_dir(manifest_dir).expect("canonicalizing manifest_dir");
    let root = match plan::find_crate_root(&canonical).expect("finding crate root") {
        CrateRoot::Rsx(root) => root,
        CrateRoot::NoRsxRoot => panic!("{} has no `.rsx` crate root", manifest_dir.display()),
    };
    let planned = plan::plan(&canonical, &root).expect("planning the crate");
    planned
        .units
        .into_iter()
        .map(|unit| {
            let source = std::fs::read_to_string(&unit.source_file).expect("reading unit source");
            (unit, source)
        })
        .collect()
}

/// (i) Incremental `.rsx` -> generated Rust, N >= 30, on both of A1's
/// fixed workspaces (`examples/phase0-app`; `tests/fixtures/workspace`'s
/// `app` and `ui-kit` members). Not `#[ignore]`d: this needs no
/// rust-analyzer and no subprocess, so it runs under the ordinary
/// `cargo test --workspace` budget. It asserts the **p95** (nearest rank,
/// N=40) per unit, not the max: a single scheduler hiccup under concurrent
/// compile load can push one sample of a sub-millisecond call far past
/// 10 ms without saying anything about the code, whereas a p95 over 10 ms
/// means at least 2 of 40 calls were slow. The max is printed, not asserted.
const REGEN_BOUND_MS: f64 = 10.0;

#[test]
fn incremental_regeneration_distribution_on_both_workspaces() {
    const N: usize = 40;
    let root = repo_root();

    let mut report = serde_json::Map::new();
    for (label, manifest_dir) in [
        ("phase0-app", root.join("examples/phase0-app")),
        ("workspace/app", root.join("tests/fixtures/workspace/app")),
        (
            "workspace/ui-kit",
            root.join("tests/fixtures/workspace/ui-kit"),
        ),
    ] {
        let units = planned_units_with_sources(&manifest_dir);
        assert!(
            !units.is_empty(),
            "{label}: expected at least one planned unit"
        );

        let mut per_unit = serde_json::Map::new();
        for (unit, source) in &units {
            let name = unit
                .source_file
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();
            let samples = regen_distribution_ms(unit, source, N);
            println!(
                "measure_a1 regen: {label}/{name}: n={N} median={:.4}ms p95={:.4}ms max={:.4}ms",
                median(&samples),
                p95(&samples),
                max_of(&samples)
            );
            per_unit.insert(
                name.clone(),
                serde_json::json!({
                    "n": N,
                    "medianMs": median(&samples),
                    "p95Ms": p95(&samples),
                    "maxMs": max_of(&samples),
                    "samples": samples,
                }),
            );
            // Same bound `regenerating_one_unit_is_fast` asserts (10ms,
            // fixed and user-confirmed 2026-09-27, `docs/alpha-results.md`,
            // "A1: measurement"), applied to the p95 — a catastrophic-
            // regression guard here, not a re-derivation of it.
            assert!(
                p95(&samples) < REGEN_BOUND_MS,
                "{label}/{name}: p95 regeneration time {:.4}ms was not under {REGEN_BOUND_MS}ms (max {:.4}ms)",
                p95(&samples),
                max_of(&samples)
            );
        }
        report.insert(label.to_string(), per_unit.into());
    }

    save_report("regen-distribution", &serde_json::Value::Object(report));
}

// ---------------------------------------------------------------------
// Shared heavy-probe plumbing (mirrors `tests/gate3.rs`'s own helpers).
// ---------------------------------------------------------------------

static COPY_COUNTER: AtomicU32 = AtomicU32::new(0);

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

/// Which fixed workspace a heavy probe measures.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fixture {
    /// `examples/phase0-app` (single crate). Every mode.
    Phase0App,
    /// `tests/fixtures/workspace`'s `app` member (two-member workspace
    /// with a cross-crate `ui-kit` path dependency). `rss` only.
    Workspace,
}

impl Fixture {
    fn label(self) -> &'static str {
        match self {
            Fixture::Phase0App => "phase0-app",
            Fixture::Workspace => "workspace",
        }
    }

    fn from_env() -> Fixture {
        match std::env::var("MEASURE_A1_FIXTURE").as_deref() {
            Err(_) | Ok("phase0-app") => Fixture::Phase0App,
            Ok("workspace") => Fixture::Workspace,
            Ok(other) => {
                panic!("MEASURE_A1_FIXTURE={other:?} is not one of `phase0-app`, `workspace`")
            }
        }
    }
}

/// A fresh temporary copy of a fixture. `temp_root` is what to delete
/// afterwards; `crate_dir` is the crate the harness measures (the copy
/// itself for `phase0-app`, its `app` member for `workspace`).
struct Copy {
    temp_root: PathBuf,
    crate_dir: PathBuf,
}

/// Fresh temporary copy of `fixture` with every `outou` path dependency
/// rewritten to an absolute path — see `tests/gate3.rs`'s own `fresh_copy`
/// for the full rationale (canonicalization for macOS `/tmp` symlink
/// resolution, never touching the repository tree itself). The workspace
/// fixture is copied whole so `app`'s `../ui-kit` path dependency keeps
/// resolving, and BOTH members' `Cargo.toml` are rewritten.
fn fresh_copy(root: &Path, tag: &str, fixture: Fixture) -> Copy {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let n = COPY_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dest = std::env::temp_dir().join(format!(
        "outou-measure-a1-{tag}-{}-{nanos}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dest).expect("creating the temp copy's root");
    let dest = dest
        .canonicalize()
        .expect("canonicalizing the temp copy's root");
    let (source, manifests, crate_dir) = match fixture {
        Fixture::Phase0App => (
            root.join("examples/phase0-app"),
            vec![dest.join("Cargo.toml")],
            dest.clone(),
        ),
        Fixture::Workspace => (
            root.join("tests/fixtures/workspace"),
            vec![dest.join("app/Cargo.toml"), dest.join("ui-kit/Cargo.toml")],
            dest.join("app"),
        ),
    };
    copy_recursive(&source, &dest);

    let outou_crate_dir = root
        .join("crates/outou")
        .canonicalize()
        .expect("crates/outou exists");
    for manifest in manifests {
        let cargo_toml = std::fs::read_to_string(&manifest)
            .unwrap_or_else(|e| panic!("reading {}: {e}", manifest.display()));
        let rewritten = cargo_toml.replace(
            "path = \"../../crates/outou\"",
            &format!("path = {:?}", outou_crate_dir),
        );
        let rewritten = rewritten.replace(
            "path = \"../../../../crates/outou\"",
            &format!("path = {:?}", outou_crate_dir),
        );
        assert_ne!(
            rewritten,
            cargo_toml,
            "expected the outou path dependency in {}",
            manifest.display()
        );
        std::fs::write(&manifest, rewritten)
            .unwrap_or_else(|e| panic!("rewriting {}: {e}", manifest.display()));
    }
    Copy {
        temp_root: dest,
        crate_dir,
    }
}

/// The `outou build` step (plan + emit) for one crate directory.
fn seed_build(crate_dir: &Path) {
    use outou_cli::build::{emit, plan};
    let canonical =
        plan::canonical_manifest_dir(crate_dir).expect("canonicalizing the temp crate dir");
    let root = match plan::find_crate_root(&canonical).expect("finding the temp crate's root") {
        plan::CrateRoot::Rsx(root) => root,
        plan::CrateRoot::NoRsxRoot => {
            panic!(
                "expected {} to have a `.rsx` crate root",
                crate_dir.display()
            )
        }
    };
    let planned = plan::plan(&canonical, &root).expect("planning the temp crate");
    emit::emit(&planned).expect("seeding the temp crate's generated Rust");
}

/// Shared `CARGO_TARGET_DIR` across every heavy probe, outside the
/// repository tree, so repeated cold-indexing runs are not each also
/// paying to recompile `dioxus` and its dependency graph from scratch —
/// mirrors `tests/gate3.rs`'s own `shared_cargo_target_dir`.
fn shared_cargo_target_dir() -> PathBuf {
    std::env::temp_dir().join("outou-measure-a1-shared-target")
}

fn artifacts_dir(root: &Path) -> PathBuf {
    if std::env::var("MEASURE_A1_SAVE_ARTIFACTS").as_deref() == Ok("1") {
        root.join("spikes/rust-analyzer/results")
    } else {
        std::env::temp_dir().join("outou-measure-a1-results")
    }
}

fn save_report(name: &str, json: &serde_json::Value) {
    let root = repo_root();
    let dir = artifacts_dir(&root);
    std::fs::create_dir_all(&dir).expect("creating the report directory");
    let path = dir.join(format!("measure-a1-{name}.json"));
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(json).expect("report serializes"),
    )
    .unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
    println!("measure_a1: wrote {}", path.display());
}

/// Runs `measure-a1.mjs` in `mode` against `crate_dir`, returning its
/// parsed JSON stdout. `extra_args` are appended verbatim (e.g. `--n`,
/// `--config`, `--phase`).
fn run_measure(setup: &Setup, mode: &str, extra_args: &[&str]) -> serde_json::Value {
    let script = setup
        .repo
        .join("spikes/rust-analyzer/client/measure-a1.mjs");
    let mut command = Command::new("node");
    command
        .arg(&script)
        .arg("--mode")
        .arg(mode)
        .arg("--root")
        .arg(&setup.copy.crate_dir)
        .arg("--outou-lsp")
        .arg(&setup.outou_lsp)
        .arg("--ra")
        .arg(&setup.ra)
        .env("CARGO_TARGET_DIR", shared_cargo_target_dir());
    for arg in extra_args {
        command.arg(arg);
    }
    let output = command
        .output()
        .unwrap_or_else(|e| panic!("running measure-a1.mjs --mode {mode}: {e}"));
    if !output.status.success() {
        panic!(
            "measure-a1.mjs --mode {mode} failed (status {}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("parsing measure-a1.mjs --mode {mode} output: {e}"))
}

/// Everything a heavy probe needs. Dropping it deletes the temp copy.
struct Setup {
    repo: PathBuf,
    copy: Copy,
    outou_lsp: PathBuf,
    ra: PathBuf,
}

impl Drop for Setup {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.copy.temp_root).ok();
    }
}

/// Prerequisites for the heavy probes. They only ever run explicitly
/// (`--ignored`), so a missing `node` or `rust-analyzer` panics loudly
/// instead of returning a pass that looks like a measurement.
fn heavy_probe_setup(tag: &str, fixture: Fixture) -> Setup {
    assert!(
        node_available(),
        "measure_a1 {tag}: NOT RUN - `node` is not available on PATH"
    );
    let ra = rust_analyzer_binary().unwrap_or_else(|| {
        panic!(
            "measure_a1 {tag}: NOT RUN - no rust-analyzer binary found (checked OUTOU_RUST_ANALYZER and PATH)"
        )
    });
    let repo = repo_root();
    let outou_lsp = PathBuf::from(env!("CARGO_BIN_EXE_outou-lsp"));
    let copy = fresh_copy(&repo, tag, fixture);
    let setup = Setup {
        repo,
        copy,
        outou_lsp,
        ra,
    };
    seed_build(&setup.copy.crate_dir);
    if fixture == Fixture::Workspace {
        seed_build(&setup.copy.temp_root.join("ui-kit"));
    }
    setup
}

// ---------------------------------------------------------------------
// (D) cold `initialize` -> first usable hover/completion/definition.
// ---------------------------------------------------------------------

#[test]
#[ignore = "spawns outou-lsp + rust-analyzer N times; run explicitly, see module docs"]
fn measure_a1_coldinit() {
    let setup = heavy_probe_setup("coldinit", Fixture::Phase0App);
    let result = run_measure(&setup, "coldinit", &["--n", "5"]);
    println!(
        "measure_a1 coldinit summary: {}",
        serde_json::to_string_pretty(&result["summary"]).unwrap()
    );
    save_report("coldinit", &result);
}

// ---------------------------------------------------------------------
// (E) flycheck latency: `didSave` -> mapped semantic diagnostic.
// ---------------------------------------------------------------------

#[test]
#[ignore = "spawns outou-lsp + rust-analyzer sessions; run explicitly, see module docs"]
fn measure_a1_flycheck() {
    let setup = heavy_probe_setup("flycheck", Fixture::Phase0App);
    let result = run_measure(&setup, "flycheck", &["--n", "10"]);
    println!(
        "measure_a1 flycheck summary (50 ms poll resolution): {}",
        serde_json::to_string_pretty(&result["summary"]).unwrap()
    );
    save_report("flycheck", &result);
}

// ---------------------------------------------------------------------
// (F) forwarded-completion overhead, per candidate-list-size bucket.
// ---------------------------------------------------------------------

const COMPLETION_BASE_MS: f64 = 5.0;
const COMPLETION_PER_CANDIDATE_MS: f64 = 0.15;
const COMPLETION_DEFAULT_N: &str = "10";

/// Reads and validates `MEASURE_A1_COMPLETION_N` / `_SITE` before anything
/// is spawned: `n` must be a positive integer, `site` non-empty if set.
fn completion_env() -> (String, Option<String>) {
    let n = std::env::var("MEASURE_A1_COMPLETION_N")
        .unwrap_or_else(|_| COMPLETION_DEFAULT_N.to_string());
    let parsed: u64 = n
        .parse()
        .unwrap_or_else(|_| panic!("MEASURE_A1_COMPLETION_N={n:?} is not a positive integer"));
    assert!(
        parsed > 0,
        "MEASURE_A1_COMPLETION_N={n:?} is not a positive integer"
    );
    let site = std::env::var("MEASURE_A1_COMPLETION_SITE").ok();
    if let Some(site) = &site {
        assert!(
            !site.trim().is_empty(),
            "MEASURE_A1_COMPLETION_SITE is set but empty"
        );
    }
    (parsed.to_string(), site)
}

/// Fixed threshold (user-confirmed 2026-10-11, `docs/alpha-results.md`,
/// "Thresholds" (2)): median overhead <= 5 ms + 0.15 ms x candidate count,
/// per bucket, asserted for the dev/debug-profile `outou-lsp` only (this
/// test runs `CARGO_BIN_EXE_outou-lsp`). Under `cargo test --release` the
/// same assertion runs (with a per-bucket verdict line naming the profile,
/// issue #21) but is far too loose to catch a release regression
/// (about 31x at 1204 candidates); read those numbers, not the pass.
/// A bucket also fails if the two sessions did not settle on, and get
/// sampled at, the same candidate count: the ceiling would otherwise be
/// computed for the wrong list size.
fn assert_completion_buckets(result: &serde_json::Value) {
    for bucket in result["buckets"].as_array().expect("buckets array") {
        let label = bucket["label"].as_str().unwrap_or("?");
        for side in ["direct", "outouLsp"] {
            let retried = &bucket[side]["retriedSamples"];
            if retried.as_array().is_some_and(|r| !r.is_empty()) {
                println!("measure_a1 completion: {label}/{side}: RETRIED samples (index, attempts): {retried}");
            }
        }
        let direct_count = bucket["direct"]["candidateCount"]
            .as_u64()
            .unwrap_or_else(|| panic!("{label}: direct candidateCount missing"));
        let outou_count = bucket["outouLsp"]["candidateCount"]
            .as_u64()
            .unwrap_or_else(|| panic!("{label}: outouLsp candidateCount missing"));
        assert!(
            bucket["candidateCountsAgree"].as_bool() == Some(true),
            "{label}: sessions disagree on the candidate count (direct settled {}, sampled {direct_count}; \
             outou-lsp settled {}, sampled {outou_count})",
            bucket["direct"]["settledCount"],
            bucket["outouLsp"]["settledCount"],
        );
        for side in ["direct", "outouLsp"] {
            assert!(
                bucket[side]["countsStable"].as_bool() == Some(true),
                "{label}/{side}: candidate count changed during sampling ({}); the session had not settled",
                bucket[side]["candidateCounts"]
            );
        }
        assert!(direct_count > 0, "{label}: 0 candidates");
        let overhead = bucket["overheadMs"]["median"]
            .as_f64()
            .expect("median overhead");
        let ceiling = COMPLETION_BASE_MS + COMPLETION_PER_CANDIDATE_MS * direct_count as f64;
        // Issue #21: the same formula judges whichever profile this test
        // binary is. Printing the verdict with the profile named keeps a
        // release run's (very loose) pass from being mistaken for
        // evidence that the release overhead is close to the ceiling.
        let profile = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };
        println!(
            "measure_a1 completion verdict [{profile}]: {label}: {direct_count} candidates, \
             median overhead {overhead:.1} ms vs ceiling {ceiling:.1} ms ({})",
            if overhead <= ceiling { "PASS" } else { "FAIL" }
        );
        assert!(
            overhead <= ceiling,
            "{label}: median forwarded-completion overhead {overhead:.1}ms exceeds {ceiling:.1}ms \
             ({COMPLETION_BASE_MS}ms + {COMPLETION_PER_CANDIDATE_MS}ms x {direct_count} candidates)"
        );
    }
}

#[test]
#[ignore = "spawns outou-lsp + rust-analyzer sessions; run explicitly, see module docs"]
fn measure_a1_completion_overhead() {
    // `MEASURE_A1_COMPLETION_N` / `MEASURE_A1_COMPLETION_SITE` override the
    // defaults (N=10, all sites) for higher-N or single-bucket re-measurement.
    let (n, site) = completion_env();
    let setup = heavy_probe_setup("completion", Fixture::Phase0App);
    let mut extra: Vec<&str> = vec!["--n", &n];
    if let Some(site) = site.as_deref() {
        extra.extend(["--site", site]);
    }
    let result = run_measure(&setup, "completion", &extra);
    println!(
        "measure_a1 completion-overhead buckets: {}",
        serde_json::to_string_pretty(&result["buckets"]).unwrap()
    );
    save_report("completion-overhead", &result);
    assert_completion_buckets(&result);
}

/// Issue #21: the multi-second `large-enum-path` stall investigation.
/// Unlike `measure_a1_completion_overhead` this does NOT wait for the
/// session to settle: completion is requested back-to-back from the
/// `didChange` that introduces the site, and every slow sample is lined
/// up with rust-analyzer's `serverStatus`/`$/progress` events (and, for a
/// direct session with `MEASURE_A1_STALL_LOG_DIR`, its own `RA_LOG`).
/// Asserts nothing; it records what happened.
#[test]
#[ignore = "spawns rust-analyzer sessions; run explicitly, see module docs"]
fn measure_a1_completion_stall() {
    let sessions = std::env::var("MEASURE_A1_STALL_SESSIONS").unwrap_or_else(|_| "5".to_string());
    assert!(
        sessions.parse::<u64>().is_ok_and(|n| n > 0),
        "MEASURE_A1_STALL_SESSIONS={sessions:?} is not a positive integer"
    );
    let via = std::env::var("MEASURE_A1_STALL_VIA").unwrap_or_else(|_| "direct".to_string());
    let log_dir = std::env::var("MEASURE_A1_STALL_LOG_DIR").ok();
    let site = std::env::var("MEASURE_A1_STALL_SITE").ok();
    let autoimport = std::env::var("MEASURE_A1_STALL_AUTOIMPORT").ok();
    let window = std::env::var("MEASURE_A1_STALL_WINDOW_MS").ok();
    let setup = heavy_probe_setup("stall", Fixture::Phase0App);
    let mut extra: Vec<&str> = vec!["--n", &sessions, "--via", &via];
    if let Some(dir) = log_dir.as_deref() {
        extra.extend(["--log-dir", dir]);
    }
    for (flag, value) in [
        ("--site", site.as_deref()),
        ("--autoimport", autoimport.as_deref()),
        ("--window-ms", window.as_deref()),
    ] {
        if let Some(value) = value {
            extra.extend([flag, value]);
        }
    }
    let result = run_measure(&setup, "stall", &extra);
    for session in result["sessions"].as_array().expect("sessions array") {
        println!(
            "measure_a1 stall: session {} via {}: {} samples, first non-empty at {} ms, max {} ms, counts seen {}, slow (> 500 ms): {}",
            session["index"],
            session["via"],
            session["sampleCount"],
            session["firstNonEmptyAtMs"],
            session["maxMs"],
            session["countsSeen"],
            session["slow"],
        );
    }
    let suffix = std::env::var("MEASURE_A1_STALL_TAG").unwrap_or_default();
    save_report(&format!("completion-stall-{via}{suffix}"), &result);
}

// ---------------------------------------------------------------------
// (F') RSS/CPU sampling: single plain rust-analyzer vs. the two-process
// arrangement (`crates/outou-lsp/README.md`, "Two processes, not one").
// One process per config measures the cold window (spawn -> settled) and,
// in the SAME process, the scripted-edit window.
// ---------------------------------------------------------------------

fn run_rss(config: &str, tag: &str) {
    let fixture = Fixture::from_env();
    let setup = heavy_probe_setup(&format!("{tag}-{}", fixture.label()), fixture);
    let result = run_measure(
        &setup,
        "rss",
        &[
            "--config",
            config,
            "--phase",
            "cold+edit",
            "--interval-ms",
            "500",
            "--edits",
            "10",
            "--edit-delay-ms",
            "500",
        ],
    );
    println!(
        "measure_a1 rss {config} ({}) windows: {}",
        fixture.label(),
        serde_json::to_string_pretty(&result["windows"]).unwrap()
    );
    println!(
        "measure_a1 rss {config} machine state: {}",
        result["machineState"]
    );
    save_report(&format!("rss-{config}-{}", fixture.label()), &result);
}

#[test]
#[ignore = "spawns rust-analyzer sessions for an extended sampling window; run explicitly, see module docs"]
fn measure_a1_rss_single() {
    run_rss("single", "rss-single");
}

#[test]
#[ignore = "spawns outou-lsp + two rust-analyzer sessions for an extended sampling window; run explicitly, see module docs"]
fn measure_a1_rss_two_process() {
    run_rss("two-process", "rss-two-process");
}
