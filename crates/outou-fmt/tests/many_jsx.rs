//! A file with many top-level JSX elements: placeholders are numbered, and
//! `__outou_fmt_jsx_1` is a prefix of `__outou_fmt_jsx_10`. Each one must be
//! found as a whole identifier (ADR 0011), not as a substring.

use outou_fmt::{format_source, FormatOptions};

fn source_with(count: usize) -> String {
    (0..count)
        .map(|n| format!("fn f{n}() -> Element {{\n    <li>a</li>\n}}\n"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_formats_idempotently(count: usize) {
    let source = source_with(count);
    let options = FormatOptions::default();
    let formatted = format_source(&source, &options)
        .unwrap_or_else(|err| panic!("{count} elements must format: {err}"));
    assert_eq!(formatted.matches("<li>a</li>").count(), count);
    assert!(!formatted.contains("__outou_fmt_jsx"));
    assert_eq!(
        format_source(&formatted, &options).unwrap(),
        formatted,
        "{count} elements: formatting is idempotent"
    );
}

#[test]
fn eleven_top_level_elements_format() {
    assert_formats_idempotently(11);
}

#[test]
fn twelve_top_level_elements_format() {
    assert_formats_idempotently(12);
}

#[test]
fn a_hundred_top_level_elements_format() {
    assert_formats_idempotently(100);
}
