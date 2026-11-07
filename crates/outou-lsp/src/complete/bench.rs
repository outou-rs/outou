//! Completion cost on a large workspace (#31 L15). Ignored by default; run
//! with
//!
//! ```text
//! cargo test -p outou-lsp --release complete::bench -- --ignored --nocapture
//! ```
//!
//! It times tag-name and attribute-name completion over 200 generated `.rsx`
//! files, against a baseline that re-parses every file per request exactly as
//! this module did before the parse moved into `RsxDocument`, and reports
//! the resident memory of the cached per-document data. The "before"
//! replica skips building the JSON completion items, so the speedup ratio it
//! prints is understated.
//! (The 30x memory figure for keeping parse trees was measured once, by
//! temporarily holding them; it is not reproduced here.) Timing is printed, never asserted.

use std::time::Instant;

use super::*;
use crate::documents::{RsxDocument, Workspace};

const FILES: usize = 200;
const REQUESTS: usize = 200;
/// Enough documents for a resident-set difference to rise above noise.
const MEMORY_FILES: usize = 3000;

/// Resident set size of this process in KiB (`ps`; the crate forbids the
/// `unsafe` a counting allocator would need).
fn rss_kib() -> usize {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .expect("ps runs");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .expect("ps prints a number")
}

fn generated_source(index: usize) -> String {
    let mut text = String::new();
    for n in 0..3 {
        text.push_str(&format!(
            "#[component]\npub fn Card{index}x{n}(title: String, items: Vec<String>, r#type: u32) -> Element {{\n    \
             let count = items.len();\n    let label = format!(\"{{}}-{{}}\", title, count);\n    \
             <section class=\"card\" id=\"c{n}\">\n        <h2>{{title}}</h2>\n        <ul>\n            \
             <li>{{label}}</li>\n            <li>{{count}}</li>\n        </ul>\n        <p>text</p>\n    </section>\n}}\n\n"
        ));
    }
    text
}

fn workspace(files: usize) -> (Workspace, usize) {
    let mut workspace = Workspace {
        manifest_dir: std::path::PathBuf::from("/app"),
        plan: None,
        registry: outou_sourcemap::Registry::new(),
        rsx: std::collections::HashMap::new(),
        generated: std::collections::HashMap::new(),
        rsx_to_generated: std::collections::HashMap::new(),
        plan_error: None,
    };
    let mut bytes = 0;
    for index in 0..files {
        let text = generated_source(index);
        bytes += text.len();
        workspace.rsx.insert(
            format!("file:///app/src/m{index}.rsx"),
            RsxDocument::new(text, 0),
        );
    }
    (workspace, bytes)
}

/// What tag-name and attribute-name completion did before: parse every
/// document for each request.
fn legacy_labels(workspace: &Workspace, partial: &str) -> usize {
    let mut labels: Vec<String> = Vec::new();
    for doc in workspace.rsx.values() {
        let parsed = crate::documents::parse_rsx(doc.line_index.text());
        for function in crate::plan::component_functions(&parsed.file) {
            if function.name.name.starts_with(partial) && !labels.contains(&function.name.name) {
                labels.push(function.name.name.clone());
            }
        }
    }
    labels.len()
}

fn median_micros(mut run: impl FnMut()) -> f64 {
    let mut samples: Vec<f64> = (0..REQUESTS)
        .map(|_| {
            let start = Instant::now();
            run();
            start.elapsed().as_secs_f64() * 1e6
        })
        .collect();
    samples.sort_by(|a, b| a.total_cmp(b));
    samples[samples.len() / 2]
}

#[test]
#[ignore = "measurement, run explicitly (see the module docs)"]
fn completion_cost_over_many_files() {
    let (mut workspace, bytes) = workspace(FILES);

    // The current file: tag position `<Card1` in file 0, which also holds
    // `<h2>` etc. for the attribute case.
    let current = "file:///app/src/current.rsx".to_string();
    let source = "#[component]\nfn App() -> Element {\n    <div><Card1</div>\n}\n";
    workspace
        .rsx
        .insert(current.clone(), RsxDocument::new(source.to_string(), 1));
    let offset = (source.find("<Card1").unwrap() + "<Card1".len()) as u32;
    let range = workspace.rsx[&current]
        .line_index
        .span_to_range(outou_sourcemap::Span::new(offset, offset));
    let position = lsp_types::Position::new(range.start.line, range.start.character);

    let tag_now = median_micros(|| {
        assert!(local_completion(&workspace, &current, position).is_some());
    });
    let tag_before = median_micros(|| {
        // The pre-change request: `local_completion` parsed the current
        // document, then the file scan parsed every document again.
        let parsed = crate::documents::parse_rsx(workspace.rsx[&current].line_index.text());
        assert!(matches!(
            classify(&parsed.file, source, offset),
            Cursor::TagName { .. }
        ));
        assert!(legacy_labels(&workspace, "Card1") > 0);
    });

    let edit_text = generated_source(0);
    let edit = median_micros(|| {
        drop(RsxDocument::new(edit_text.clone(), 2));
    });

    println!("files: {FILES} ({} KiB of .rsx source)", bytes / 1024);
    println!("tag-name completion per request, before: {tag_before:.0} us");
    println!("tag-name completion per request, after:  {tag_now:.1} us");
    println!("one edit (building one RsxDocument):      {edit:.0} us");
}

/// Resident memory of `MEMORY_FILES` documents, against the same texts held
/// as plain strings (the baseline).
#[test]
#[ignore = "measurement, run explicitly (see the module docs)"]
fn memory_held_per_document() {
    let texts: Vec<String> = (0..MEMORY_FILES).map(generated_source).collect();
    let bytes: usize = texts.iter().map(String::len).sum();
    let base = rss_kib();
    let plain: Vec<String> = texts.clone();
    let plain_cost = rss_kib() - base;
    drop(plain);
    let base = rss_kib();
    let docs: Vec<RsxDocument> = texts
        .iter()
        .map(|text| RsxDocument::new(text.clone(), 0))
        .collect();
    let docs_cost = rss_kib() - base;
    println!(
        "{MEMORY_FILES} documents, {} KiB of source: strings alone ~{plain_cost} KiB, \
         full RsxDocument (text, line index, diagnostics, components) ~{docs_cost} KiB",
        bytes / 1024
    );
    assert_eq!(docs.len(), MEMORY_FILES);
}
