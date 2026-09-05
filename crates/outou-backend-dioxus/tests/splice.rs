//! Splice invariant (issue #6 fix list item 1, CRITICAL-1 regression
//! test): Strict-mode codegen must never drop source bytes.
//!
//! Grammar §9's round-trip contract says the AST always makes the
//! original source reconstructible by splicing each node's text in
//! order. Codegen's `Mode::Strict` output must uphold the *codegen* half
//! of that same idea: every byte of the input that a JSX element's own
//! span does not cover must reappear, verbatim and in order, somewhere in
//! the generated Rust — never silently dropped, whether or not the
//! parser happened to also emit a diagnostic for the file.
//!
//! This is checked for every fixture under `tests/fixtures/{formatting,
//! incomplete,modules}` (recursively) and `examples/phase0-app`, plus
//! `tests/fixtures/incomplete/bodyless-fn-mid-file.rsx`, which is the
//! actual regression case for CRITICAL-1: a body-less `fn App(` mid-file
//! that swallows the rest of the file into its own signature text, with
//! *no* parser diagnostic at all (`bodyless-fn-mid-file.expected` is
//! empty) — so the `Mode::Strict` gate (`reject_syntax_errors_in_strict_mode`)
//! does not refuse generation, and codegen itself was the only thing
//! standing between this input and silently losing every function after
//! `App`.
//!
//! A fixture whose diagnostics *do* include an error (most of
//! `incomplete/`, several of `modules/`) makes `DioxusBackend::generate`
//! return `Error::SyntaxErrors` for `Mode::Strict` before codegen ever
//! runs; there is no generated output to check the invariant against, so
//! those fixtures are skipped rather than asserted on directly. The
//! invariant is still applied uniformly — the skip is a consequence of
//! the shared gate, not a fixture-specific carve-out.

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use outou_backend_dioxus::DioxusBackend;
use outou_codegen::{Backend, Error, GenerateOptions, Mode};
use outou_sourcemap::{Span, Uri};
use outou_syntax::ast;

/// Every top-level `Expr::Jsx` span reachable from `file`: the exact set
/// of ranges codegen replaces with generated Dioxus syntax rather than
/// splicing verbatim. A JSX element's own attributes/children (including
/// any nested elements) live entirely inside its span, so they are never
/// walked into separately here — removing the outer span already removes
/// everything nested in it.
fn collect_jsx_spans(file: &ast::File, out: &mut Vec<Span>) {
    for item in &file.items {
        collect_item(item, out);
    }
}

fn collect_item(item: &ast::Item, out: &mut Vec<Span>) {
    match item {
        ast::Item::Function(f) => collect_block(&f.body, out),
        ast::Item::Module(m) => {
            if let Some(items) = &m.items {
                for item in items {
                    collect_item(item, out);
                }
            }
        }
        ast::Item::Rust(r) => collect_exprs(&r.parts, out),
        ast::Item::Error(_) => {}
    }
}

fn collect_block(block: &ast::Block, out: &mut Vec<Span>) {
    collect_exprs(&block.statements, out);
    if let Some(tail) = &block.tail {
        collect_expr(tail, out);
    }
}

fn collect_exprs(exprs: &[ast::Expr], out: &mut Vec<Span>) {
    for expr in exprs {
        collect_expr(expr, out);
    }
}

fn collect_expr(expr: &ast::Expr, out: &mut Vec<Span>) {
    if let ast::Expr::Jsx(element) = expr {
        out.push(element.span);
    }
}

/// The source, split at every collected JSX span, into the chunks that
/// must survive verbatim: everything *outside* a top-level JSX element.
fn chunks_outside_jsx(source: &str, mut spans: Vec<Span>) -> Vec<String> {
    spans.sort_by_key(|s| s.start);
    let mut chunks = Vec::new();
    let mut cursor = 0u32;
    for span in spans {
        let start = span.start.max(cursor);
        if start > cursor {
            chunks.push(source[cursor as usize..start as usize].to_string());
        }
        cursor = span.end.max(cursor);
    }
    if (cursor as usize) < source.len() {
        chunks.push(source[cursor as usize..].to_string());
    }
    chunks
}

