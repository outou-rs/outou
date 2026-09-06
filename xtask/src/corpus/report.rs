//! Building the `.corpus/report.json` document and the markdown summary
//! `corpus test` prints to stdout.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use outou_syntax::Severity;

/// One panic or unreadable file found while testing a corpus.
#[derive(Debug, Clone)]
pub struct PanicRecord {
    /// Path of the file, relative to the repository root.
    pub file: String,
    /// The panic payload, or an I/O error message for an unreadable file.
    pub message: String,
}

/// A file whose parse exceeded the per-file wall-clock budget.
pub type TimeoutRecord = String;

/// A file that produced at least one Outou diagnostic. Since every
/// corpus file is plain Rust, any diagnostic on it is a false positive,
/// not a real failure — reported for count and inspection, not as a
/// failure by default.
///
/// `on_valid_rust`/`first_severity` (issue #12 corpus review, F6/MEDIUM):
/// the previous report counted "any file with any diagnostic" as one
/// undifferentiated bucket, with no regard for whether rustc itself
/// rejects the file (a `.stderr` companion) or whether the diagnostic is
/// only a warning. A Gate 4 number needs that distinction — a warning on
/// a file rustc already rejects is a far smaller signal than an error on
/// ordinary, valid Rust.
#[derive(Debug, Clone)]
pub struct FalsePositive {
    /// Path of the file, relative to the repository root.
    pub file: String,
    /// How many Outou diagnostics this file produced.
    pub diagnostic_count: usize,
    /// The message of the first diagnostic found.
    pub first_message: String,
    /// Severity of the *first* diagnostic found (matching `first_message`).
    pub first_severity: Severity,
    /// `true` when this file has no sibling `.stderr` — rustc accepts it
    /// outright ("valid Rust" in the corpus categories' sense). `false`
    /// means rustc rejects the file too (it has a `.stderr` companion),
    /// so an Outou diagnostic on it is a smaller signal.
    pub on_valid_rust: bool,
}

/// A file on which at least one JSX element was detected at all — itself
/// a mis-detection, since the corpus is plain Rust with no JSX in it.
#[derive(Debug, Clone)]
pub struct JsxMisdetection {
    /// Path of the file, relative to the repository root.
    pub file: String,
    /// Human-readable description, including the first element's
    /// location.
    pub description: String,
}

/// A file whose splice round trip did not reproduce its source, *and* on
/// which zero JSX elements were detected — a genuine splice-partitioning
/// bug (a node's own source slice does not cover what it claims to),
/// independent of JSX mis-detection entirely (issue #12 corpus review,
/// F7/MEDIUM: the previous report folded this and [`JsxMisdetection`]
/// into one `round_trip_mismatches` bucket, only ever populating this
/// half when the JSX-detection half found nothing — so the two were
/// reported as if they were two independent signals when, in every real
/// run so far, they described the exact same files for the exact same
/// reason).
#[derive(Debug, Clone)]
pub struct SpliceMismatch {
    /// Path of the file, relative to the repository root.
    pub file: String,
    /// Human-readable description of the mismatch.
    pub description: String,
}

/// Per-corpus-entry file and category counts.
///
/// `revision`/`resolved_commit` (issue #12 corpus review, F9/MEDIUM): a
/// bare `20,722 / 0 / 0 / 61` count cannot be tied to anything without
/// knowing which corpus, at which revision, produced it.
#[derive(Debug, Clone)]
pub struct EntrySummary {
    /// The corpus entry's name (`corpus.lock`'s `name` field).
    pub name: String,
    /// How many `.rs` files were scanned under this entry.
    pub files_scanned: usize,
    /// Counts per tracked `tests/ui` subdirectory category (issue #12's
    /// checklist: macro token trees, qualified paths, raw strings,
    /// lifetimes, generics, valid/invalid Rust). A category absent from
    /// this entry's checkout is recorded as `0`, not omitted, so the
    /// report always names every tracked category.
    pub category_counts: Vec<(String, usize)>,
    /// The pinned revision (`corpus.lock`'s `tag`/`commit`), as displayed
    /// (e.g. `"tag 1.98.1"`).
    pub revision: String,
    /// The commit this entry's checkout actually resolved to, when
    /// `cargo xtask corpus fetch` recorded one (F8). `None` for a
    /// checkout that predates that field, or that was never fetched
    /// through this code path.
    pub resolved_commit: Option<String>,
}

