//! Recovery tests (issue #6, deliverable 5c): every `tests/fixtures/
//! incomplete/*.rsx` fixture, generated in Recovery mode, must produce
//! text that (1) parses as a Rust file (checked with `syn`, a dev-only
//! dependency — never a normal one) and (2) still contains, verbatim,
//! every complete Rust expression the input had (an incomplete
//! construct — the ones this whole fixture set exists to exercise — is
//! never counted, since it was never a complete expression to begin
//! with).
//!
//! **`syn` proves less than it looks like it does (HIGH-4, issue #6 fix
//! list item 6).** `syn::parse_file` only has to balance token trees; it
//! never validates the *content* of a macro body, and every JSX element
//! this crate lowers lives inside `::outou::__private::rsx! { … }`. A
//! recovery-mode output can pass `syn::parse_file` and still fail to
//! `cargo check` — reproduced with `unclosed-island-mid-file.rsx` below,
//! whose swallowed tail (`fn After`, absorbed into a JSX text child by
//! the parser's own recovery, issue #4) becomes unparseable Rust once
//! `rsx!` actually expands it. [`every_incomplete_fixture_compiles_with_no_parse_errors`]
//! is the real compile gate this crate needs: it reuses
//! `tests/compile_check.rs`'s throwaway-crate machinery to run a genuine
//! `cargo check` over each fixture's recovery output and asserts "no
//! *parse* error; type/name-resolution errors are permitted" — recovery
//! output only has to stay *analyzable*, never to type-check, since it
//! is for rust-analyzer, not `rustc` (see `crate` docs and
//! `crates/outou-backend-dioxus/README.md`). It is `#[ignore]`d for the
//! same reason `compile_check.rs` is: building the full `dioxus`
//! dependency tree from a cold `target/` is slow. Run explicitly with:
//!
//! ```sh
//! cargo test -p outou-backend-dioxus --test recovery -- --ignored --nocapture
//! ```
//!
//! `unclosed-tag-mid-file.rsx`, `class-attribute-value-mid-file.rsx` and
//! `unclosed-island-mid-file.rsx` are the mid-edit fixtures this item
//! adds: the same broken shapes as `unterminated-attribute-name.rsx` /
//! `unterminated-attribute-value.rsx`, but placed mid-file (a `fn Before`
//! before, a `fn After` after) rather than at the literal end of input,
//! so a real edit session — broken syntax with more of the file typed
//! below it — is exercised, not just "stopped typing at EOF".
//! `unclosed-island-mid-file.rsx` is the one that actually demonstrates
//! the symbol-loss HIGH-4 describes: `fn After` is not dropped as an
//! `Item` (nothing this crate's codegen decides to drop) — the *parser*
//! itself, recovering from the unclosed `{items.iter()…}` island, folds
//! `fn After`'s entire text into a `JsxText` child of `App`'s `<ul>`
//! before codegen ever sees the file, so the symbol is unrecoverable by
//! construction, not by a codegen choice. This is issue #4's
//! element-runs-to-EOF recovery, not something `outou-backend-dioxus`
//! can fix; it is documented here, not fixed here.

mod support;

use std::fs;
use std::process::Command;

use outou_backend_dioxus::DioxusBackend;
use outou_codegen::{Backend, GenerateOptions, Mode};
use outou_sourcemap::Uri;
use outou_syntax::ast;

fn fixture_paths() -> Vec<std::path::PathBuf> {
    let dir = support::repo_root().join("tests/fixtures/incomplete");
    let mut paths: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("rsx"))
        .collect();
    paths.sort();
    assert!(
        !paths.is_empty(),
        "no incomplete fixtures found under {}",
        dir.display()
    );
    paths
}

/// Every `Expr::Rust` slice reachable from `file`: the exact set of
/// "complete Rust expressions" this test requires to survive recovery
/// mode verbatim. A broken construct never produces an `Expr::Rust` (it
/// becomes an `ErrorNode` instead), so this walk naturally excludes the
/// incomplete part of each fixture without needing to special-case it.
fn collect_rust_texts(file: &ast::File) -> Vec<String> {
    let mut out = Vec::new();
    for item in &file.items {
        collect_item(item, &mut out);
    }
    out
}

