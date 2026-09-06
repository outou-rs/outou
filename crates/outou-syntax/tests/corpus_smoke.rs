//! A small in-repo stand-in for the external corpus (issue #12): plain
//! Rust snippets, deliberately chosen from the categories `cargo xtask
//! corpus test` reports on (macro token trees, qualified paths, raw
//! strings, lifetimes, generics, comparison chains that could be
//! mistaken for a JSX tag), asserting `outou_syntax::parse` finds no JSX
//! and raises no diagnostics on any of them. This runs in the normal
//! `cargo test --workspace` suite, unlike the external corpus (which
//! needs `cargo xtask corpus fetch` first and is exercised by the
//! nightly job instead).

/// (label, source). Every source is plain Rust with no JSX in it; a
/// diagnostic or a detected JSX element on any of these is a Outou
/// false-positive detection bug, not a fixture-authoring mistake.
const SNIPPETS: &[(&str, &str)] = &[
    (
        "raw-string-with-angle-brackets",
        r####"fn f() -> &'static str { r"a < b" }"####,
    ),
    (
        "raw-string-with-hash-and-quotes",
        r####"fn f() -> &'static str { r#"contains "quotes" and < > too"# }"####,
    ),
    (
        "comparison-chain",
        "fn f(a: i32, b: i32, c: i32) -> bool { a < b && b > c }",
    ),
    (
        "comparison-chain-parenthesized",
        "fn f(a: i32, b: i32, c: i32, d: i32) -> bool { (a < b) == (c > d) }",
    ),
    (
        "turbofish",
        "fn f() -> Vec<i32> { Vec::<i32>::new() }",
    ),
    (
        "nested-turbofish",
        "fn f() -> Vec<Vec<i32>> { Vec::<Vec<i32>>::new() }",
    ),
    (
        "higher-ranked-trait-bound",
        "fn f<T>() where T: for<'a> Fn(&'a str) -> bool {}",
    ),
    (
        "impl-with-generic-and-hrtb-bound",
        "trait Trait<'a> {} struct Foo<T>(T); impl<T: Clone> Trait<'_> for Foo<T> where T: for<'a> Fn(&'a str) {}",
    ),
    (
        "macro-body-with-jsx-like-tokens",
        r#"macro_rules! m { () => { let x = "<div>not jsx</div>"; } } fn f() { m!(); }"#,
    ),
    (
        "macro-invocation-with-comparison",
        "macro_rules! m { ($x:expr) => { $x } } fn f() -> bool { m!(1 < 2) }",
    ),
    (
        "byte-string",
        r#"fn f() -> &'static [u8] { b"bytes < more bytes" }"#,
    ),
    (
        "byte-char-literal",
        "fn f() -> u8 { b'<' }",
    ),
    (
        "char-literals",
        "fn f() -> (char, char) { ('<', '>') }",
    ),
    (
        "lifetimes",
        "fn f<'a>(x: &'a str) -> &'a str { x }",
    ),
    (
        "struct-with-lifetime-and-generic",
        "struct Pair<'a, T> { a: &'a T, b: &'a T }",
    ),
    (
        "nested-generics",
        "use std::collections::HashMap; fn f() -> Vec<Vec<HashMap<String, i32>>> { Vec::new() }",
    ),
    (
        "qualified-path-method",
        "trait Trait { fn f() -> i32; } struct Foo; impl Trait for Foo { fn f() -> i32 { 0 } } fn g() -> i32 { <Foo as Trait>::f() }",
    ),
    (
        "qualified-path-assoc-const",
        "trait Trait { const N: i32; } struct Foo; impl Trait for Foo { const N: i32 = 1; } fn g() -> i32 { <Foo as Trait>::N }",
    ),
    (
        "chained-comparisons-in-if",
        "fn f(x: i32, y: i32, z: i32, w: i32) -> bool { if x < y && y < z && z < w { true } else { false } }",
    ),
    (
        "generic-fn-with-where-and-comparison-body",
        "fn f<T: PartialOrd>(a: T, b: T) -> bool where T: Copy { a < b }",
    ),
    // F1 (issue #12 corpus review, CRITICAL): item-level (brace depth 0)
    // whitespace before `<` used to be committed to JSX. These are the
    // exact shapes that made up 40 of the 61 real-corpus false positives;
    // `crates/outou-syntax/tests/item_level_less_than.rs` carries the full
    // end-to-end regression suite (through the real `outou_syntax::parse`
    // driver, per F11) — these snippets are kept here too so the fixed
    // shapes stay covered by the same in-repo "small corpus" this file
    // otherwise is, and so a regression here fails the ordinary `cargo
    // test --workspace` suite, not only the dedicated test file.
    (
        "item-level-impl-with-space-before-generic-param",
        "struct Foo<T>(T);\nimpl <T> Foo<T> {}",
    ),
    (
        "item-level-struct-with-space-before-generic-param",
        "struct X <T> {}",
    ),
    (
        "item-level-trait-with-space-before-generic-param",
        "trait Tr <T> {}",
    ),
    (
        "item-level-const-comparison-with-space-before-lt",
        "const X: i32 = 1;\nconst Y: i32 = 2;\nconst C: bool = X < Y;",
    ),
    (
        "item-level-type-alias-wrapped-across-lines-before-lt",
        "struct S<T>(T);\ntype S = S<S<S\n<u8>>>;",
    ),
];

#[test]
fn no_snippet_produces_diagnostics_or_detects_jsx() {
    assert!(
        SNIPPETS.len() >= 20,
        "expected at least 20 corpus-smoke snippets, found {}",
        SNIPPETS.len()
    );
    for (label, source) in SNIPPETS {
        let parsed = outou_syntax::parse(source);
        assert!(
            parsed.diagnostics.is_empty(),
            "{label}: expected no diagnostics, got {:?}",
            parsed.diagnostics
        );
        // F19 (issue #12 corpus review): this traversal used to be a
        // verbatim copy of `xtask/src/corpus/splice.rs`'s own
        // `collect_jsx_elements`; both now call the one canonical
        // implementation, `outou_syntax::ast::File::jsx_elements`.
        let elements = parsed.file.jsx_elements();
        assert!(
            elements.is_empty(),
            "{label}: expected no JSX elements, found {} (first span: {:?})",
            elements.len(),
            elements[0].span
        );
    }
}
