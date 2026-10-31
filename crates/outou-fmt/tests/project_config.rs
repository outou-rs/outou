//! The formatter must behave as `rustfmt` does for the project the formatted
//! file belongs to: the nearest `rustfmt.toml` / `.rustfmt.toml` and the
//! crate's edition apply, wherever the process was started from.

use std::path::{Path, PathBuf};

use outou_fmt::{format_source, FormatError, FormatOptions};

/// Points the user-level configuration lookup of every `rustfmt` this test
/// binary starts at an empty directory, once, so a developer's own
/// `~/.config/rustfmt/rustfmt.toml` cannot change the results. Production
/// behaviour is untouched: there `rustfmt`'s user-level config is honoured.
fn isolate_user_config() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let home = std::env::temp_dir().join(format!("outou-fmt-it-home-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_CONFIG_HOME", &home);
    });
}

/// A fresh, empty directory unique to `name`.
fn project(name: &str) -> PathBuf {
    isolate_user_config();
    let dir = std::env::temp_dir().join(format!("outou-fmt-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn options_for(path: &Path) -> FormatOptions {
    FormatOptions {
        source_path: Some(path.to_path_buf()),
        ..FormatOptions::default()
    }
}

/// Valid only in edition 2015: `async` is a keyword from 2018 on, and every
/// `rustfmt` rejects that in its parser (unlike, say, let chains, which
/// toolchains up to 1.88 accept in any edition).
const ASYNC_NAME: &str = "fn async() {}\n\nfn f() -> Element {\n    <div />\n}\n";

#[test]
fn the_edition_comes_from_the_nearest_cargo_toml() {
    let dir = project("edition-package");
    write(
        &dir.join("Cargo.toml"),
        "[package]\nname = \"p\"\nversion = \"0.1.0\"\nedition = \"2015\"\n",
    );
    let file = dir.join("src/a.rsx");
    let out =
        format_source(ASYNC_NAME, &options_for(&file)).expect("2015 allows `async` as a name");
    assert_eq!(out, ASYNC_NAME);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn edition_workspace_true_reads_the_workspace_root() {
    let dir = project("edition-workspace");
    write(
        &dir.join("Cargo.toml"),
        "[workspace]\nmembers = [\"member\"]\n[workspace.package]\nedition = \"2015\"\n",
    );
    write(
        &dir.join("member/Cargo.toml"),
        "[package]\nname = \"m\"\nversion = \"0.1.0\"\nedition.workspace = true\n",
    );
    let file = dir.join("member/src/a.rsx");
    let out = format_source(ASYNC_NAME, &options_for(&file)).expect("inherited 2015");
    assert_eq!(out, ASYNC_NAME);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_edition_falls_back_to_2021_without_a_cargo_toml() {
    let dir = project("edition-fallback");
    let file = dir.join("src/a.rsx");
    let err = format_source(ASYNC_NAME, &options_for(&file)).unwrap_err();
    assert!(matches!(err, FormatError::RustfmtFailed { .. }), "{err:?}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_workspace_only_manifest_stops_the_search_for_an_edition() {
    let dir = project("edition-workspace-only");
    // An unrelated enclosing crate (2015) must not leak into the nested
    // workspace root, which names no edition: 2021 applies.
    write(
        &dir.join("Cargo.toml"),
        "[package]\nname = \"outer\"\nversion = \"0.1.0\"\nedition = \"2015\"\n",
    );
    write(&dir.join("inner/Cargo.toml"), "[workspace]\nmembers = []\n");
    let file = dir.join("inner/src/a.rsx");
    let err = format_source(ASYNC_NAME, &options_for(&file)).unwrap_err();
    assert!(matches!(err, FormatError::RustfmtFailed { .. }), "{err:?}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_workspace_only_manifest_supplies_its_workspace_package_edition() {
    let dir = project("edition-workspace-pkg");
    write(
        &dir.join("Cargo.toml"),
        "[workspace]\nmembers = []\n[workspace.package]\nedition = \"2015\"\n",
    );
    let file = dir.join("src/a.rsx");
    assert!(format_source(ASYNC_NAME, &options_for(&file)).is_ok());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_syntax_error_is_refused_without_running_rustfmt() {
    let options = FormatOptions {
        rustfmt_program: "/nonexistent/rustfmt".to_string(),
        ..FormatOptions::default()
    };
    let err = format_source("fn f() {\n<div cl\n", &options).unwrap_err();
    assert!(matches!(err, FormatError::SyntaxErrors { .. }), "{err:?}");
}

#[cfg(unix)]
#[test]
fn a_symlinked_file_is_formatted_with_its_real_projects_settings() {
    let dir = project("symlink");
    write(&dir.join("real/rustfmt.toml"), "tab_spaces = 2\n");
    write(&dir.join("real/src/a.rsx"), "");
    write(&dir.join("elsewhere/placeholder"), "");
    let link = dir.join("elsewhere/link.rsx");
    std::os::unix::fs::symlink(dir.join("real/src/a.rsx"), &link).unwrap();
    let out = format_source("fn f() -> Element { <div/> }\n", &options_for(&link)).unwrap();
    assert_eq!(out, "fn f() -> Element {\n  <div />\n}\n");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_explicit_edition_overrides_detection() {
    let dir = project("edition-explicit");
    write(
        &dir.join("Cargo.toml"),
        "[package]\nname = \"p\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    let options = FormatOptions {
        edition: Some("2015".to_string()),
        ..options_for(&dir.join("src/a.rsx"))
    };
    assert!(format_source(ASYNC_NAME, &options).is_ok());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rustfmt_toml_of_the_formatted_files_project_is_found_from_any_cwd() {
    let dir = project("config-discovery");
    write(&dir.join("rustfmt.toml"), "tab_spaces = 2\n");
    let out = format_source(
        "fn f() -> Element { <div/> }\n",
        &options_for(&dir.join("src/a.rsx")),
    )
    .unwrap();
    assert_eq!(out, "fn f() -> Element {\n  <div />\n}\n");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_dot_rustfmt_toml_is_found_too() {
    let dir = project("config-discovery-dot");
    write(&dir.join(".rustfmt.toml"), "tab_spaces = 2\n");
    let out = format_source(
        "fn f() -> Element { <div/> }\n",
        &options_for(&dir.join("src/a.rsx")),
    )
    .unwrap();
    assert_eq!(out, "fn f() -> Element {\n  <div />\n}\n");
    std::fs::remove_dir_all(&dir).ok();
}

fn format_in(dir: &Path, source: &str) -> Result<String, FormatError> {
    format_source(source, &options_for(&dir.join("src/a.rsx")))
}

fn max_line_width(text: &str) -> usize {
    text.lines()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0)
}

const THREE_ATTRIBUTES: &str =
    "fn f() -> Element {\n    <div class=\"aaaaaaaa\" id=\"bbbbbbbbbbbb\" title=\"cccccccccccc\" />\n}\n";

#[test]
fn without_a_config_the_width_budget_is_rustfmts_default_of_100() {
    let dir = project("width-default");
    let out = format_in(&dir, THREE_ATTRIBUTES).unwrap();
    assert_eq!(out, THREE_ATTRIBUTES);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn max_width_is_read_from_rustfmt_toml() {
    let dir = project("width-60");
    write(&dir.join("rustfmt.toml"), "max_width = 60\n");
    let out = format_in(&dir, THREE_ATTRIBUTES).unwrap();
    assert_eq!(
        out,
        "fn f() -> Element {\n    <div\n        class=\"aaaaaaaa\"\n        id=\"bbbbbbbbbbbb\"\n        title=\"cccccccccccc\"\n    />\n}\n"
    );
    assert_eq!(format_in(&dir, &out).unwrap(), out, "idempotent");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_larger_max_width_keeps_a_longer_element_on_one_line() {
    let dir = project("width-140");
    write(&dir.join("rustfmt.toml"), "max_width = 140\n");
    let source = format!(
        "fn f() -> Element {{\n    <div class=\"{}\" id=\"x\" />\n}}\n",
        "a".repeat(110)
    );
    assert_eq!(format_in(&dir, &source).unwrap(), source);
    std::fs::remove_dir_all(&dir).ok();
}

/// Backlog: a placeholder after `let name = ` at column 35+ was measured at
/// the line's indent, so a 115-column line stayed unwrapped.
#[test]
fn the_fit_check_counts_the_column_the_element_starts_at() {
    let dir = project("width-column");
    let prefix = "    let some_long_variable_name_here = ";
    // Element 75 columns wide: the line is 4 + 35 + 75 + 1 = 115 columns.
    let element = format!("<div class=\"{}\" id=\"x\" />", "a".repeat(75 - 23));
    assert_eq!(element.chars().count(), 75);
    let source = format!("fn f() -> Element {{\n{prefix}{element};\n    x\n}}\n");
    let out = format_in(&dir, &source).unwrap();
    assert!(max_line_width(&out) <= 100, "{out}");
    assert!(out.contains("<div\n"), "{out}");
    assert_eq!(format_in(&dir, &out).unwrap(), out, "idempotent");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_fit_check_counts_the_semicolon_after_the_element() {
    let dir = project("width-semicolon");
    let prefix = "    let v = ";
    // 4 + 8 + element + 1 == 100 exactly when element is 87 wide.
    let fits_exactly = format!("<div class=\"{}\" id=\"x\" />", "a".repeat(87 - 23));
    let one_more = format!("<div class=\"{}\" id=\"x\" />", "a".repeat(88 - 23));
    let ok = format!("fn f() -> Element {{\n{prefix}{fits_exactly};\n    v\n}}\n");
    assert_eq!(format_in(&dir, &ok).unwrap(), ok);
    let over = format!("fn f() -> Element {{\n{prefix}{one_more};\n    v\n}}\n");
    let out = format_in(&dir, &over).unwrap();
    assert!(out.contains("<div\n"), "{out}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_island_inside_an_element_counts_the_columns_of_its_enclosing_indent() {
    let dir = project("width-island-base");
    // Inside `<div>{ ... }</div>` the `<span ... />` starts at column 12.
    let span = format!("<span class=\"{}\" id=\"x\" />", "a".repeat(90 - 24));
    assert_eq!(span.chars().count(), 90);
    let source =
        format!("fn f() -> Element {{\n    <div>\n        {{\n            if c {{\n                {span}\n            }} else {{\n                <b />\n            }}\n        }}\n    </div>\n}}\n");
    let out = format_in(&dir, &source).unwrap();
    assert!(max_line_width(&out) <= 100, "{out}");
    assert_eq!(format_in(&dir, &out).unwrap(), out, "idempotent");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_indent_unit_follows_tab_spaces_inside_an_expression_island() {
    let dir = project("indent-unit-2");
    write(&dir.join("rustfmt.toml"), "tab_spaces = 2\n");
    let source = "fn f() -> Element {\n<div>{ if c { <a/> } else { <b/> } }</div>\n}\n";
    let out = format_in(&dir, source).unwrap();
    assert_eq!(
        out,
        "fn f() -> Element {\n  <div>\n    {\n      if c {\n        <a />\n      } else {\n        <b />\n      }\n    }\n  </div>\n}\n"
    );
    assert_eq!(format_in(&dir, &out).unwrap(), out, "idempotent");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn hard_tabs_in_rustfmt_toml_is_still_contained_to_spaces() {
    let dir = project("hard-tabs");
    write(
        &dir.join("rustfmt.toml"),
        "hard_tabs = true\ntab_spaces = 2\n",
    );
    let out = format_in(&dir, "fn f() -> Element { <div/> }\n").unwrap();
    assert_eq!(out, "fn f() -> Element {\n  <div />\n}\n");
    std::fs::remove_dir_all(&dir).ok();
}

const CRLF_SOURCE: &str = "use outou::prelude::*;\r\n\r\n#[component]\r\nfn Crlf() -> Element {\r\n<div class=\"a\"><p>\r\n  hello\r\n  world</p>{if c { <b/> } else { <i/> }}</div>\r\n}\r\n";

/// No `\n` that is not part of a `\r\n`.
fn is_pure_crlf(text: &str) -> bool {
    text.matches('\n').count() == text.matches("\r\n").count() && !text.is_empty()
}

fn lf_only(text: &str) -> String {
    text.replace("\r\n", "\n")
}

#[test]
fn crlf_input_is_formatted_like_its_lf_twin_and_stays_crlf() {
    let dir = project("crlf-auto");
    let crlf = format_in(&dir, CRLF_SOURCE).unwrap();
    let lf = format_in(&dir, &lf_only(CRLF_SOURCE)).unwrap();
    assert!(is_pure_crlf(&crlf), "{crlf:?}");
    assert_eq!(lf_only(&crlf), lf);
    assert!(!lf.contains('\r'));
    assert_eq!(format_in(&dir, &crlf).unwrap(), crlf, "idempotent");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn newline_style_unix_turns_crlf_input_into_lf() {
    let dir = project("crlf-unix");
    write(&dir.join("rustfmt.toml"), "newline_style = \"Unix\"\n");
    let out = format_in(&dir, CRLF_SOURCE).unwrap();
    assert!(!out.contains('\r'), "{out:?}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn newline_style_windows_turns_lf_input_into_crlf() {
    let dir = project("lf-windows");
    write(&dir.join("rustfmt.toml"), "newline_style = \"Windows\"\n");
    let out = format_in(&dir, &lf_only(CRLF_SOURCE)).unwrap();
    assert!(is_pure_crlf(&out), "{out:?}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn mixed_line_endings_follow_the_first_one_and_are_not_mixed_on_output() {
    let dir = project("mixed");
    let mixed = "fn a() {}\r\nfn b() { <div/> }\nfn c() {}\n";
    let out = format_in(&dir, mixed).unwrap();
    assert!(is_pure_crlf(&out), "{out:?}");
    let mixed_lf_first = "fn a() {}\nfn b() { <div/> }\r\nfn c() {}\r\n";
    let out = format_in(&dir, mixed_lf_first).unwrap();
    assert!(!out.contains('\r'), "{out:?}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_syntax_error_in_a_crlf_file_is_still_refused() {
    let dir = project("crlf-error");
    let err = format_in(&dir, "fn f() {\r\n<div cl\r\n").unwrap_err();
    assert!(matches!(err, FormatError::SyntaxErrors { .. }), "{err:?}");
    std::fs::remove_dir_all(&dir).ok();
}