fn collect_item(item: &ast::Item, out: &mut Vec<String>) {
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

fn collect_block(block: &ast::Block, out: &mut Vec<String>) {
    collect_exprs(&block.statements, out);
    if let Some(tail) = &block.tail {
        collect_expr(tail, out);
    }
}

fn collect_exprs(exprs: &[ast::Expr], out: &mut Vec<String>) {
    for expr in exprs {
        collect_expr(expr, out);
    }
}

fn collect_expr(expr: &ast::Expr, out: &mut Vec<String>) {
    match expr {
        ast::Expr::Rust(rust) => out.push(rust.text.clone()),
        ast::Expr::Jsx(element) => collect_element(element, out),
        ast::Expr::Error(_) => {}
    }
}

fn collect_element(element: &ast::JsxElement, out: &mut Vec<String>) {
    for attribute in &element.attributes {
        if let Some(ast::JsxAttributeValue::Expression(island)) = &attribute.value {
            collect_exprs(&island.parts, out);
        }
    }
    for child in &element.children {
        match child {
            ast::JsxChild::Expression(island) => collect_exprs(&island.parts, out),
            ast::JsxChild::Element(nested) => collect_element(nested, out),
            ast::JsxChild::Text(_) | ast::JsxChild::Error(_) => {}
        }
    }
}

#[test]
fn every_incomplete_fixture_produces_parseable_analyzable_rust() {
    for path in fixture_paths() {
        let source =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        let parsed = outou_syntax::parse(&source);
        let opts = GenerateOptions::new(Uri::new("file:///gen.rs"), Uri::new("file:///a.rsx"));
        let generated = DioxusBackend
            .generate(&parsed, &source, Mode::Recovery, &opts)
            .unwrap_or_else(|e| {
                panic!(
                    "{}: recovery generation must never fail: {e}",
                    path.display()
                )
            });

        syn::parse_file(&generated.rust).unwrap_or_else(|e| {
            panic!(
                "{}: recovery output does not parse as Rust: {e}\n---\n{}",
                path.display(),
                generated.rust
            )
        });

        for text in collect_rust_texts(&parsed.file) {
            assert!(
                generated.rust.contains(&text),
                "{}: complete Rust {:?} is missing from recovery output:\n{}",
                path.display(),
                text,
                generated.rust
            );
        }

        assert!(
            !generated.rust.contains("dioxus"),
            "{}: recovery output contains the substring \"dioxus\":\n{}",
            path.display(),
            generated.rust
        );
    }
}

#[test]
fn recovery_generation_is_deterministic() {
    for path in fixture_paths() {
        let source =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        let parsed = outou_syntax::parse(&source);
        let opts = GenerateOptions::new(Uri::new("file:///gen.rs"), Uri::new("file:///a.rsx"));
        let first = DioxusBackend
            .generate(&parsed, &source, Mode::Recovery, &opts)
            .unwrap();
        let second = DioxusBackend
            .generate(&parsed, &source, Mode::Recovery, &opts)
            .unwrap();
        assert_eq!(first.rust, second.rust, "{}", path.display());
        assert_eq!(first.source_map, second.source_map, "{}", path.display());
    }
}

/// The real compile gate HIGH-4 asks for (issue #6 fix list item 6):
/// every `tests/fixtures/incomplete/*.rsx` fixture's Recovery-mode
/// output is written into a throwaway crate (as a library — recovery
/// output is a bare sequence of items, never a `fn main`) with `outou`
/// as a path dependency, `cargo check`ed with `--message-format=json`,
/// and asserted to produce no *parse*-level error diagnostic. Type and
/// name-resolution errors are permitted (and expected: several
/// fixtures reference a name, like `load_user`, that a truncated `fn
/// App` never got to define) — recovery output only has to stay
/// analyzable for rust-analyzer, never to type-check.
#[test]
#[ignore = "builds the full dioxus dependency tree; run explicitly, see module docs"]
fn every_incomplete_fixture_compiles_with_no_parse_errors() {
    let root = support::repo_root();
    let outou_path = root.join("crates/outou");
    let temp_dir = root.join("target/outou-recovery-compile-check");
    let src_dir = temp_dir.join("src");
    fs::create_dir_all(&src_dir).expect("creating temp crate src dir");

    let manifest = format!(
        "[package]\n\
         name = \"outou-recovery-compile-check\"\n\
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

    for path in fixture_paths() {
        let source =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        let parsed = outou_syntax::parse(&source);
        let opts = GenerateOptions::new(Uri::new("file:///gen/lib.rs"), Uri::new("file:///a.rsx"));
        let generated = DioxusBackend
            .generate(&parsed, &source, Mode::Recovery, &opts)
            .unwrap_or_else(|e| {
                panic!(
                    "{}: recovery generation must never fail: {e}",
                    path.display()
                )
            });
        fs::write(src_dir.join("lib.rs"), &generated.rust)
            .unwrap_or_else(|e| panic!("writing generated lib.rs for {}: {e}", path.display()));

        let output = Command::new(env!("CARGO"))
            .arg("check")
            .arg("--message-format=json")
            .current_dir(&temp_dir)
            .env("CARGO_TARGET_DIR", root.join("target"))
            .output()
            .unwrap_or_else(|e| panic!("running `cargo check` for {}: {e}", path.display()));

        let parse_errors = parse_error_messages(&output.stdout);
        assert!(
            parse_errors.is_empty(),
            "{}: recovery output has a parse error (type/name-resolution errors are \
             permitted):\n{}\n---generated---\n{}",
            path.display(),
            parse_errors.join("\n---\n"),
            generated.rust,
        );
    }
}

/// Extracts the rendered text of every `error`-level diagnostic from
/// `cargo check --message-format=json` output whose rustc `code` field
/// is absent — the signature of a *parse* error. A type or
/// name-resolution error (`E0308` mismatched types, `E0425` cannot find
/// value, `E0599` no method named, …) is assigned an `E`-code almost
/// without exception; the parser's own errors, reported before any code
/// is ever assigned, are not. This is the distinction HIGH-4 asks for
/// ("no parse error; type errors allowed") and the one thing
/// `syn::parse_file` cannot make at all.
fn parse_error_messages(stdout: &[u8]) -> Vec<String> {
    let mut parse_errors = Vec::new();
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
        if message.get("level").and_then(|l| l.as_str()) != Some("error") {
            continue;
        }
        if message.get("code").is_some() {
            // Has an E-code: a type/name-resolution error, permitted.
            continue;
        }
        let rendered = message
            .get("rendered")
            .and_then(|r| r.as_str())
            .unwrap_or("<no rendered message>");
        parse_errors.push(rendered.to_string());
    }
    parse_errors
}
