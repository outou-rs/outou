//! One test per row of `docs/grammar.md` §4.1's decision table: for each
//! input, either a JSX expression must appear somewhere in the parsed
//! AST, or none must — matching the table's Rust/JSX verdict. The parser
//! never validates whether the *rest* of a Rust snippet is well-formed
//! (that is rustc's job), so a row that yields "Rust" only needs to
//! contain no [`outou_syntax::ast::Expr::Jsx`] anywhere; a row that yields
//! "JSX" (whether accepted or reserved-and-diagnosed) needs at least one.

use outou_syntax::ast;

fn contains_jsx_in_file(file: &ast::File) -> bool {
    file.items.iter().any(contains_jsx_in_item)
}

fn contains_jsx_in_item(item: &ast::Item) -> bool {
    match item {
        ast::Item::Function(f) => {
            f.body.statements.iter().any(contains_jsx_in_expr)
                || f.body.tail.as_deref().is_some_and(contains_jsx_in_expr)
        }
        ast::Item::Module(m) => m.items.iter().flatten().any(contains_jsx_in_item),
        ast::Item::Rust(_) | ast::Item::Error(_) => false,
    }
}

fn contains_jsx_in_expr(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Jsx(_) => true,
        ast::Expr::Rust(_) | ast::Expr::Error(_) => false,
    }
}

/// Parses `body` as a function body and asserts whether a JSX expression
/// was found anywhere in it.
fn assert_row(row: &str, body: &str, expect_jsx: bool) {
    let source = format!("use outou::prelude::*;\n\nfn f() {{\n    {body}\n}}\n");
    let parsed = outou_syntax::parse(&source);
    let found = contains_jsx_in_file(&parsed.file);
    assert_eq!(
        found, expect_jsx,
        "row `{row}`: expected JSX={expect_jsx}, got JSX={found} (source: {source:?})"
    );
}

#[test]
fn row_a_lt_b_is_rust() {
    assert_row("a < b", "a < b;", false);
}

#[test]
fn row_a_lt_path_is_rust() {
    assert_row("a < B::C", "a < B::C;", false);
}

#[test]
fn row_a_lt_b_gt_c_is_rust_tokens() {
    assert_row("a <b>c", "a <b>c;", false);
}

#[test]
fn row_if_x_lt_y_is_rust() {
    assert_row("if x < y {", "if x < y {};", false);
}

#[test]
fn row_closure_is_rust() {
    assert_row("|x| x < y", "let _ = |x: i32| x < y;", false);
}

#[test]
fn row_chained_comparison_is_rust() {
    assert_row("x < y && y > z", "x < y && y > z;", false);
}

#[test]
fn row_turbofish_is_rust() {
    assert_row("f::<T>()", "f::<T>();", false);
}

#[test]
fn row_type_annotation_is_rust() {
    assert_row("let v: Vec<A> = …", "let v: Vec<A> = make();", false);
}

#[test]
fn row_qualified_path_as_trait_is_rust() {
    assert_row("<T as Trait>::f()", "<T as Trait>::f();", false);
}

#[test]
fn row_qualified_path_coloncolon_is_rust() {
    assert_row("<A::B>::c()", "<A::B>::c();", false);
}

#[test]
fn row_slice_type_is_rust() {
    assert_row("<[T]>::len(&x)", "<[T]>::len(&x);", false);
}

#[test]
fn row_reference_type_is_rust() {
    assert_row("<&str>::from(s)", "<&str>::from(s);", false);
}

#[test]
fn row_tuple_type_is_rust() {
    assert_row("<(A, B)>::default()", "<(A, B)>::default();", false);
}

#[test]
fn row_raw_pointer_type_is_rust() {
    assert_row("<*const T>::f()", "<*const T>::f();", false);
}

#[test]
fn row_dyn_trait_is_rust() {
    assert_row("<dyn Trait>::f(&x)", "<dyn Trait>::f(&x);", false);
}

#[test]
fn row_nested_qualified_path_is_rust() {
    assert_row("<<A as B>::C>::d()", "<<A as B>::C>::d();", false);
}

#[test]
fn row_generic_type_with_path_sep_is_rust() {
    assert_row("<Vec<i32>>::new()", "<Vec<i32>>::new();", false);
}

#[test]
fn row_self_type_is_rust() {
    assert_row("<Self>::new()", "<Self>::new();", false);
}

#[test]
fn row_reserved_text_after_tag_is_rust() {
    assert_row("<A>::B</A>", "<A>::B;", false);
}

#[test]
fn row_intrinsic_div_is_jsx() {
    assert_row("<div>", "<div></div>;", true);
}

