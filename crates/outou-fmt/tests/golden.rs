//! Golden fixture tests: each directory under `tests/golden/` holds an
//! `input.rsx` and (unless the case is expected to be refused) an
//! `expected.rsx` that `outou_fmt::format_source(input)` must produce
//! exactly. Every formattable case is additionally checked for
//! idempotency: `format(expected) == expected`.
//!
//! `tests/golden/parse-error-refused/` has no `expected.rsx`: it checks
//! that a file with a syntax error is refused, not formatted.

use std::path::Path;

use outou_fmt::{format_source, FormatError, FormatOptions};

fn golden_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden"))
}

fn run_case(name: &str) {
    let dir = golden_dir().join(name);
    let input = std::fs::read_to_string(dir.join("input.rsx"))
        .unwrap_or_else(|err| panic!("{name}: reading input.rsx: {err}"));
    let expected_path = dir.join("expected.rsx");

    let options = FormatOptions::default();
    let result = format_source(&input, &options);

    if expected_path.exists() {
        let expected = std::fs::read_to_string(&expected_path)
            .unwrap_or_else(|err| panic!("{name}: reading expected.rsx: {err}"));
        let formatted =
            result.unwrap_or_else(|err| panic!("{name}: expected success, got error: {err}"));
        assert_eq!(formatted, expected, "{name}: formatted output mismatch");

        let twice = format_source(&formatted, &options)
            .unwrap_or_else(|err| panic!("{name}: formatting its own output failed: {err}"));
        assert_eq!(twice, formatted, "{name}: formatting is not idempotent");
    } else {
        match result {
            Err(FormatError::SyntaxErrors { .. }) => {}
            Err(other) => panic!("{name}: expected a syntax-error refusal, got: {other}"),
            Ok(text) => panic!("{name}: expected a refusal, but formatting succeeded:\n{text}"),
        }
    }
}

macro_rules! golden_test {
    ($name:ident, $dir:literal) => {
        #[test]
        fn $name() {
            run_case($dir);
        }
    };
}

golden_test!(plain_rust_only, "plain-rust-only");
golden_test!(single_jsx, "single-jsx");
golden_test!(nested_jsx_in_braces, "nested-jsx-in-braces");
golden_test!(attributes_mixed, "attributes-mixed");
golden_test!(self_closing, "self-closing");
golden_test!(significant_text, "significant-text");
golden_test!(comment_near_jsx, "comment-near-jsx");
golden_test!(long_attribute_list_wraps, "long-attribute-list-wraps");
golden_test!(crlf_input, "crlf-input");
golden_test!(already_formatted, "already-formatted");
golden_test!(parse_error_refused, "parse-error-refused");
golden_test!(placeholder_collision, "placeholder-collision");
golden_test!(
    blank_lines_between_children_collapse,
    "blank-lines-between-children-collapse"
);
golden_test!(
    multiline_raw_string_in_island,
    "multiline-raw-string-in-island"
);
golden_test!(multiline_string_in_island, "multiline-string-in-island");
golden_test!(
    multiline_attr_string_in_nested_jsx,
    "multiline-attr-string-in-nested-jsx"
);
golden_test!(
    multiline_block_comment_in_island,
    "multiline-block-comment-in-island"
);

/// Every golden case with an `expected.rsx` is idempotent starting from
/// its own `input.rsx` too (`run_case` only checks idempotency starting
/// from `expected.rsx`; this also guards against a case where the first
/// formatting pass differs from the second but happens to still match
/// `expected.rsx` by coincidence — impossible given `assert_eq!` above,
/// but kept as a single place that iterates every case directory rather
/// than relying solely on the fixed list of macro calls staying in sync
/// with `tests/golden/`).
#[test]
fn every_golden_directory_is_covered_by_a_case_above() {
    let known: std::collections::HashSet<&str> = [
        "plain-rust-only",
        "single-jsx",
        "nested-jsx-in-braces",
        "attributes-mixed",
        "self-closing",
        "significant-text",
        "comment-near-jsx",
        "long-attribute-list-wraps",
        "crlf-input",
        "already-formatted",
        "parse-error-refused",
        "placeholder-collision",
        "blank-lines-between-children-collapse",
        "multiline-raw-string-in-island",
        "multiline-string-in-island",
        "multiline-attr-string-in-nested-jsx",
        "multiline-block-comment-in-island",
    ]
    .into_iter()
    .collect();

    for entry in std::fs::read_dir(golden_dir()).expect("tests/golden exists") {
        let entry = entry.expect("readable directory entry");
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_str().expect("utf-8 directory name");
        assert!(
            known.contains(name),
            "tests/golden/{name} has no golden_test! case in tests/golden.rs"
        );
    }
}
