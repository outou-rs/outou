//! Building the `.corpus/report.json` document and the markdown summary
//! `corpus test` prints to stdout.

use std::collections::BTreeMap;

use serde_json::{json, Value};

/// One panic or unreadable file found while testing a corpus.
#[derive(Debug, Clone)]
pub struct PanicRecord {
    pub file: String,
    pub message: String,
}

/// A file whose parse exceeded the per-file wall-clock budget.
pub type TimeoutRecord = String;

/// A file that produced at least one Outou diagnostic. Since every
/// corpus file is plain Rust, any diagnostic on it is a false positive,
/// not a real failure — reported for count and inspection, not as a
/// failure by default.
#[derive(Debug, Clone)]
pub struct FalsePositive {
    pub file: String,
    pub diagnostic_count: usize,
    pub first_message: String,
}

/// A file whose splice round trip did not reproduce its source, or in
/// which a JSX element was found at all (itself a mis-detection on plain
/// Rust input).
#[derive(Debug, Clone)]
pub struct RoundTripMismatch {
    pub file: String,
    pub description: String,
}

/// Per-corpus-entry file and category counts.
#[derive(Debug, Clone)]
pub struct EntrySummary {
    pub name: String,
    pub files_scanned: usize,
    /// Counts per tracked `tests/ui` subdirectory category (issue #12's
    /// checklist: macro token trees, qualified paths, raw strings,
    /// lifetimes, generics, valid/invalid Rust). A category absent from
    /// this entry's checkout is recorded as `0`, not omitted, so the
    /// report always names every tracked category.
    pub category_counts: Vec<(String, usize)>,
}

/// The full result of one `corpus test` run.
#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub entries: Vec<EntrySummary>,
    pub files_scanned: usize,
    pub panics: Vec<PanicRecord>,
    pub timeouts: Vec<TimeoutRecord>,
    pub false_positives: Vec<FalsePositive>,
    pub round_trip_mismatches: Vec<RoundTripMismatch>,
}

/// How many files' worth of detail the printed markdown summary lists,
/// per section, before truncating (the full list always goes to
/// `.corpus/report.json`).
const MARKDOWN_FALSE_POSITIVE_LIMIT: usize = 20;
const MARKDOWN_ROUND_TRIP_LIMIT: usize = 10;

pub fn to_json(summary: &Summary) -> Value {
    json!({
        "files_scanned": summary.files_scanned,
        "entries": summary.entries.iter().map(|e| json!({
            "name": e.name,
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
            "files": summary.false_positives.iter().map(|f| json!({
                "file": f.file,
                "diagnostic_count": f.diagnostic_count,
                "first_message": f.first_message,
            })).collect::<Vec<_>>(),
        },
        "round_trip_mismatches": {
            "count": summary.round_trip_mismatches.len(),
            "files": summary.round_trip_mismatches.iter().map(|m| json!({
                "file": m.file,
                "description": m.description,
            })).collect::<Vec<_>>(),
        },
    })
}

pub fn to_markdown(summary: &Summary) -> String {
    let mut out = String::new();
    out.push_str("# Corpus test summary\n\n");
    out.push_str(&format!("- files scanned: {}\n", summary.files_scanned));
    out.push_str(&format!("- panics: {}\n", summary.panics.len()));
    out.push_str(&format!("- timeouts: {}\n", summary.timeouts.len()));
    out.push_str(&format!(
        "- false positives (Outou diagnostics on plain Rust): {}\n",
        summary.false_positives.len()
    ));
    out.push_str(&format!(
        "- round-trip mismatches (including any JSX mis-detection): {}\n\n",
        summary.round_trip_mismatches.len()
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

    if !summary.round_trip_mismatches.is_empty() {
        out.push_str(&format!(
            "## Round-trip mismatches (first {})\n\n",
            MARKDOWN_ROUND_TRIP_LIMIT.min(summary.round_trip_mismatches.len())
        ));
        for record in summary
            .round_trip_mismatches
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
            });
        }
        let markdown = to_markdown(&summary);
        assert_eq!(
            markdown.matches("- `file").count(),
            MARKDOWN_FALSE_POSITIVE_LIMIT
        );

        let json = to_json(&summary);
        assert_eq!(json["false_positives"]["count"], 30);
        assert_eq!(
            json["false_positives"]["files"].as_array().unwrap().len(),
            30
        );
    }

    #[test]
    fn category_counts_serialize_even_when_zero() {
        let mut summary = Summary::default();
        summary.entries.push(EntrySummary {
            name: "rust-ui-tests".to_string(),
            files_scanned: 0,
            category_counts: vec![("macros".to_string(), 0)],
        });
        let json = to_json(&summary);
        assert_eq!(json["entries"][0]["category_counts"]["macros"], 0);
    }
}