#[test]
fn row_truncated_tag_is_jsx() {
    assert_row("<div cl", "<div cl", true);
}

#[test]
fn row_keyword_attribute_name_is_jsx() {
    assert_row("<div type=\"x\" />", "<div type=\"x\" />;", true);
}

#[test]
fn row_as_attribute_is_jsx() {
    assert_row("<link as=\"style\" />", "<link as=\"style\" />;", true);
}

#[test]
fn row_self_closing_in_let_is_jsx() {
    assert_row("let v = <Button />;", "let v = <Button />;", true);
}

#[test]
fn row_return_position_is_jsx() {
    assert_row("return <A />", "return <A />;", true);
}

#[test]
fn row_match_arm_is_jsx() {
    assert_row(
        "match m { _ => <A/> }",
        "match m { _ => <A/>, _ => <A/> };",
        true,
    );
}

#[test]
fn row_fragment_is_jsx() {
    assert_row("<>", "<>;", true);
}

#[test]
fn row_stray_closing_tag_is_jsx() {
    assert_row("</div> with nothing open", "</div>;", true);
}

#[test]
fn row_dotted_tag_name_is_jsx() {
    assert_row("<Foo.Bar />", "<Foo.Bar />;", true);
}

#[test]
fn row_namespaced_name_is_jsx() {
    assert_row("<svg:rect />", "<svg:rect />;", true);
}

#[test]
fn row_spread_attribute_is_jsx() {
    assert_row("<A {...props} />", "<A {...props} />;", true);
}

#[test]
fn row_generic_arguments_on_tag_is_jsx() {
    assert_row(
        "<List<T> items={items} />",
        "<List<T> items={items} />;",
        true,
    );
}

#[test]
fn row_truncated_then_rbrace_is_jsx() {
    assert_row("<User` then `}`", "<User", true);
}

#[test]
fn row_reserved_keyword_tag_names_are_rust() {
    for snippet in ["<fn />", "<for />", "<dyn />"] {
        assert_row(snippet, &format!("{snippet};"), false);
    }
}

#[test]
fn row_for_with_lifetime_binder_is_rust() {
    assert_row("<for<'a> fn(&'a T)>::x", "<for<'a> fn(&'a T)>::x;", false);
}

#[test]
fn row_type_macro_is_rust() {
    assert_row("<ty!()>::default()", "<ty!()>::default();", false);
}

#[test]
fn row_bang_not_a_type_macro_is_jsx() {
    assert_row("<div!>", "<div!>;", true);
}

// ---------------------------------------------------------------------
// Item 3 (H4, decision D2): expression-position fixes.
// ---------------------------------------------------------------------

fn assert_source_has_jsx(label: &str, source: &str, expect_jsx: bool) {
    let parsed = outou_syntax::parse(source);
    let found = contains_jsx_in_file(&parsed.file);
    assert_eq!(
        found, expect_jsx,
        "{label}: expected JSX={expect_jsx}, got JSX={found} (source: {source:?})"
    );
}

#[test]
fn statement_boundary_brace_opens_expression_position() {
    let cases: &[(&str, &str)] = &[
        (
            "return-inside-if-then-tail-element",
            "fn view() -> E { if !ready { return <Spinner/>; }\n <div>hi</div> }",
        ),
        (
            "for-loop-then-element",
            "fn f() { for i in items { log(i); }\n<div/>; }",
        ),
        ("field-init", "fn f() { Props { child: <A/> }; }"),
        ("if-condition", "fn f() { if <A/> {} }"),
        ("while-condition", "fn f() { while <A/> {} }"),
        ("match-scrutinee", "fn f() { match <A/> {} }"),
        (
            "local-struct-then-element",
            "fn f() { struct Local {}\n<A/>; }",
        ),
    ];
    for (label, source) in cases {
        assert_source_has_jsx(label, source, true);
    }
}

/// D2's documented residual risk, named for the Rust reading a human
/// expects (`{ 1 } < y` is a block compared with `<`, so the whole
/// statement "stays Rust"): treating every `}` as a statement boundary
/// (decision D2, needed so `if !ready { return <A/>; }` followed by real
/// JSX is recognized) cannot also tell a block used as a comparison
/// *operand* apart from one that ends a statement, since both look
/// identical from the previous-token view `from_prev` uses (grammar §4
/// rule 1 does not specify a full statement grammar). So `<` right after
/// `{ 1 }` is — incorrectly — treated as an expression position and
/// committed to JSX by rule 2's catch-all, producing a broken `<y>`
/// element and two diagnostics instead of leaving `{ 1 } < y` alone as a
/// comparison. This is accepted (D2: "vanishingly rare") rather than
/// fixed, and pinned here so the trade-off stays visible instead of
/// silently regressing further.
#[test]
fn block_operand_comparison_stays_rust() {
    let source = "fn f() { let b = { 1 } < y; }";
    assert_source_has_jsx("block-as-comparison-operand", source, true);
    let parsed = outou_syntax::parse(source);
    assert_eq!(
        parsed.diagnostics.len(),
        2,
        "expected the documented two-diagnostic divergence: {:?}",
        parsed.diagnostics
    );
}

