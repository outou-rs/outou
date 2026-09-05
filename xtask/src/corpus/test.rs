//! `cargo xtask corpus test`: runs `outou_syntax::parse` over every
//! `*.rs` file in every fetched corpus, in a `catch_unwind` with a
//! per-file wall-clock budget, and reports panics, timeouts, Outou
//! diagnostics on plain Rust (false positives), and splice round-trip
//! mismatches (including any JSX element found at all, itself a
//! mis-detection on plain Rust input).

use std::fs;
use std::panic;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use super::lock::CorpusEntry;
use super::report::{EntrySummary, FalsePositive, PanicRecord, RoundTripMismatch, Summary};
use super::{fetch, splice, walk};

/// How long one file's parse is allowed to run before it is reported as
/// a timeout (issue #12: "a per-file wall-clock budget (e.g. 2 s)").
const PER_FILE_BUDGET: Duration = Duration::from_secs(2);

/// The `tests/ui` subdirectory categories issue #12's checklist calls
/// out by name, each with the candidate directory names it might exist
/// under in an actual rust-lang/rust checkout (naming has drifted across
/// rustc versions; `qualified-paths` is filed under `fully-qualified-type`
/// as of tag `1.98.1`, confirmed against the checkout this pin fetches).
/// A category whose directory is not found in a given entry's checkout is
/// still reported, at `0`, rather than silently omitted.
const UI_TEST_DIRECTORY_CATEGORIES: &[(&str, &[&str])] = &[
    ("macros", &["macros"]),
    ("parser", &["parser"]),
    ("generics", &["generics"]),
    ("lifetimes", &["lifetimes"]),
    (
        "qualified paths",
        &[
            "qualified-paths",
            "qualified_paths",
            "qualified",
            "fully-qualified-type",
        ],
    ),
];

pub fn run(root: &Path, entries: &[CorpusEntry], strict: bool) -> Result<(), String> {
    let mut summary = Summary::default();

    // Suppress the default panic hook's stderr backtrace for the whole
    // scan: every panic here is expected to be *possible* (that is what
    // this command is checking for) and is already captured and reported
    // through the summary; letting the default hook print for each one
    // would flood a nightly CI log with noise for exactly the failures
    // this report already lists.
    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let scan_result = scan_all(root, entries, &mut summary);
    panic::set_hook(previous_hook);
    scan_result?;

    let report_path = root.join(".corpus").join("report.json");
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    let json = super::report::to_json(&summary);
    let text = serde_json::to_string_pretty(&json).expect("summary always serializes") + "\n";
    fs::write(&report_path, &text)
        .map_err(|e| format!("writing {}: {e}", report_path.display()))?;

    println!("{}", super::report::to_markdown(&summary));
    println!(
        "corpus test: full report written to {}",
        report_path.display()
    );

    let panics = summary.panics.len();
    let timeouts = summary.timeouts.len();
    let false_positives = summary.false_positives.len();
    let round_trip_mismatches = summary.round_trip_mismatches.len();

    if panics > 0 || timeouts > 0 {
        return Err(format!(
            "corpus test: {panics} panic(s), {timeouts} timeout(s) — see {}",
            report_path.display()
        ));
    }
    if strict && (false_positives > 0 || round_trip_mismatches > 0) {
        return Err(format!(
            "corpus test --strict: {false_positives} false positive(s), {round_trip_mismatches} round-trip mismatch(es) — see {}",
            report_path.display()
        ));
    }
    Ok(())
}

fn scan_all(root: &Path, entries: &[CorpusEntry], summary: &mut Summary) -> Result<(), String> {
    for entry in entries {
        let base_dir = fetch::checkout_dir(root, entry);
        if !base_dir.is_dir() {
            return Err(format!(
                "corpus test: {} not found at {} — run `cargo xtask corpus fetch` first",
                entry.name,
                base_dir.display()
            ));
        }
        let files = walk::collect_rs_files(&base_dir);
        let mut category_counts = directory_category_counts(&base_dir, &files);
        let files_scanned = files.len();

        // Content-based categories (issue #12's checklist also names
        // "valid/invalid Rust" and "raw strings", neither of which is a
        // single `tests/ui` subdirectory): a file with a sibling
        // `<name>.stderr` is one rustc expects to fail on ("invalid"
        // Rust, in the checklist's sense — not a claim about this
        // parser); a file containing `r"`/`r#"` syntax anywhere counts
        // toward "raw strings". Computed in the same pass as the parse
        // itself so each file's content is read only once.
        let mut invalid_count = 0usize;
        let mut raw_string_count = 0usize;
        for file in &files {
            if file.with_extension("stderr").is_file() {
                invalid_count += 1;
            }
            if scan_file(root, file, summary) {
                raw_string_count += 1;
            }
        }
        category_counts.push((
            "valid Rust (no `.stderr` companion)".to_string(),
            files_scanned - invalid_count,
        ));
        category_counts.push((
            "invalid Rust (has a `.stderr` companion)".to_string(),
            invalid_count,
        ));
        category_counts.push(("raw strings".to_string(), raw_string_count));

        summary.files_scanned += files_scanned;
        summary.entries.push(EntrySummary {
            name: entry.name.clone(),
            files_scanned,
            category_counts,
        });
    }
    Ok(())
}

