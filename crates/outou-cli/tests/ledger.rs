//! Guard against `docs/backend-leakage.md`'s table being split into
//! separate GFM paragraphs by a stray blank line between rows.
//!
//! A Markdown table is only recognized by GFM renderers while every row
//! is on a contiguous run of lines directly under the header/separator
//! pair; a blank line anywhere inside that run ends the table and turns
//! every following `| ... |` line into a literal paragraph of pipes
//! instead of a rendered row. `AGENTS.md` says the ledger's rows "are
//! appended, never rewritten" — this test makes a future append that
//! accidentally leaves (or inserts) a blank line before the new row fail
//! loudly, rather than silently breaking rendering for every row after
//! it.

use std::fs;
use std::path::PathBuf;

/// The exact header row `docs/backend-leakage.md`'s table starts with.
const HEADER_ROW: &str = "| # | Constraint |";

fn ledger_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/backend-leakage.md")
        .canonicalize()
        .expect("docs/backend-leakage.md exists relative to CARGO_MANIFEST_DIR")
}

#[test]
fn ledger_table_rows_are_contiguous_and_numbered_in_order() {
    let path = ledger_path();
    let text =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let lines: Vec<&str> = text.lines().collect();

    let header_idx = lines
        .iter()
        .position(|line| line.starts_with(HEADER_ROW))
        .unwrap_or_else(|| {
            panic!(
                "header row starting with {HEADER_ROW:?} not found in {}",
                path.display()
            )
        });

    // The line directly under the header is the GFM alignment/separator
    // row (`|---|---|...`); it must be present and itself part of the
    // same contiguous block as the header, or there is no table at all.
    let separator_idx = header_idx + 1;
    let separator = lines.get(separator_idx).unwrap_or_else(|| {
        panic!(
            "expected a separator row directly under {HEADER_ROW:?} at line {}",
            separator_idx + 1
        )
    });
    assert!(
        separator.starts_with('|'),
        "line {} directly under the header must be the `|---|...|` separator row, got: {separator:?}",
        separator_idx + 1
    );

    // Every row from just after the separator up to the first blank
    // line is the table body: this is the run a GFM renderer will
    // actually treat as one table. Collect it and stop at the first
    // blank line, so a row appended after a stray blank line is simply
    // excluded here -- and then caught by the row-numbering check below.
    let body_start = separator_idx + 1;
    let mut body: Vec<&str> = Vec::new();
    for line in &lines[body_start..] {
        if line.trim().is_empty() {
            break;
        }
        body.push(line);
    }

    assert!(
        !body.is_empty(),
        "expected at least one table row after the header/separator at line {}",
        separator_idx + 1
    );

    for (offset, line) in body.iter().enumerate() {
        assert!(
            line.starts_with('|'),
            "line {} (row {} of the table body) must start with `|` to stay inside the same GFM table as the header; found: {line:?}",
            body_start + offset + 1,
            offset + 1
        );
    }

    // Parse each row's leading `| N |` cell and require the numbers to
    // be exactly 1..=N with no gaps and no repeats -- a row separated
    // from the rest by a blank line (this test's real target) is simply
    // missing from `body`, which shows up here as a break in the
    // sequence rather than `body.len()` matching the ledger's true
    // highest row number.
    let row_numbers: Vec<usize> = body
        .iter()
        .map(|line| {
            let cell = line
                .trim_start_matches('|')
                .split('|')
                .next()
                .unwrap_or_default()
                .trim();
            cell.parse::<usize>().unwrap_or_else(|e| {
                panic!("row {line:?}: leading `| N |` cell {cell:?} is not a plain integer: {e}")
            })
        })
        .collect();

    let expected: Vec<usize> = (1..=row_numbers.len()).collect();
    assert_eq!(
        row_numbers, expected,
        "table body rows must be numbered 1..={} with no gaps; a gap here means a row is separated \
         from the rest of the table by a blank line (or the numbering itself skipped/repeated a value)",
        row_numbers.len()
    );

    // The check above only ever looks at the contiguous body, so a row
    // that got separated from the table by a blank line (this test's
    // actual target) is simply absent from `body` -- it would NOT show
    // up as a gap in `row_numbers`, since `body` just ends before it.
    // Catch that case directly: scan every line in the rest of the file
    // for anything shaped like a numbered row cell, and require that the
    // highest such number is exactly `body.len()` -- i.e. every numbered
    // row in the file is inside the contiguous body, none of them sit
    // past a blank line.
    let max_row_number_anywhere = lines[body_start..]
        .iter()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with('|') {
                return None;
            }
            let cell = trimmed.trim_start_matches('|').split('|').next()?.trim();
            cell.parse::<usize>().ok()
        })
        .max();

    assert_eq!(
        max_row_number_anywhere,
        Some(row_numbers.len()),
        "found a numbered row (`| N |`) later in {} whose number exceeds the last row of the \
         contiguous table body (row {}); that row is separated from the table by a blank line and \
         renders as a stray paragraph instead of a table row",
        path.display(),
        row_numbers.len()
    );
}
