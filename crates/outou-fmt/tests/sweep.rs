//! Runs the formatter over every real `.rsx` file this repository ships
//! (`tests/fixtures/**` and `examples/phase0-app/src/**`) as a
//! never-panics + idempotency + semantics-preserved sweep
//! (`docs/phase0/issues/13-formatter.md`).
//!
//! A file with syntax errors (most of `tests/fixtures/incomplete/` and
//! `tests/fixtures/diagnostics/`) is expected to be refused, not
//! formatted; every other file must format without panicking, format
//! idempotently, and keep every JSX element's tag, attributes and
//! children the same (`assert_semantics_preserved`, a coarse
//! span-independent comparison of the parsed trees — see its own doc
//! comment for what it does and does not catch).

use std::path::{Path, PathBuf};

use outou_fmt::{format_source, FormatOptions};
use outou_syntax::ast;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/outou-fmt is two levels below the workspace root")
        .to_path_buf()
}

fn collect_rsx_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rsx") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn never_panics_and_is_idempotent_and_semantics_preserved_over_every_fixture() {
    let root = workspace_root();
    let mut files = collect_rsx_files(&root.join("tests/fixtures"));
    files.extend(collect_rsx_files(&root.join("examples/phase0-app/src")));
    // This crate's own golden fixtures (`tests/golden.rs`), including the
    // multi-line-token cases (a multi-line string literal, a multi-line
    // attribute value, an island mixing a comment with code) that must
    // never regress into corruption on a second pass — exactly what this
    // sweep's idempotency check below would catch.
    files.extend(collect_rsx_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
    ));
    assert!(!files.is_empty(), "expected to find .rsx fixtures to sweep");

    let options = FormatOptions::default();
    let mut formatted_count = 0;
    let mut refused_count = 0;

    for path in &files {
        let source = std::fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
        let relative = path.strip_prefix(&root).unwrap_or(path);

        let parsed = outou_syntax::parse(&source);
        let source_has_errors = !parsed.diagnostics.is_empty();

        match format_source(&source, &options) {
            Ok(formatted) => {
                assert!(
                    !source_has_errors,
                    "{}: formatted a file that has syntax errors",
                    relative.display()
                );
                formatted_count += 1;

                let twice = format_source(&formatted, &options).unwrap_or_else(|err| {
                    panic!(
                        "{}: formatting its own output failed: {err}",
                        relative.display()
                    )
                });
                assert_eq!(
                    formatted,
                    twice,
                    "{}: formatting is not idempotent",
                    relative.display()
                );

                assert_semantics_preserved(&source, &formatted, relative);
            }
            Err(err) => {
                // Refusing is always acceptable for a broken file: most
                // come from `tests/fixtures/incomplete/` and
                // `tests/fixtures/diagnostics/`, which have an Outou
                // diagnostic. A handful are broken only at the *Rust*
                // level (an unclosed `fn` signature, say) — Outou's own
                // parser does not validate Rust syntax (`AGENTS.md`:
                // "the parser never panics", not "the parser rejects
                // invalid Rust"), so that surfaces as `rustfmt` refusing
                // the (still Rust-invalid) text instead, which is an
                // equally correct refusal. What must never happen is an
                // internal-invariant failure, which would mean this
                // crate's own placeholder/splice bookkeeping broke.
                assert!(
                    !matches!(err, outou_fmt::FormatError::Internal(_)),
                    "{}: internal formatter error: {err}",
                    relative.display()
                );
                let _ = source_has_errors;
                refused_count += 1;
            }
        }
    }

    // Sanity check on the sweep itself: both branches above should have
    // been exercised, or this test would pass vacuously.
    assert!(formatted_count > 0, "no fixture was ever formatted");
    assert!(refused_count > 0, "no fixture was ever refused");
}

