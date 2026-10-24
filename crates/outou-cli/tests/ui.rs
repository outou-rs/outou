//! UI test harness (issue #11): compares Outou's rendered diagnostics
//! against `tests/ui/<case>/expected.stderr` for every case directory
//! under the repository's `tests/ui/`. See `tests/ui/README.md` for the
//! on-disk format and the `BLESS=1` workflow.
//!
//! Two kinds of case share the same directory tree:
//!
//! - **Fast cases** (no marker file): the "Outou syntax" diagnostic
//!   layer. [`outou_cli::check::check_source`] is called in-process —
//!   never by spawning the `outou` binary — matching "there is one
//!   compiler" (`AGENTS.md`): the harness and `outou check` itself must
//!   go through the exact same code path, only the caller differs.
//!   Exercised by [`ui_syntax_cases_match_expected_stderr`], on every
//!   `cargo test`.
//! - **Build cases** (a `NEEDS_CARGO_CHECK` marker file next to
//!   `input.rsx`): the "Rust semantic (mapped back)" and "backend
//!   (translated)" layers, which need the generated code actually built
//!   against the real `dioxus` dependency tree to observe a genuine
//!   rustc diagnostic. Exercised by [`ui_build_cases_match_expected_stderr`],
//!   `#[ignore]`d for the same reason `outou-backend-dioxus`'s
//!   `compile_check.rs`/`recovery.rs` are (a cold `target/` build can
//!   take well over 30s) — every such case is generated into one shared
//!   throwaway crate so the expensive dependency build happens once for
//!   the whole suite, not once per case. Run explicitly with:
//!
//!   ```sh
//!   cargo test -p outou-cli --test ui -- --ignored --nocapture
//!   ```
//!
//! The semantic/backend mapping and translation machinery is no longer
//! test-only: it lives in `outou_cli::diagnostics` and is what `outou cargo
//! <build|check|clippy|test>` (issue #25) uses on a real Cargo run. This
//! harness feeds it the same `cargo check --message-format=json` stream, so
//! "there is one compiler" (`AGENTS.md`) holds for these layers too. The
//! harness keeps the compact `error: ... --> file:L:C` format for
//! `expected.stderr`; `outou cargo` prints a richer one.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

use outou_backend_dioxus::DioxusBackend;
use outou_cli::diagnostics::translate::SourceFile;
use outou_cli::diagnostics::{
    classify_line, suppress_untranslated_duplicates, translate, CargoLine, CompilerMessage,
    Location, Resolved, Resolver, Unit,
};
use outou_codegen::{Backend, GenerateOptions, Mode};
use outou_sourcemap::Uri;
use outou_syntax::{Diagnostic, Severity};

/// Marker file (empty) that flags a case directory as needing a real
/// `cargo check` rather than the fast in-process syntax layer.
const NEEDS_CARGO_CHECK: &str = "NEEDS_CARGO_CHECK";

/// The repository root, computed from this crate's own manifest
/// directory so tests work regardless of the current working directory
/// `cargo test` was invoked from.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root exists")
}

/// `tests/ui`, under the repository root.
fn ui_dir() -> PathBuf {
    repo_root().join("tests/ui")
}

/// Every case directory under `tests/ui/` (anything containing an
/// `input.rsx`), sorted for a deterministic run order.
fn case_dirs() -> Vec<PathBuf> {
    let dir = ui_dir();
    let mut dirs: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.join("input.rsx").is_file())
        .collect();
    dirs.sort();
    assert!(
        !dirs.is_empty(),
        "expected at least one case under {}",
        dir.display()
    );
    dirs
}

/// Whether `BLESS=1` was set: write `expected.stderr` instead of
/// asserting equality (`tests/ui/README.md`'s blessing workflow).
///
/// F17 (issue #12 corpus review, LOW): checks the value is exactly `"1"`,
/// not merely present — `var_os(..).is_some()` also blessed on `BLESS=0`
/// and even `BLESS=` (empty string), either of which is far more likely
/// to be a leftover shell variable than a deliberate request to overwrite
/// every checked-in `expected.stderr`.
fn bless_requested() -> bool {
    std::env::var("BLESS").as_deref() == Ok("1")
}