fn assert_splice_invariant(label: &str, source: &str) {
    let parsed = outou_syntax::parse(source);
    let mut spans = Vec::new();
    collect_jsx_spans(&parsed.file, &mut spans);
    let chunks = chunks_outside_jsx(source, spans);

    let opts = GenerateOptions::new(Uri::new("file:///gen.rs"), Uri::new("file:///a.rsx"));
    let generated = match DioxusBackend.generate(&parsed, source, Mode::Strict, &opts) {
        Ok(generated) => generated,
        // Strict generation is refused whenever the parser reported an
        // error diagnostic; there is no generated output to check, and
        // that refusal is itself the correct behavior for those cases.
        Err(Error::SyntaxErrors { .. }) => return,
        Err(other) => panic!("{label}: unexpected codegen error: {other}"),
    };

    let mut cursor = 0usize;
    for chunk in &chunks {
        if chunk.is_empty() {
            continue;
        }
        match generated.rust[cursor..].find(chunk.as_str()) {
            Some(offset) => cursor += offset + chunk.len(),
            None => panic!(
                "{label}: source chunk {chunk:?} is missing (or out of order) in the \
                 generated Strict-mode output:\n---\n{}",
                generated.rust
            ),
        }
    }
}

fn collect_rsx_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            collect_rsx_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rsx") {
            out.push(path);
        }
    }
}

#[test]
fn strict_mode_never_drops_source_bytes() {
    let root = support::repo_root();
    let mut paths = Vec::new();

    // `tests/fixtures/formatting/*/input.rsx`
    let formatting_dir = root.join("tests/fixtures/formatting");
    for entry in fs::read_dir(&formatting_dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", formatting_dir.display()))
        .filter_map(|e| e.ok())
    {
        let input = entry.path().join("input.rsx");
        if input.is_file() {
            paths.push(input);
        }
    }

    // `tests/fixtures/incomplete/*.rsx` (includes the CRITICAL-1
    // regression fixture, `bodyless-fn-mid-file.rsx`).
    collect_rsx_files(&root.join("tests/fixtures/incomplete"), &mut paths);

    // `tests/fixtures/modules/**/*.rsx`
    collect_rsx_files(&root.join("tests/fixtures/modules"), &mut paths);

    // `examples/phase0-app/src/*.rsx`
    collect_rsx_files(&root.join("examples/phase0-app/src"), &mut paths);

    assert!(!paths.is_empty(), "no fixtures found to check");

    let mut checked_bodyless_regression = false;
    for path in &paths {
        let source =
            fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        let label = path.display().to_string();
        assert_splice_invariant(&label, &source);
        if path.ends_with("bodyless-fn-mid-file.rsx") {
            checked_bodyless_regression = true;
            // The whole point of this fixture: no parser diagnostic at
            // all, so the Strict gate does not refuse generation, and
            // `Before`/`App`/`After` must all still be present verbatim.
            let parsed = outou_syntax::parse(&source);
            assert!(
                parsed.diagnostics.is_empty(),
                "bodyless-fn-mid-file.rsx unexpectedly has diagnostics: {:?}",
                parsed.diagnostics
            );
            let opts = GenerateOptions::new(Uri::new("file:///gen.rs"), Uri::new("file:///a.rsx"));
            let generated = DioxusBackend
                .generate(&parsed, &source, Mode::Strict, &opts)
                .expect("strict generation must succeed: no error diagnostics were emitted");
            assert!(generated.rust.contains("fn Before"), "{}", generated.rust);
            assert!(generated.rust.contains("fn App"), "{}", generated.rust);
            assert!(generated.rust.contains("fn After"), "{}", generated.rust);
        }
    }
    assert!(
        checked_bodyless_regression,
        "tests/fixtures/incomplete/bodyless-fn-mid-file.rsx was not found"
    );
}
