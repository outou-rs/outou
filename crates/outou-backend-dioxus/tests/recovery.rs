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
//! as it stood before issue #18: its swallowed tail (`fn After`,
//! absorbed into a JSX text child by the parser's own recovery, issue #4)
//! became unparseable Rust once `rsx!` actually expands it. [`every_incomplete_fixture_compiles_with_no_parse_errors`]
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
//! Those two fixtures used to demonstrate the swallowed-tail limitation
//! (HIGH-4, LOW-17): the parser's own recovery folded `fn After` into the
//! broken function's signature or JSX text before codegen ever saw the
//! file, so the symbol was unrecoverable by construction. Issue #18 fixed
//! that in the parser (a broken signature stops, and an unclosed body that
//! runs to end of input is cut, at the next column-0 item head); the dedicated test
//! `swallowed_tail_fixtures_keep_the_item_after_the_broken_construct`
//! asserts `fn After` is its own item, the broken construct is diagnosed
//! and `fn After` is in the recovery output.

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

/// The swallowed-tail fixtures (issue #18, gate A2): the item after the
/// broken construct must survive as its own `Item::Function`, the broken
/// construct must be diagnosed, and the recovery output must still carry
/// `fn After` verbatim so rust-analyzer can resolve the symbol.
#[test]
fn swallowed_tail_fixtures_keep_the_item_after_the_broken_construct() {
    // Every `*-mid-file` fixture has a `fn After` below its broken construct.
    let mid_file: Vec<_> = fixture_paths()
        .into_iter()
        .filter(|path| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| stem.ends_with("-mid-file"))
        })
        .collect();
    assert!(
        mid_file.len() >= 4,
        "expected the four *-mid-file fixtures, found {mid_file:?}"
    );
    for path in mid_file {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
        let source =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        let parsed = outou_syntax::parse(&source);

        let after = parsed
            .file
            .items
            .iter()
            .find_map(|item| match item {
                ast::Item::Function(f) if f.name.name == "After" => Some(f),
                _ => None,
            })
            .unwrap_or_else(|| {
                panic!("{name}: `fn After` is not its own item: {:#?}", parsed.file)
            });
        assert!(after.is_component, "{name}: `fn After` lost #[component]");
        assert!(
            after.body.close.is_some(),
            "{name}: `fn After` lost its body"
        );
        assert!(
            !parsed.diagnostics.is_empty(),
            "{name}: the truncated construct is not diagnosed"
        );
        let after_text = &source[after.span.start as usize..after.span.end as usize];
        assert!(
            after_text.trim_start().starts_with("#[component]") && after_text.ends_with('}'),
            "{name}: `fn After` span is wrong: {after_text:?}"
        );

        let opts = GenerateOptions::new(Uri::new("file:///gen.rs"), Uri::new("file:///a.rsx"));
        let generated = DioxusBackend
            .generate(&parsed, &source, Mode::Recovery, &opts)
            .unwrap_or_else(|e| panic!("{name}: recovery generation must never fail: {e}"));
        assert!(
            generated.rust.contains("fn After() -> Element {"),
            "{name}: `fn After` is missing from recovery output:\n{}",
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

    let mut checked = 0usize;
    let mut failures: Vec<String> = Vec::new();
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
        checked += 1;
        if !parse_errors.is_empty() {
            failures.push(format!(
                "{}: recovery output has a parse error (type/name-resolution errors are \
                 permitted):\n{}\n---generated---\n{}",
                path.display(),
                parse_errors.join("\n---\n"),
                generated.rust,
            ));
        }
    }
    println!("recovery compile gate: checked {checked} fixtures");
    assert!(
        failures.is_empty(),
        "{} of {checked} fixtures failed:\n{}",
        failures.len(),
        failures.join("\n=====\n")
    );
}

/// Extracts the rendered text of every `error`-level diagnostic from
/// `cargo check --message-format=json` output whose rustc `code` is `null`
/// (rustc always writes the key; a parse error has no code), except the
/// `aborting due to` summary — the signature of a *parse* error. A type or
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
        // rustc always emits the key, with a JSON `null` when there is no
        // code, so presence of the key proves nothing (the gate used to
        // skip every error that way).
        if message.get("code").is_some_and(|code| !code.is_null()) {
            // Has an E-code: a type/name-resolution error, permitted.
            continue;
        }
        let text = message
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("");
        if text.starts_with("aborting due to") {
            // The summary line has no code either; it is not a parse error.
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

fn recovery_output(source: &str) -> String {
    let parsed = outou_syntax::parse(source);
    let opts = GenerateOptions::new(Uri::new("file:///gen.rs"), Uri::new("file:///a.rsx"));
    DioxusBackend
        .generate(&parsed, source, Mode::Recovery, &opts)
        .expect("recovery generation never fails")
        .rust
}

/// `rsx!` accepts a child as `name { .. }` or `{expr}`, never as a bare call,
/// so the placeholder must be braced in child position.
#[test]
fn recovery_element_in_child_position_is_braced() {
    let rust = recovery_output("fn App() -> Element {\n    <div>a <3> b</div>\n}\n");
    assert!(
        rust.contains("{::outou::__private::recovery_element()}"),
        "{rust}"
    );
}

/// A closing tag swallowed by an island that lost its `}` must not leave a
/// placeholder next to the operand it follows.
#[test]
fn swallowed_closing_tag_after_an_operand_leaves_no_placeholder() {
    let source = "fn App(v: Vec<i32>) -> Element {\n    <ul>\n        {v.iter().map(|x| <li>{x}</li>)\n    </ul>\n}\n";
    let rust = recovery_output(source);
    assert!(!rust.contains("</ul>"), "{rust}");
    assert!(
        !rust.contains(") ::outou")
            && !rust.contains(")\n    ::outou::__private::recovery_element"),
        "{rust}"
    );
}
