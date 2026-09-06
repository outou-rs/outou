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
//! TODO(phase0): the semantic/backend mapping+rendering machinery below
//! lives only in this test harness, not behind an `outou check
//! --semantic` flag — the task that requested it left that decision
//! open ("add that flag only if it is small; otherwise the harness-only
//! function is fine"), and wiring a second, real compiler invocation
//! into the `outou` binary itself is not a small change. Revisit if a
//! later phase needs this from the command line.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use outou_backend_dioxus::DioxusBackend;
use outou_codegen::{Backend, GenerateOptions, Mode};
use outou_sourcemap::{NarrowOutcome, SourceMap, Span, Uri};
use outou_syntax::{vocabulary, Diagnostic, Severity};

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
    source_map: SourceMap,
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

            BuildCase {
                case_dir,
                ident,
                rsx_source,
                generated_rust: generated.rust,
                source_map: generated.source_map,
            }
        })
        .collect()
}

/// One `cargo check --message-format=json` diagnostic, in the shape this
/// harness needs from it.
struct RustcMessage {
    /// Rustc's own short message text (`message.message`), before
    /// translation.
    message: String,
    /// `"error"` or `"warning"`; anything else is dropped.
    level: String,
    /// Byte range of the primary span inside the generated file it
    /// belongs to.
    span: Span,
    /// The generated file's path, exactly as rustc reports it
    /// (`"src/<ident>.rs"`, relative to the crate root `cargo check` ran
    /// in), so a message can be routed back to the [`BuildCase`] that
    /// produced it.
    file_name: String,
}

/// Writes every case's generated Rust into one throwaway crate (a
/// library depending on `outou` by path, exactly like
/// `outou-backend-dioxus`'s `compile_check.rs`/`recovery.rs`), runs
/// `cargo check --message-format=json` on it once, and returns every
/// error/warning diagnostic it produced.
fn write_and_check_build_crate(cases: &[BuildCase]) -> Vec<RustcMessage> {
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
        .output()
        .expect("running `cargo check` on the UI build-case crate");

    parse_rustc_messages(&output.stdout)
}

/// Extracts every error/warning [`RustcMessage`] with a primary span from
/// `cargo check --message-format=json`'s stdout, in emission order.
/// Mirrors `outou-backend-dioxus/tests/recovery.rs`'s own JSON-line
/// parsing.
fn parse_rustc_messages(stdout: &[u8]) -> Vec<RustcMessage> {
    let mut out = Vec::new();
    for line in String::from_utf8_lossy(stdout).lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value.get("reason").and_then(|r| r.as_str()) != Some("compiler-message") {
            continue;
        }
        let Some(message) = value.get("message") else {
            continue;
        };
        let Some(level) = message.get("level").and_then(|l| l.as_str()) else {
            continue;
        };
        if level != "error" && level != "warning" {
            continue;
        }
        let Some(spans) = message.get("spans").and_then(|s| s.as_array()) else {
            continue;
        };
        let Some(primary) = spans
            .iter()
            .find(|span| span.get("is_primary").and_then(|p| p.as_bool()) == Some(true))
        else {
            continue;
        };
        let (Some(file_name), Some(byte_start), Some(byte_end)) = (
            primary.get("file_name").and_then(|f| f.as_str()),
            primary.get("byte_start").and_then(|b| b.as_u64()),
            primary.get("byte_end").and_then(|b| b.as_u64()),
        ) else {
            continue;
        };
        let Some(text) = message.get("message").and_then(|m| m.as_str()) else {
            continue;
        };
        out.push(RustcMessage {
            message: text.to_string(),
            level: level.to_string(),
            span: Span::new(byte_start as u32, byte_end as u32),
            file_name: file_name.to_string(),
        });
    }
    out
}