/// The full result of one `corpus test` run.
#[derive(Debug, Clone, Default)]
pub struct Summary {
    /// Per-corpus-entry file and category counts.
    pub entries: Vec<EntrySummary>,
    /// Total files scanned across every entry.
    pub files_scanned: usize,
    /// Files that panicked while parsing, or could not be read.
    pub panics: Vec<PanicRecord>,
    /// Files whose parse exceeded the per-file wall-clock budget.
    pub timeouts: Vec<TimeoutRecord>,
    /// Files that produced at least one Outou diagnostic.
    pub false_positives: Vec<FalsePositive>,
    /// Files on which a JSX element was detected at all.
    pub jsx_misdetections: Vec<JsxMisdetection>,
    /// Files whose splice round trip did not reproduce their source, with
    /// zero JSX detected.
    pub splice_mismatches: Vec<SpliceMismatch>,
    /// Unix timestamp (seconds) this run finished scanning, for
    /// provenance (F9).
    pub generated_at_unix: u64,
    /// The per-file wall-clock budget (`test::PER_FILE_BUDGET`) this run
    /// used, in seconds, for provenance (F9).
    pub per_file_budget_secs: u64,
}

/// How many files' worth of detail the printed markdown summary lists,
/// per section, before truncating (the full list always goes to
/// `.corpus/report.json`).
const MARKDOWN_FALSE_POSITIVE_LIMIT: usize = 20;
const MARKDOWN_ROUND_TRIP_LIMIT: usize = 10;