/// Replaces every occurrence of `case_dir`'s absolute path with `$DIR`,
/// the same normalization convention rustc's own UI test suite uses:
/// `expected.stderr` is portable across machines and checkouts, never
/// containing this repository's absolute path.
fn normalize(rendered: &str, case_dir: &Path) -> String {
    rendered.replace(&*case_dir.to_string_lossy(), "$DIR")
}

/// Compares `actual` (already normalized) against `case_dir/expected.stderr`,
/// blessing it instead when [`bless_requested`]. Returns `Err` with a
/// diff-shaped message on mismatch rather than asserting directly, so
/// every case in a run is checked and reported, not just the first
/// failure.
fn check_or_bless(case_dir: &Path, actual: &str) -> Result<(), String> {
    let expected_path = case_dir.join("expected.stderr");
    if bless_requested() {
        fs::write(&expected_path, format!("{}\n", actual.trim_end()))
            .unwrap_or_else(|e| panic!("writing {}: {e}", expected_path.display()));
        return Ok(());
    }
    let expected = fs::read_to_string(&expected_path).unwrap_or_else(|e| {
        panic!(
            "reading {} (run with BLESS=1 to create it): {e}",
            expected_path.display()
        )
    });
    if expected.trim_end() == actual.trim_end() {
        Ok(())
    } else {
        Err(format!(
            "{}:\n--- expected ---\n{}\n--- actual ---\n{}\n",
            case_dir.display(),
            expected.trim_end(),
            actual.trim_end()
        ))
    }
}

// ---------------------------------------------------------------------
// Fast (syntax) cases
// ---------------------------------------------------------------------