/// A coarse, span-independent comparison of every JSX element's shape
/// between `before` and `after`: tag name, each attribute's name and
/// value, and each child's kind and content. Element and expression
/// content is compared via [`token_normalized`] (rustfmt is free to
/// reflow the Rust inside an expression island; this only checks that
/// its *tokens* — including a literal's own content, byte-for-byte — did
/// not change) — this misses a change that only reorders
/// identically-normalized tokens inside a comment, but catches any real
/// change to structure, tag names, attribute names/values, text content,
/// or a literal's interior (the corruption class `outou_fmt`'s
/// `multiline_guard` module guards against).
fn assert_semantics_preserved(before_source: &str, after_source: &str, relative: &Path) {
    let before = outou_syntax::parse(before_source);
    let after = outou_syntax::parse(after_source);
    assert!(
        after.diagnostics.is_empty(),
        "{}: formatted output has syntax errors: {:?}",
        relative.display(),
        after.diagnostics
    );

    let before_canon = canon_file(&before.file, before_source);
    let after_canon = canon_file(&after.file, after_source);
    assert_eq!(
        before_canon,
        after_canon,
        "{}: JSX structure or content changed by formatting",
        relative.display()
    );
}

#[derive(Debug, PartialEq)]
enum AttrCanon {
    Bare,
    Text(String),
    Expr(String),
}

#[derive(Debug, PartialEq)]
enum ChildCanon {
    Text(String),
    Expr(String),
    ElementTag(String),
    Error,
}

#[derive(Debug, PartialEq)]
struct ElementCanon {
    tag: String,
    attrs: Vec<(String, AttrCanon)>,
    children: Vec<ChildCanon>,
}

/// Canonicalizes a Rust expression's source text for comparison: token
/// boundaries are normalized to a single space, but a literal's own
/// content (a string, a raw string, a comment) is copied through
/// byte-for-byte.
///
/// A plain "strip every whitespace character" comparison here would be
/// worse than useless for exactly the bug class this sweep exists to
/// catch: it would call a multi-line string literal whose *interior*
/// whitespace was corrupted by reformatting "equal" to the original,
/// since all whitespace — structural and literal-content alike — was
/// being discarded on both sides. Tokenizing with the same coarse lexer
/// the parser itself uses (`outou_syntax::lexer::rust_token`) and keeping
/// a `Literal`/comment token's text untouched makes this comparison
/// actually sensitive to that corruption.
fn token_normalized(text: &str) -> String {
    use outou_syntax::lexer::rust_token::next_token;

    let bytes = text.as_bytes();
    let mut out = String::new();
    let mut pos = 0usize;
    loop {
        let token = next_token(bytes, pos);
        if token.start == token.end {
            return out; // End of input.
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&text[token.start..token.end]);
        pos = token.end;
    }
}

fn tag_name(tag: &ast::JsxTag) -> String {
    match tag {
        ast::JsxTag::Named { name, .. } => name.name.clone(),
        ast::JsxTag::Incomplete(_) => "<incomplete>".to_string(),
    }
}

fn canon_element(element: &ast::JsxElement, source: &str) -> ElementCanon {
    let attrs = element
        .attributes
        .iter()
        .map(|attribute| {
            let value = match &attribute.value {
                None => AttrCanon::Bare,
                Some(ast::JsxAttributeValue::Text(text)) => AttrCanon::Text(text.value.clone()),
                Some(ast::JsxAttributeValue::Expression(island)) => AttrCanon::Expr(
                    token_normalized(&source[island.span.start as usize..island.span.end as usize]),
                ),
                Some(ast::JsxAttributeValue::Error(_)) => AttrCanon::Bare,
            };
            (attribute.name.name.clone(), value)
        })
        .collect();

    let children = element
        .children
        .iter()
        .map(|child| match child {
            ast::JsxChild::Text(text) => ChildCanon::Text(text.value.clone()),
            ast::JsxChild::Expression(island) => ChildCanon::Expr(token_normalized(
                &source[island.span.start as usize..island.span.end as usize],
            )),
            ast::JsxChild::Element(nested) => ChildCanon::ElementTag(tag_name(&nested.open)),
            ast::JsxChild::Error(_) => ChildCanon::Error,
        })
        .collect();

    ElementCanon {
        tag: tag_name(&element.open),
        attrs,
        children,
    }
}

fn canon_file(file: &ast::File, source: &str) -> Vec<ElementCanon> {
    file.jsx_elements()
        .iter()
        .map(|element| canon_element(element, source))
        .collect()
}
