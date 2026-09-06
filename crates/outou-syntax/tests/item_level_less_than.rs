//! End-to-end regression tests for F1 (issue #12 corpus review, CRITICAL):
//! at item level (brace depth 0), any `<` preceded by whitespace or a
//! comment used to be committed to JSX, because
//! `parser::item::parse_items`'s post-`scan_leading_attributes` guard
//! reset `prev` to `None` whenever *any* trivia was skipped — including
//! plain whitespace with no attribute at all — rather than only when an
//! attribute or doc comment was actually consumed. `position::from_prev`
//! treats `None` the same as start-of-file: the permissive `Expr`
//! default, which makes the following `<` a JSX candidate.
//!
//! Every case here goes through the real driver, `outou_syntax::parse`,
//! not the classifier functions in isolation (`lexer::position`,
//! `parser::disambiguate`) the way `tests/disambiguation.rs` and those
//! modules' own unit tests do. That distinction is the point: the bug
//! lived one level up, in `parser::item`'s own caller logic, and every
//! unit test calling `position::from_prev`/`disambiguate::classify`
//! directly on a hand-built token sequence passed regardless — only a
//! test that actually runs the item-level scanning loop over real source
//! text can catch it, which is exactly why the corpus caught it and no
//! existing in-repo test did (issue #12 review, F11).

/// Parses `source` and asserts it produced no JSX and no diagnostics —
/// the expected result for every row below, all of which are ordinary,
/// syntactically valid Rust with no JSX in it at all.
fn assert_clean(label: &str, source: &str) {
    let parsed = outou_syntax::parse(source);
    assert!(
        parsed.diagnostics.is_empty(),
        "{label}: expected no diagnostics, got {:?} (source: {source:?})",
        parsed.diagnostics
    );
    let elements = parsed.file.jsx_elements();
    assert!(
        elements.is_empty(),
        "{label}: expected no JSX elements, found {} (first span: {:?}, source: {source:?})",
        elements.len(),
        elements[0].span
    );
}

/// The inverse of [`assert_clean`]: asserts `source` *is* detected as
/// containing JSX. Used only for the handful of controls that must stay
/// JSX (there are none among F1's own table, but kept for symmetry with
/// `tests/disambiguation.rs`'s style and in case a future regression
/// flips one of the clean cases the wrong way).
fn assert_has_jsx(label: &str, source: &str) {
    let parsed = outou_syntax::parse(source);
    assert!(
        !parsed.file.jsx_elements().is_empty(),
        "{label}: expected at least one JSX element, found none (source: {source:?})"
    );
}

// --- F1 table rows: whitespace before `<` at item level (depth 0) -------

#[test]
fn impl_block_with_space_before_generic_param() {
    assert_clean(
        "impl <T> Foo<T> {}",
        "impl <T> Foo<T> {}\nstruct Foo<T>(T);\n",
    );
}

#[test]
fn struct_with_space_before_generic_param() {
    assert_clean("struct X <T> {}", "struct X <T> {}\n");
}

#[test]
fn enum_with_space_before_generic_param() {
    assert_clean("enum E <T> { V(T) }", "enum E <T> { V(T) }\n");
}

#[test]
fn trait_with_space_before_generic_param() {
    assert_clean("trait Tr <T> {}", "trait Tr <T> {}\n");
}

#[test]
fn type_alias_with_space_before_generic_param() {
    assert_clean("type A <T> = Vec<T>;", "type A <T> = Vec<T>;\n");
}

#[test]
fn impl_trait_for_type_with_space_before_generic_param() {
    assert_clean(
        "impl Add <RHS> for X {}",
        "trait Add<RHS> { fn add(self, rhs: RHS) -> Self; }\nstruct X;\nimpl Add <RHS> for X {\n    fn add(self, rhs: RHS) -> Self { self }\n}\nstruct RHS;\n",
    );
}

#[test]
fn top_level_const_comparison_with_space_before_lt() {
    assert_clean(
        "const C: bool = X < Y;",
        "const X: i32 = 1;\nconst Y: i32 = 2;\nconst C: bool = X < Y;\n",
    );
}

#[test]
fn top_level_static_comparison_with_space_before_lt() {
    assert_clean(
        "static C: bool = X < Y;",
        "const X: i32 = 1;\nconst Y: i32 = 2;\nstatic C: bool = X < Y;\n",
    );
}

#[test]
fn type_alias_with_space_before_turbofish_style_arg() {
    assert_clean("type A = Vec <u8>;", "type A = Vec <u8>;\n");
}

/// The exact `issue-93775.rs` shape: a deeply nested generic type alias
/// with a newline (not a plain space) before one of its `<`s. Plain
/// whitespace, not specifically a space character, is what the buggy
/// guard reacted to.
#[test]
fn type_alias_wrapped_across_lines_before_lt() {
    assert_clean(
        "type S = S<S<S\n<u8>>>;",
        "struct S<T>(T);\ntype S = S<S<S\n<u8>>>;\n",
    );
}

/// A comment (not just whitespace) between an identifier and `<` at item
/// level: a second, related bug in the same family (the comment-skipping
/// branch overwrote `prev` with the comment token itself, and every
/// comment kind falls through to `position::from_prev`'s permissive
/// `Expr` default) — fixed alongside F1's own guard in both
/// `parser::item` and `parser::region`.
#[test]
fn dyn_trait_object_with_comment_before_associated_type_binding() {
    assert_clean(
        "dyn Bar // comment\n <Assoc=()>",
        "trait Bar { type Assoc; }\nfn f() -> Box<dyn Bar // comment\n <Assoc=()>> { todo!() }\n",
    );
}

// --- Controls: the passing forms that must stay passing -----------------

#[test]
fn no_space_impl_generic_stays_clean() {
    assert_clean("impl<T> F for T {}", "trait F {}\nimpl<T> F for T {}\n");
}

#[test]
fn no_space_comparison_stays_clean() {
    assert_clean(
        "const C: bool = A< B;",
        "const A: i32 = 1;\nconst B: i32 = 2;\nconst C: bool = A< B;\n",
    );
}

#[test]
fn in_body_comparison_stays_clean_at_any_depth() {
    assert_clean(
        "fn f() { let c = A < B; }",
        "fn f() -> bool { let a = 1; let b = 2; let c = a < b; c }\n",
    );
}

#[test]
fn nested_item_inside_a_brace_depth_greater_than_zero_stays_clean() {
    // `depth > 0` (inside `impl X { ... }`): the whitespace-before-`<`
    // guard only ever ran at `depth == 0`, so this was already clean even
    // before the fix — kept as a control so a future regression in the
    // depth tracking itself is caught here too.
    assert_clean(
        "struct X; impl X { const C: bool = A < B; }",
        "struct X;\nconst A: i32 = 1;\nconst B: i32 = 2;\nimpl X {\n    const C: bool = A < B;\n}\n",
    );
}

#[test]
fn real_jsx_at_item_level_is_still_detected() {
    // A genuine regression guard in the opposite direction: the fix must
    // not make item-level JSX detection stop working altogether.
    assert_has_jsx(
        "const VIEW: Element = <div/>;",
        "use outou::prelude::*;\nconst VIEW: Element = <div/>;\n",
    );
}