/// The "Outou syntax" diagnostic layer: parses `input.rsx` and renders
/// Outou's own syntax diagnostics for it, exactly as `outou check` would.
#[test]
fn ui_syntax_cases_match_expected_stderr() {
    let mut failures = Vec::new();
    let mut checked = 0usize;
    for case_dir in case_dirs() {
        if case_dir.join(NEEDS_CARGO_CHECK).is_file() {
            continue;
        }
        checked += 1;
        let input_path = case_dir.join("input.rsx");
        let source = fs::read_to_string(&input_path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", input_path.display()));

        let report = outou_cli::check::check_source(&input_path.display().to_string(), &source);
        let normalized = normalize(&report.rendered, &case_dir);

        if let Err(failure) = check_or_bless(&case_dir, &normalized) {
            failures.push(failure);
        }
    }
    assert!(
        checked > 0,
        "expected at least one fast (non-build) UI case"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---------------------------------------------------------------------
// Build cases: "Rust semantic (mapped back)" and "backend (translated)"
// ---------------------------------------------------------------------

/// One build case, generated and ready to be written into the shared
/// throwaway crate.
struct BuildCase {
    case_dir: PathBuf,
    /// A valid Rust module/file identifier derived from the directory
    /// name (`-` replaced with `_`).
    ident: String,
    rsx_source: String,
    generated_rust: String,
    /// The generated file as the shared translation library sees it.
    unit: Rc<Unit>,
}

/// The generated file name rustc reports for a case, relative to the
/// throwaway crate (`src/<ident>.rs`).
fn generated_file_name(ident: &str) -> String {
    format!("src/{ident}.rs")
}

/// Resolves a rustc file name to a case's in-memory [`Unit`]; there are
/// no source maps on disk in the throwaway crate.
struct HarnessResolver {
    units: HashMap<String, Rc<Unit>>,
}

impl Resolver for HarnessResolver {
    fn resolve(&mut self, file_name: &str, _manifest_path: &Path) -> Resolved {
        match self.units.get(file_name) {
            Some(unit) => Resolved::Unit(Rc::clone(unit)),
            None => Resolved::NotGenerated,
        }
    }
}

/// Generates every build case's Rust and returns it, without writing or
/// compiling anything yet — kept separate from
/// [`write_and_check_build_crate`] so a syntax error in a case's own
/// fixture fails loudly and individually, before the (very expensive)
/// `cargo check` even starts.
fn generate_build_cases() -> Vec<BuildCase> {
    case_dirs()
        .into_iter()
        .filter(|dir| dir.join(NEEDS_CARGO_CHECK).is_file())
        .map(|case_dir| {
            let ident = case_dir
                .file_name()
                .unwrap()
                .to_string_lossy()
                .replace('-', "_");
            let input_path = case_dir.join("input.rsx");
            let rsx_source = fs::read_to_string(&input_path)
                .unwrap_or_else(|e| panic!("reading {}: {e}", input_path.display()));

            let parsed = outou_syntax::parse(&rsx_source);
            assert!(
                !parsed
                    .diagnostics
                    .iter()
                    .any(|d| d.severity == Severity::Error),
                "{}: build case must itself be free of Outou syntax errors",
                case_dir.display()
            );

            let opts = GenerateOptions::new(
                Uri::new(format!("file:///gen/{ident}.rs")),
                Uri::new(format!("file://{}", input_path.display())),
            );
            let generated = DioxusBackend
                .generate(&parsed, &rsx_source, Mode::Strict, &opts)
                .unwrap_or_else(|e| panic!("{}: generation failed: {e}", case_dir.display()));

            let unit = Rc::new(Unit {
                id: generated_file_name(&ident),
                generated_text: generated.rust.clone(),
                source_map: generated.source_map,
                sources: vec![SourceFile {
                    display: input_path.display().to_string(),
                    text: rsx_source.clone(),
                }],
            });
            BuildCase {
                case_dir,
                ident,
                rsx_source,
                generated_rust: generated.rust,
                unit,
            }
        })
        .collect()
}

/// Writes every case's generated Rust into one throwaway crate (a
/// library depending on `outou` by path, exactly like
/// `outou-backend-dioxus`'s `compile_check.rs`/`recovery.rs`), runs
/// `cargo check --message-format=json` on it once, and returns every
/// error/warning diagnostic it produced.
///
/// Unlike `compile_check.rs`/`recovery.rs`/`build_compile.rs`/`matrix.rs`,
/// this harness's expected output (`expected.stderr`) encodes a specific
/// diagnostic *severity*, not merely pass/fail — the CLI's severity
/// translation (`Severity::Error` vs `Severity::Warning`, from `m.level`)
/// is exactly what a case like `backend-missing-required-prop` is testing.
/// If the ambient environment sets `RUSTFLAGS=-D warnings` (as the "example
/// app builds with cargo build alone" CI job does, for the whole job), a
/// `cargo check` that inherits it promotes every rustc warning to an error,
/// which this harness would then correctly re-translate to
/// `Severity::Error` — but that is a different, environment-dependent
/// result from the `Severity::Warning` a plain `cargo check` produces and
/// `expected.stderr` was blessed against. Clearing both the flag and its
/// pre-encoded form makes this `cargo check` deterministic across `cargo
/// test` invocations regardless of the caller's own `RUSTFLAGS`/
/// `CARGO_ENCODED_RUSTFLAGS` (`CARGO_ENCODED_RUSTFLAGS` takes precedence
/// over `RUSTFLAGS` when both are set, so both must be cleared).
fn write_and_check_build_crate(cases: &[BuildCase]) -> Vec<CompilerMessage> {
    let root = repo_root();
    let outou_path = root.join("crates/outou");
    let temp_dir = root.join("target/outou-ui-build-check");
    let src_dir = temp_dir.join("src");
    // F18 (issue #12 corpus review, LOW): this directory is reused across
    // runs (it shares `target/` so a cold `dioxus` build only happens
    // once), but was never cleaned — a case renamed or removed since the
    // last run left its stale `src/<ident>.rs` behind forever, still
    // compiled (and still `mod`-declared only if `lib.rs`, rewritten
    // below, happens to reference it) or worse, silently unreferenced and
    // never even failing loudly. Removing `src/` before writing means
    // every run's throwaway crate contains exactly this run's cases.
    fs::remove_dir_all(&src_dir).ok();
    fs::create_dir_all(&src_dir).expect("creating temp crate src dir");

    let manifest = format!(
        "[package]\n\
         name = \"outou-ui-build-check\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n\
         publish = false\n\
         \n\
         [workspace]\n\
         \n\
         [lib]\n\
         path = \"src/lib.rs\"\n\
         \n\
         [dependencies]\n\
         outou = {{ path = {outou_path:?} }}\n"
    );
    fs::write(temp_dir.join("Cargo.toml"), manifest).expect("writing throwaway Cargo.toml");

    // Every case is an independent module; incidental `dead_code`/
    // `unused_variables` noise (nothing in this crate is ever called)
    // is silenced crate-wide so it can never masquerade as one of the
    // diagnostics a case is actually testing for.
    let mut lib_rs = String::from("#![allow(dead_code, unused_variables)]\n");
    for case in cases {
        lib_rs.push_str(&format!("mod {};\n", case.ident));
        let file_path = src_dir.join(format!("{}.rs", case.ident));
        fs::write(&file_path, &case.generated_rust)
            .unwrap_or_else(|e| panic!("writing {}: {e}", file_path.display()));
    }
    fs::write(src_dir.join("lib.rs"), &lib_rs).expect("writing throwaway lib.rs");

    let output = Command::new(env!("CARGO"))
        .arg("check")
        .arg("--message-format=json")
        .current_dir(&temp_dir)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .output()
        .expect("running `cargo check` on the UI build-case crate");

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(classify_line)
        .filter_map(|line| match line {
            CargoLine::Message(message) => Some(*message),
            CargoLine::Other | CargoLine::BuildFinished => None,
        })
        .collect()
}

/// The "Rust semantic (mapped back)" and "backend (translated)"
/// diagnostic layers: builds every `NEEDS_CARGO_CHECK` case in one shared
/// throwaway crate, maps each rustc diagnostic whose primary span lands
/// in that case's generated file back to its `.rsx` position, translates
/// any backend vocabulary the message still carries
/// (`outou_syntax::vocabulary::translate_message`), and renders it in the
/// exact same `error: … --> $DIR/input.rsx:L:C` format the fast (syntax)
/// layer uses (`outou_syntax::render::render_diagnostics`).
#[test]
#[ignore = "builds the full dioxus dependency tree; run explicitly, see module docs"]
fn ui_build_cases_match_expected_stderr() {
    let cases = generate_build_cases();
    assert!(!cases.is_empty(), "expected at least one build UI case");

    let messages = write_and_check_build_crate(&cases);
    // F17 (issue #12 corpus review, LOW): if the throwaway crate failed to
    // build for a reason unrelated to any individual case (a toolchain
    // problem, a broken shared dependency), `cargo check`'s own JSON
    // stream can come back with zero compiler-message entries even though
    // every case's `input.rsx` is fine. Without this guard, a `BLESS=1`
    // run would then silently overwrite every build case's
    // `expected.stderr` with empty output and report success.
    assert!(
        !messages.is_empty(),
        "expected at least one rustc message from the throwaway build crate; \
         `cargo check` may have failed for an unrelated reason (see stderr above)"
    );

    let mut resolver = HarnessResolver {
        units: cases
            .iter()
            .map(|case| (generated_file_name(&case.ident), Rc::clone(&case.unit)))
            .collect(),
    };
    let translated = suppress_untranslated_duplicates(
        messages
            .iter()
            .filter_map(|message| translate(message, &mut resolver))
            .collect(),
    );

    let mut failures = Vec::new();
    for case in &cases {
        let input_path = case.case_dir.join("input.rsx");
        let file_name = input_path.display().to_string();

        let diagnostics: Vec<Diagnostic> = translated
            .iter()
            .filter(
                |t| matches!(&t.location, Location::Source { unit, .. } if unit.id == case.unit.id),
            )
            .map(|t| t.diagnostic.clone())
            .collect();

        let rendered =
            outou_syntax::render::render_diagnostics(&diagnostics, &case.rsx_source, &file_name);
        let normalized = normalize(&rendered, &case.case_dir);

        if let Err(failure) = check_or_bless(&case.case_dir, &normalized) {
            failures.push(failure);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