#[test]
fn field_init_colon_is_expression_position() {
    assert_source_has_jsx(
        "field-init-colon",
        "fn f() { Props { child: <A/> }; }",
        true,
    );
}

// ---------------------------------------------------------------------
// Item 8 (H8): hyphenated tag names are one `JsxName`, not `ident` `-`
// `ident` (grammar §5: `JsxName := IDENTIFIER_OR_KEYWORD ('-'
// IDENTIFIER_OR_KEYWORD)*`).
// ---------------------------------------------------------------------

fn find_jsx_element(file: &ast::File) -> Option<&ast::JsxElement> {
    for item in &file.items {
        if let ast::Item::Function(f) = item {
            for expr in f.body.statements.iter().chain(f.body.tail.as_deref()) {
                if let ast::Expr::Jsx(el) = expr {
                    return Some(el);
                }
            }
        }
    }
    None
}

fn tag_name(element: &ast::JsxElement) -> &str {
    match &element.open {
        ast::JsxTag::Named { name, .. } => &name.name,
        ast::JsxTag::Incomplete(tag) => tag.name.as_ref().map(|n| n.name.as_str()).unwrap_or(""),
    }
}

#[test]
fn hyphenated_tag_name_is_one_name() {
    let source = "fn f() { <my-element />; }";
    let parsed = outou_syntax::parse(source);
    assert!(
        parsed.diagnostics.is_empty(),
        "source: {source}: {:?}",
        parsed.diagnostics
    );
    let element = find_jsx_element(&parsed.file).expect("jsx element");
    assert_eq!(tag_name(element), "my-element");
    assert!(element.attributes.is_empty(), "{:?}", element.attributes);

    let source2 = "fn f() { <my-element>x</my-element>; }";
    let parsed2 = outou_syntax::parse(source2);
    assert!(
        parsed2.diagnostics.is_empty(),
        "source: {source2}: {:?}",
        parsed2.diagnostics
    );
    let element2 = find_jsx_element(&parsed2.file).expect("jsx element");
    assert_eq!(tag_name(element2), "my-element");
    assert!(matches!(
        &element2.close,
        Some(ast::JsxTag::Named { name, .. }) if name.name == "my-element"
    ));
}

// ---------------------------------------------------------------------
// Item 10 (M2): a non-ASCII char literal used as a comparison operand
// must not be mistaken for a lifetime, which would make the following
// `<` look like a fresh JSX candidate instead of the less-than operator.
// ---------------------------------------------------------------------

#[test]
fn char_literal_operand_keeps_less_than_rust() {
    let source = "fn f() { <p>{'é' < x}</p>; }";
    let parsed = outou_syntax::parse(source);
    assert!(
        parsed.diagnostics.is_empty(),
        "source: {source}: {:?}",
        parsed.diagnostics
    );
}

// ---------------------------------------------------------------------
// Item 19 (M11): `JsxName` conformance — a raw identifier (`r#type`) is a
// single name, and a name may never start with a digit.
// ---------------------------------------------------------------------

#[test]
fn raw_identifier_tag_name_round_trips() {
    let source = "fn f() { <r#type></r#type>; }";
    let parsed = outou_syntax::parse(source);
    assert!(
        parsed.diagnostics.is_empty(),
        "source: {source}: {:?}",
        parsed.diagnostics
    );
}

#[test]
fn digit_leading_attribute_name_is_rejected() {
    let source = "fn f() { <A 123=\"x\" />; }";
    let parsed = outou_syntax::parse(source);
    assert!(
        !parsed.diagnostics.is_empty(),
        "expected a diagnostic for a digit-leading attribute name, got none"
    );
}

#[test]
fn type_colon_still_rust() {
    let cases: &[(&str, &str)] = &[
        ("let-type-ascription", "fn f() { let x: <T as Tr>::A = y; }"),
        ("fn-param-type", "fn f(x: <A<B>>::C) {}"),
        ("turbofish", "fn f() { f::<T>(); }"),
    ];
    for (label, source) in cases {
        assert_source_has_jsx(label, source, false);
    }
}