/// Scans one file: parses it, records any panic/timeout/false
/// positive/round-trip mismatch onto `summary`, and returns whether its
/// source contains raw-string syntax (`r"..."` or `r#"..."#`, including
/// the `b`/`c` prefixed forms), for the entry's "raw strings" category
/// count.
fn scan_file(root: &Path, file: &Path, summary: &mut Summary) -> bool {
    let relative = file
        .strip_prefix(root)
        .unwrap_or(file)
        .display()
        .to_string();

    let source = match fs::read_to_string(file) {
        Ok(source) => source,
        Err(error) => {
            summary.panics.push(PanicRecord {
                file: relative,
                message: format!("could not read file: {error}"),
            });
            return false;
        }
    };

    let has_raw_string = contains_raw_string_syntax(&source);

    match parse_with_budget(source.clone(), PER_FILE_BUDGET) {
        FileOutcome::Panic(message) => summary.panics.push(PanicRecord {
            file: relative,
            message,
        }),
        FileOutcome::Timeout => summary.timeouts.push(relative),
        FileOutcome::Parsed(parsed) => {
            if !parsed.diagnostics.is_empty() {
                summary.false_positives.push(FalsePositive {
                    file: relative.clone(),
                    diagnostic_count: parsed.diagnostics.len(),
                    first_message: parsed.diagnostics[0].message.clone(),
                });
            }

            let elements = splice::collect_jsx_elements(&parsed.file);
            if let Some(first) = elements.first() {
                let (line, col) = splice::line_col(&source, first.span.start);
                summary.round_trip_mismatches.push(RoundTripMismatch {
                    file: relative,
                    description: format!(
                        "JSX element detected at {line}:{col} (mis-detection: this corpus is plain Rust)"
                    ),
                });
            } else {
                let rebuilt = splice::rebuild_source(&parsed.file, &source);
                if rebuilt != source {
                    summary.round_trip_mismatches.push(RoundTripMismatch {
                        file: relative,
                        description: "splice round trip did not reproduce the source".to_string(),
                    });
                }
            }
        }
    }

    has_raw_string
}

/// Whether `source` contains raw-string literal syntax anywhere: `r"`,
/// `r#`, or either prefixed with `b`/`c` (byte/C-string). A cheap
/// substring scan rather than a real lexer — good enough for the
/// "raw strings" corpus category count, not a claim about how many raw
/// string *literals* the file has.
fn contains_raw_string_syntax(source: &str) -> bool {
    ["r\"", "r#", "br\"", "br#", "cr\"", "cr#"]
        .iter()
        .any(|needle| source.contains(needle))
}

enum FileOutcome {
    Parsed(outou_syntax::Parsed),
    Panic(String),
    Timeout,
}

/// Runs `outou_syntax::parse` on its own thread, wrapped in
/// `catch_unwind`, and waits for it for at most `budget`. A `parse` that
/// hangs (an infinite loop, not a panic) cannot be interrupted — `std`
/// has no thread-kill primitive — so the timeout path leaves that thread
/// running and detached rather than blocking `corpus test` on it.
fn parse_with_budget(source: String, budget: Duration) -> FileOutcome {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = panic::catch_unwind(|| outou_syntax::parse(&source));
        let _ = tx.send(result);
    });
    match rx.recv_timeout(budget) {
        Ok(Ok(parsed)) => FileOutcome::Parsed(parsed),
        Ok(Err(payload)) => FileOutcome::Panic(panic_message(payload)),
        Err(_) => FileOutcome::Timeout,
    }
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

/// Per-category file counts under `base_dir`, resolving each category's
/// first matching candidate directory name (see
/// [`UI_TEST_DIRECTORY_CATEGORIES`]); a category with no matching
/// directory in this checkout is reported as `0` rather than omitted.
fn directory_category_counts(base_dir: &Path, files: &[PathBuf]) -> Vec<(String, usize)> {
    UI_TEST_DIRECTORY_CATEGORIES
        .iter()
        .map(|(label, candidates)| {
            let resolved = candidates
                .iter()
                .map(|candidate| base_dir.join(candidate))
                .find(|candidate_dir| candidate_dir.is_dir());
            let count = match resolved {
                Some(dir) => files.iter().filter(|file| file.starts_with(&dir)).count(),
                None => 0,
            };
            (label.to_string(), count)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_with_budget_reports_success() {
        match parse_with_budget("fn f() {}".to_string(), Duration::from_secs(2)) {
            FileOutcome::Parsed(parsed) => assert!(parsed.diagnostics.is_empty()),
            _ => panic!("expected a successful parse"),
        }
    }

    #[test]
    fn category_counts_are_zero_for_a_missing_category() {
        let dir = std::env::temp_dir().join(format!(
            "outou-xtask-corpus-test-categories-{}",
            std::process::id()
        ));
        fs::create_dir_all(dir.join("macros")).unwrap();
        fs::write(dir.join("macros/a.rs"), "").unwrap();

        let files = walk::collect_rs_files(&dir);
        let counts = directory_category_counts(&dir, &files);
        let counts: std::collections::BTreeMap<_, _> = counts.into_iter().collect();
        assert_eq!(counts["macros"], 1);
        assert_eq!(counts["generics"], 0);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn raw_string_syntax_is_detected() {
        assert!(contains_raw_string_syntax("let s = r\"a < b\";"));
        assert!(contains_raw_string_syntax("let s = r#\"a\"#;"));
        assert!(contains_raw_string_syntax("let s = br\"bytes\";"));
        assert!(!contains_raw_string_syntax("let s = \"plain\";"));
    }
}