fn severity_str(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

pub fn to_json(summary: &Summary) -> Value {
    let false_positives_on_valid_rust = summary
        .false_positives
        .iter()
        .filter(|f| f.on_valid_rust)
        .count();
    let false_positives_on_rejected_rust =
        summary.false_positives.len() - false_positives_on_valid_rust;
    let false_positives_error_severity = summary
        .false_positives
        .iter()
        .filter(|f| f.first_severity == Severity::Error)
        .count();
    let false_positives_warning_severity =
        summary.false_positives.len() - false_positives_error_severity;

    json!({
        "provenance": {
            "generated_at_unix": summary.generated_at_unix,
            "per_file_budget_secs": summary.per_file_budget_secs,
        },
        "files_scanned": summary.files_scanned,
        "entries": summary.entries.iter().map(|e| json!({
            "name": e.name,
            "revision": e.revision,
            "resolved_commit": e.resolved_commit,
            "files_scanned": e.files_scanned,
            "category_counts": e.category_counts.iter().cloned().collect::<BTreeMap<String, usize>>(),
        })).collect::<Vec<_>>(),
        "panics": {
            "count": summary.panics.len(),
            "files": summary.panics.iter().map(|p| json!({
                "file": p.file,
                "message": p.message,
            })).collect::<Vec<_>>(),
        },
        "timeouts": {
            "count": summary.timeouts.len(),
            "files": summary.timeouts,
        },
        "false_positives": {
            "count": summary.false_positives.len(),
            "on_valid_rust": false_positives_on_valid_rust,
            "on_rejected_rust": false_positives_on_rejected_rust,
            "error_severity": false_positives_error_severity,
            "warning_severity": false_positives_warning_severity,
            "files": summary.false_positives.iter().map(|f| json!({
                "file": f.file,
                "diagnostic_count": f.diagnostic_count,
                "first_message": f.first_message,
                "first_severity": severity_str(f.first_severity),
                "on_valid_rust": f.on_valid_rust,
            })).collect::<Vec<_>>(),
        },
        "jsx_misdetections": {
            "count": summary.jsx_misdetections.len(),
            "files": summary.jsx_misdetections.iter().map(|m| json!({
                "file": m.file,
                "description": m.description,
            })).collect::<Vec<_>>(),
        },
        "splice_mismatches": {
            "count": summary.splice_mismatches.len(),
            "files": summary.splice_mismatches.iter().map(|m| json!({
                "file": m.file,
                "description": m.description,
            })).collect::<Vec<_>>(),
        },
    })
}

pub fn to_markdown(summary: &Summary) -> String {
    let false_positives_on_valid_rust = summary
        .false_positives
        .iter()
        .filter(|f| f.on_valid_rust)
        .count();

    let mut out = String::new();
    out.push_str("# Corpus test summary\n\n");
    out.push_str(&format!(
        "- generated at (unix): {}\n",
        summary.generated_at_unix
    ));
    out.push_str(&format!(
        "- per-file budget: {}s\n",
        summary.per_file_budget_secs
    ));
    for entry in &summary.entries {
        out.push_str(&format!(
            "- `{}` revision: {}{}\n",
            entry.name,
            entry.revision,
            entry
                .resolved_commit
                .as_deref()
                .map(|c| format!(" (resolved commit {c})"))
                .unwrap_or_default()
        ));
    }
    out.push_str(&format!("- files scanned: {}\n", summary.files_scanned));
    out.push_str(&format!("- panics: {}\n", summary.panics.len()));
    out.push_str(&format!("- timeouts: {}\n", summary.timeouts.len()));
    out.push_str(&format!(
        "- false positives (Outou diagnostics on plain Rust): {} ({} on valid Rust, {} on Rust rustc itself rejects)\n",
        summary.false_positives.len(),
        false_positives_on_valid_rust,
        summary.false_positives.len() - false_positives_on_valid_rust,
    ));
    out.push_str(&format!(
        "- JSX mis-detections (a JSX element found on plain Rust): {}\n",
        summary.jsx_misdetections.len()
    ));
    out.push_str(&format!(
        "- splice round-trip mismatches (a genuine splice-partitioning bug, no JSX detected): {}\n\n",
        summary.splice_mismatches.len()
    ));

    out.push_str("## Per-entry category counts\n\n");
    for entry in &summary.entries {
        out.push_str(&format!(
            "- `{}`: {} files\n",
            entry.name, entry.files_scanned
        ));
        for (category, count) in &entry.category_counts {
            out.push_str(&format!("  - {category}: {count}\n"));
        }
    }
    out.push('\n');

    if !summary.panics.is_empty() {
        out.push_str("## Panics\n\n");
        for record in &summary.panics {
            out.push_str(&format!("- `{}`: {}\n", record.file, record.message));
        }
        out.push('\n');
    }

    if !summary.timeouts.is_empty() {
        out.push_str("## Timeouts\n\n");
        for file in &summary.timeouts {
            out.push_str(&format!("- `{file}`\n"));
        }
        out.push('\n');
    }

    if !summary.false_positives.is_empty() {
        out.push_str(&format!(
            "## False positives (first {})\n\n",
            MARKDOWN_FALSE_POSITIVE_LIMIT.min(summary.false_positives.len())
        ));
        for record in summary
            .false_positives
            .iter()
            .take(MARKDOWN_FALSE_POSITIVE_LIMIT)
        {
            out.push_str(&format!(
                "- `{}` ({} diagnostic(s)): {}\n",
                record.file, record.diagnostic_count, record.first_message
            ));
        }
        out.push('\n');
    }

    if !summary.jsx_misdetections.is_empty() {
        out.push_str(&format!(
            "## JSX mis-detections (first {})\n\n",
            MARKDOWN_ROUND_TRIP_LIMIT.min(summary.jsx_misdetections.len())
        ));
        for record in summary
            .jsx_misdetections
            .iter()
            .take(MARKDOWN_ROUND_TRIP_LIMIT)
        {
            out.push_str(&format!("- `{}`: {}\n", record.file, record.description));
        }
        out.push('\n');
    }

    if !summary.splice_mismatches.is_empty() {
        out.push_str(&format!(
            "## Splice round-trip mismatches (first {})\n\n",
            MARKDOWN_ROUND_TRIP_LIMIT.min(summary.splice_mismatches.len())
        ));
        for record in summary
            .splice_mismatches
            .iter()
            .take(MARKDOWN_ROUND_TRIP_LIMIT)
        {
            out.push_str(&format!("- `{}`: {}\n", record.file, record.description));
        }
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_truncates_but_json_keeps_everything() {
        let mut summary = Summary::default();
        for i in 0..30 {
            summary.false_positives.push(FalsePositive {
                file: format!("file{i}.rs"),
                diagnostic_count: 1,
                first_message: "diag".to_string(),
                first_severity: Severity::Error,
                on_valid_rust: true,
            });
        }
        let markdown = to_markdown(&summary);
        assert_eq!(
            markdown.matches("- `file").count(),
            MARKDOWN_FALSE_POSITIVE_LIMIT
        );

        let json = to_json(&summary);
        assert_eq!(json["false_positives"]["count"], 30);
        assert_eq!(json["false_positives"]["on_valid_rust"], 30);
        assert_eq!(json["false_positives"]["on_rejected_rust"], 0);
        assert_eq!(json["false_positives"]["error_severity"], 30);
        assert_eq!(json["false_positives"]["warning_severity"], 0);
        assert_eq!(
            json["false_positives"]["files"].as_array().unwrap().len(),
            30
        );
    }

    /// F6 (issue #12 corpus review, MEDIUM): a Gate 4 report must
    /// distinguish false positives on valid Rust from ones on Rust rustc
    /// itself already rejects, and separate error- from warning-severity
    /// ones — not one undifferentiated bucket.
    #[test]
    fn false_positives_split_by_validity_and_severity() {
        let mut summary = Summary::default();
        summary.false_positives.push(FalsePositive {
            file: "valid_error.rs".to_string(),
            diagnostic_count: 1,
            first_message: "error diag".to_string(),
            first_severity: Severity::Error,
            on_valid_rust: true,
        });
        summary.false_positives.push(FalsePositive {
            file: "rejected_warning.rs".to_string(),
            diagnostic_count: 1,
            first_message: "warning diag".to_string(),
            first_severity: Severity::Warning,
            on_valid_rust: false,
        });

        let json = to_json(&summary);
        assert_eq!(json["false_positives"]["count"], 2);
        assert_eq!(json["false_positives"]["on_valid_rust"], 1);
        assert_eq!(json["false_positives"]["on_rejected_rust"], 1);
        assert_eq!(json["false_positives"]["error_severity"], 1);
        assert_eq!(json["false_positives"]["warning_severity"], 1);
    }

    /// F7 (issue #12 corpus review, MEDIUM): a JSX mis-detection and a
    /// genuine splice-partitioning bug are reported as two separate
    /// fields, not folded into one "round-trip mismatches" bucket.
    #[test]
    fn jsx_misdetections_and_splice_mismatches_are_reported_separately() {
        let mut summary = Summary::default();
        summary.jsx_misdetections.push(JsxMisdetection {
            file: "a.rs".to_string(),
            description: "JSX element detected at 1:1".to_string(),
        });
        summary.splice_mismatches.push(SpliceMismatch {
            file: "b.rs".to_string(),
            description: "splice round trip did not reproduce the source".to_string(),
        });

        let json = to_json(&summary);
        assert_eq!(json["jsx_misdetections"]["count"], 1);
        assert_eq!(json["splice_mismatches"]["count"], 1);

        let markdown = to_markdown(&summary);
        assert!(markdown.contains("JSX mis-detections"));
        assert!(markdown.contains("Splice round-trip mismatches"));
    }

    /// F9 (issue #12 corpus review, MEDIUM): a report must carry enough
    /// provenance (revision, resolved commit, when it ran, the per-file
    /// budget) to tie its numbers to something.
    #[test]
    fn json_carries_provenance() {
        let mut summary = Summary {
            generated_at_unix: 1_700_000_000,
            per_file_budget_secs: 2,
            ..Summary::default()
        };
        summary.entries.push(EntrySummary {
            name: "rust-ui-tests".to_string(),
            files_scanned: 1,
            category_counts: vec![],
            revision: "tag 1.98.1".to_string(),
            resolved_commit: Some("48a229ceaefd4985c50990b14116b6d856af0985".to_string()),
        });

        let json = to_json(&summary);
        assert_eq!(json["provenance"]["generated_at_unix"], 1_700_000_000);
        assert_eq!(json["provenance"]["per_file_budget_secs"], 2);
        assert_eq!(json["entries"][0]["revision"], "tag 1.98.1");
        assert_eq!(
            json["entries"][0]["resolved_commit"],
            "48a229ceaefd4985c50990b14116b6d856af0985"
        );

        let markdown = to_markdown(&summary);
        assert!(markdown.contains("tag 1.98.1"));
        assert!(markdown.contains("48a229ceaefd4985c50990b14116b6d856af0985"));
    }

    #[test]
    fn category_counts_serialize_even_when_zero() {
        let mut summary = Summary::default();
        summary.entries.push(EntrySummary {
            name: "rust-ui-tests".to_string(),
            files_scanned: 0,
            category_counts: vec![("macros".to_string(), 0)],
            revision: "tag 1.98.1".to_string(),
            resolved_commit: Some("48a229c".to_string()),
        });
        let json = to_json(&summary);
        assert_eq!(json["entries"][0]["category_counts"]["macros"], 0);
    }
}
