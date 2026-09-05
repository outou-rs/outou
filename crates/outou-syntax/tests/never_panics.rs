//! `outou_syntax::parse` must never panic, on any input: every fixture,
//! every byte-boundary-respecting prefix of every fixture, and a handful
//! of adversarial inputs designed to hit the mode/frame-stack machinery
//! directly.

use std::fs;
use std::panic;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/outou-syntax has a parent")
        .parent()
        .expect("crates/ has a parent")
        .to_path_buf()
}

fn all_fixture_sources() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let fixtures = repo_root().join("tests").join("fixtures");
    for sub in ["formatting", "incomplete", "diagnostics"] {
        collect_rsx(&fixtures.join(sub), &mut out);
    }
    out
}

/// Collects every `.rsx` fixture under `dir`, recursively. Unlike a
/// silent "skip what can't be read" scan, every I/O error here panics
/// with the specific path that failed (L3): a broken fixtures directory
/// must fail loudly, not quietly shrink the set of fixtures this test
/// actually exercises down to whatever happened to be readable.
fn collect_rsx(dir: &Path, out: &mut Vec<(String, String)>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("reading directory {}: {e}", dir.display()));
    for entry in entries {
        let entry = entry
            .unwrap_or_else(|e| panic!("reading a directory entry under {}: {e}", dir.display()));
        let path = entry.path();
        if path.is_dir() {
            collect_rsx(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rsx") {
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
            out.push((path.display().to_string(), source));
        }
    }
}

fn assert_parses_without_panic(label: &str, source: &str) {
    let result = panic::catch_unwind(|| outou_syntax::parse(source));
    assert!(result.is_ok(), "parse panicked on {label}: {source:?}");
}

/// The known minimum number of `.rsx` fixtures under `formatting/`,
/// `incomplete/` and `diagnostics/` combined (L3): a plain "not empty"
/// check would not notice the collection quietly losing fixtures (an
/// unreadable subdirectory, say) as long as at least one remained. Grows
/// over time as fixtures are added; never shrinks without a reason.
const MIN_KNOWN_FIXTURE_COUNT: usize = 17;

#[test]
fn every_fixture_parses_without_panic() {
    let fixtures = all_fixture_sources();
    assert!(
        fixtures.len() >= MIN_KNOWN_FIXTURE_COUNT,
        "expected at least {MIN_KNOWN_FIXTURE_COUNT} .rsx fixtures, found {}",
        fixtures.len()
    );
    for (label, source) in fixtures {
        assert_parses_without_panic(&label, &source);
    }
}

/// Every byte-boundary-respecting prefix of every fixture must also parse
/// without panicking: this is exactly what an editor sends on every
/// keystroke while a file is mid-edit.
#[test]
fn every_prefix_of_every_fixture_parses_without_panic() {
    let fixtures = all_fixture_sources();
    for (label, source) in fixtures {
        for (byte_index, _) in source.char_indices() {
            let prefix = &source[..byte_index];
            assert_parses_without_panic(&format!("{label} (prefix {byte_index})"), prefix);
        }
        assert_parses_without_panic(&format!("{label} (full)"), &source);
    }
}

/// Regression for H1: a trailing, unescaped backslash at end of input (or
/// right before end of a string body) must not panic `advance_char`'s
/// out-of-bounds slice. See `docs/grammar.md` §9 ("the parser MUST produce
/// an AST for any input").
#[test]
fn trailing_backslash_in_unterminated_string_does_not_panic() {
    let inputs = ["\"\\", "b\"x\\", "fn f() { let s = \"abc\\", "\"é\\"];
    for input in inputs {
        let result = panic::catch_unwind(|| outou_syntax::parse(input));
        assert!(result.is_ok(), "parse panicked on {input:?}");
        let parsed = outou_syntax::parse(input);
        for item in &parsed.file.items {
            assert_item_spans_in_bounds(item, input.len(), input);
        }
    }
}

fn assert_item_spans_in_bounds(item: &outou_syntax::ast::Item, len: usize, label: &str) {
    use outou_syntax::ast::Item;
    match item {
        Item::Function(f) => {
            assert!(
                f.span.end as usize <= len,
                "{label}: function span out of bounds"
            );
        }
        Item::Module(m) => {
            assert!(
                m.span.end as usize <= len,
                "{label}: module span out of bounds"
            );
        }
        Item::Rust(r) => {
            assert!(
                r.span.end as usize <= len,
                "{label}: rust item span out of bounds"
            );
        }
        Item::Error(e) => {
            assert!(
                e.span.end as usize <= len,
                "{label}: error span out of bounds"
            );
        }
    }
}

#[test]
fn adversarial_inputs_parse_without_panic() {
    let inputs: Vec<String> = vec![
        "<".to_string(),
        "</".to_string(),
        "<>".to_string(),
        "{".to_string(),
        "}".to_string(),
        "<a<".to_string(),
        "<a<b<c".to_string(),
        "<<<<".to_string(),
        "\"".to_string(),
        "\"unterminated".to_string(),
        "macro_rules! m {".to_string(),
        "<".repeat(10_000),
        "fn f() { <div title=\"unterminated />".to_string(),
        "fn f() { <div>{</div>".to_string(),
        "fn f() { <A></B<C> }".to_string(),
        "fn f() { <A><B></A></B> }".to_string(),
        "fn f() { </div> }".to_string(),
        "fn f() { <Self /> }".to_string(),
        "fn f() { <div {...props} /> }".to_string(),
        "fn f() { <Foo.Bar /> }".to_string(),
        "fn f() { <svg:rect /> }".to_string(),
        "fn f() { <List<T> items={items} /> }".to_string(),
        "fn f() { <div title='x' /> }".to_string(),
        "fn f() { <div value=\"a\" value=\"b\" /> }".to_string(),
        "fn f() { <div /> .into() }".to_string(),
        "#[component]\nfn App() -> Element {".to_string(),
        String::new(),
        "\0\0\0".to_string(),
        "// just a comment, no code at all".to_string(),
    ];
    for input in inputs {
        assert_parses_without_panic("adversarial", &input);
    }
}

/// Regression for H7: unbounded recursion (one call frame per nested JSX
/// element, and one per nested inline `mod`) overflows the stack before
/// grammar §9's "the parser MUST produce an AST for any input" can be
/// honored. Decision D4 caps nesting at 128 levels and diagnoses instead of
/// recursing further. These must be plain `#[test]`s (no spawned thread with
/// a larger stack) so they exercise the same ~2 MiB thread `cargo test`
/// itself runs on.
#[test]
fn deeply_nested_input_is_diagnosed_not_overflowed() {
    let jsx_500_deep = format!("fn f() {{ {}{} }}", "<a>".repeat(500), "</a>".repeat(500));
    let unclosed_2000_deep = format!("fn f() {{ {} }}", "<a>".repeat(2000));
    let mod_1000_deep = "mod m {".repeat(1000);

    for (label, source) in [
        ("jsx-500-deep", jsx_500_deep),
        ("unclosed-2000-deep", unclosed_2000_deep),
        ("mod-1000-deep", mod_1000_deep),
    ] {
        let result = panic::catch_unwind(|| outou_syntax::parse(&source));
        assert!(result.is_ok(), "parse panicked on {label}");
        let parsed = outou_syntax::parse(&source);
        let nesting_diagnostics: Vec<_> = parsed
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("nested too deeply"))
            .collect();
        assert_eq!(
            nesting_diagnostics.len(),
            1,
            "{label}: expected exactly one nesting diagnostic, got {:?}",
            parsed.diagnostics
        );
    }
}
