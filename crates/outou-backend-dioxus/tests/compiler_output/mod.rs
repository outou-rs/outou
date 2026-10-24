//! Reading cargo's stderr in the compile tests.

/// Removes ANSI escape sequences (`\x1b[1m\x1b[33m`): CI sets
/// `CARGO_TERM_COLOR=always`, which colours diagnostics.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
        }
    }
    out
}

/// True when cargo's stderr holds a rustc warning diagnostic: a line that
/// starts with `warning:` or `warning[`. Cargo's progress lines
/// (`Compiling warnings v0.2.1`) merely mention the word, and the summary
/// `warning: build failed` is a warning line too, which is fine since the
/// caller has already asserted success.
pub fn has_compiler_warning(stderr: &str) -> bool {
    strip_ansi(stderr).lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("warning:") || line.starts_with("warning[")
    })
}

#[test]
fn a_progress_line_naming_the_warnings_crate_is_not_a_warning() {
    assert!(!has_compiler_warning("   Compiling warnings v0.2.1\n"));
    assert!(!has_compiler_warning(
        "\x1b[1m\x1b[92m   Compiling\x1b[0m warnings v0.2.1\n    Checking error-code v1.0.0\n"
    ));
}

#[test]
fn coloured_and_plain_warning_diagnostics_are_detected() {
    assert!(has_compiler_warning(
        "\x1b[1m\x1b[33mwarning\x1b[0m\x1b[1m: unused variable: `x`\x1b[0m\n"
    ));
    assert!(has_compiler_warning("warning: unused variable: `x`\n"));
    assert!(has_compiler_warning("warning[E0123]: something\n"));
    assert!(has_compiler_warning("  warning: indented\n"));
    assert!(has_compiler_warning(
        "   Compiling a v1\nwarning: build failed, waiting\n"
    ));
}

#[test]
fn clean_output_has_no_warning() {
    assert!(!has_compiler_warning(""));
    assert!(!has_compiler_warning(
        "   Compiling x v0.0.0\n    Finished `dev` profile in 1s\n     Running `target/debug/x`\n"
    ));
}