/// Maps a byte span in generated Rust back to its `.rsx` source span, in
/// up to three steps. This harness has no `Workspace`/`Registry` to call
/// into, so it calls the shared span-mapping primitives directly against a
/// bare [`SourceMap`] instead:
///
/// 1. [`SourceMap::narrow`]: the containing mapping has exactly one
///    source, same-length as its generated span (the common case,
///    `Writer::verbatim`) — scale the query proportionally within it.
///    Necessary because one mapping frequently covers a whole spliced
///    Rust run (every plain-Rust statement between two JSX elements in a
///    component body is one mapping): without this step, every
///    diagnostic inside such a run would land at the run's very first
///    byte, regardless of which statement in it actually erred. A
///    [`NarrowOutcome::LengthMismatch`] (a transformed mapping, e.g. an
///    escaped string literal) is treated as unmapped here too, exactly
///    like `outou-lsp`'s own caller (`mapping::generated_location_to_source`)
///    — this harness used to fall through to step 2's coarser answer
///    instead for that case, which let a `tests/ui/*/expected.stderr`
///    encode a position the editor would never actually show for the same
///    input (issue #12 corpus review, F4/HIGH). Both callers now share one
///    `SourceMap::narrow` and one `LengthMismatch` decision — "there is
///    one compiler" (`AGENTS.md`) extends to "there is one narrowing".
/// 2. [`SourceMap::map_range`]'s coarser whole-span answer, when step 1
///    does not apply (several sources, or no containing mapping at all).
/// 3. The nearest mapped position, when the span is unmapped altogether
///    (synthesized code, e.g. inside the `rsx!` macro's own expansion
///    scaffolding) — the same "never drop a hard error just because its
///    span has no direct mapping" rule
///    `outou-lsp`'s `mapping::nearest_source_position` uses.
fn map_generated_span_to_source(source_map: &SourceMap, generated_span: Span) -> Span {
    match source_map.narrow(generated_span) {
        NarrowOutcome::Exact { span, .. } => return span,
        NarrowOutcome::LengthMismatch | NarrowOutcome::NotApplicable => {}
    }
    let mapped = source_map.map_range(generated_span);
    if let Some(first) = mapped.sources.first() {
        return first.span;
    }
    source_map
        .mappings
        .iter()
        .filter(|mapping| !mapping.sources.is_empty())
        .min_by_key(|mapping| generated_distance(mapping.generated, generated_span))
        .map(|mapping| mapping.sources[0].span)
        .unwrap_or(Span::new(0, 0))
}

/// Byte distance between two spans: `0` when they overlap or touch,
/// otherwise the gap between the closer pair of endpoints.
fn generated_distance(a: Span, b: Span) -> u32 {
    if a.end <= b.start {
        b.start.saturating_sub(a.end)
    } else if b.end <= a.start {
        a.start.saturating_sub(b.end)
    } else {
        0
    }
}

/// F21 (issue #12 corpus review, `docs/backend-leakage.md` row 27): a
/// missing required prop produces *two* rustc diagnostics for one `.rsx`
/// mistake — the already-translated deprecated-`build`-method warning,
/// and a second, distinct, untranslated error (`` this method takes 1
/// argument but 0 arguments were supplied ``) at the exact same mapped
/// span, naming no `BACKEND_MARKERS` substring at all so
/// `translate_message` leaves it untouched. A user who writes
/// `<UserCard />` would see that second, backend-shaped-in-substance
/// message as the thing that actually fails their build.
///
/// Rather than a general "smarter than substring" classifier (out of
/// scope, per row 27's own note), this drops any *untranslated*
/// diagnostic whose mapped span exactly matches a diagnostic that *was*
/// translated: the translated one already explains the mistake in Outou
/// vocabulary, so the untranslated second message at the same position
/// adds nothing but confusion. A translated diagnostic, and any
/// untranslated diagnostic at a span no translated one shares, are always
/// kept.
fn suppress_untranslated_duplicates(candidates: Vec<(Diagnostic, bool)>) -> Vec<Diagnostic> {
    let translated_spans: std::collections::HashSet<Span> = candidates
        .iter()
        .filter(|(_, was_translated)| *was_translated)
        .map(|(diagnostic, _)| diagnostic.span)
        .collect();
    candidates
        .into_iter()
        .filter(|(diagnostic, was_translated)| {
            *was_translated || !translated_spans.contains(&diagnostic.span)
        })
        .map(|(diagnostic, _)| diagnostic)
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

    let mut failures = Vec::new();
    for case in &cases {
        let generated_file_name = format!("src/{}.rs", case.ident);
        let input_path = case.case_dir.join("input.rsx");
        let file_name = input_path.display().to_string();

        let diagnostics: Vec<Diagnostic> = suppress_untranslated_duplicates(
            messages
                .iter()
                .filter(|m| m.file_name == generated_file_name)
                .map(|m| {
                    let translated_message = vocabulary::translate_message(&m.message);
                    let was_translated = translated_message != m.message;
                    (
                        Diagnostic {
                            span: map_generated_span_to_source(&case.source_map, m.span),
                            message: translated_message,
                            severity: if m.level == "error" {
                                Severity::Error
                            } else {
                                Severity::Warning
                            },
                        },
                        was_translated,
                    )
                })
                .collect(),
        );

        let rendered =
            outou_syntax::render::render_diagnostics(&diagnostics, &case.rsx_source, &file_name);
        let normalized = normalize(&rendered, &case.case_dir);

        if let Err(failure) = check_or_bless(&case.case_dir, &normalized) {
            failures.push(failure);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
