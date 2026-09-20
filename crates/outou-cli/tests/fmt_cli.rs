//! Integration tests for `outou fmt` (issue #13): formatting in place,
//! `--check` reporting without writing, and refusing a file with a
//! syntax error.

use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "outou-cli-fmt-it-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn outou_fmt_rewrites_an_unformatted_file_in_place() {
    let dir = temp_dir("rewrites");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("app.rsx");
    fs::write(&file, "fn f() { <div   /> }").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_outou"))
        .arg("fmt")
        .arg(&file)
        .output()
        .expect("running the `outou` binary");

    let formatted = fs::read_to_string(&file).unwrap();
    fs::remove_dir_all(&dir).ok();

    assert!(output.status.success());
    assert_eq!(formatted, "fn f() {\n    <div />\n}\n");
}

#[test]
fn outou_fmt_check_reports_unformatted_files_without_writing() {
    let dir = temp_dir("check");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("app.rsx");
    let original = "fn f() { <div   /> }";
    fs::write(&file, original).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_outou"))
        .arg("fmt")
        .arg("--check")
        .arg(&file)
        .output()
        .expect("running the `outou` binary");

    let unchanged = fs::read_to_string(&file).unwrap();
    fs::remove_dir_all(&dir).ok();

    assert!(!output.status.success(), "--check must exit non-zero");
    assert_eq!(unchanged, original, "--check must never write to disk");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("app.rsx"), "stdout: {stdout}");
}

#[test]
fn outou_fmt_check_succeeds_on_already_formatted_input() {
    let dir = temp_dir("check-clean");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("app.rsx");
    fs::write(&file, "fn f() {\n    <div />\n}\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_outou"))
        .arg("fmt")
        .arg("--check")
        .arg(&file)
        .output()
        .expect("running the `outou` binary");

    fs::remove_dir_all(&dir).ok();

    assert!(output.status.success());
}

#[test]
fn outou_fmt_refuses_a_file_with_a_syntax_error() {
    let dir = temp_dir("refuse");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("broken.rsx");
    let original = "fn f() { <div cl";
    fs::write(&file, original).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_outou"))
        .arg("fmt")
        .arg(&file)
        .output()
        .expect("running the `outou` binary");

    let unchanged = fs::read_to_string(&file).unwrap();
    fs::remove_dir_all(&dir).ok();

    assert!(!output.status.success());
    assert_eq!(unchanged, original, "a refused file must not be rewritten");
}

#[test]
fn outou_fmt_scans_a_directory_argument_recursively() {
    let dir = temp_dir("dir-arg");
    fs::create_dir_all(dir.join("nested")).unwrap();
    fs::write(dir.join("a.rsx"), "fn a() { <div   /> }").unwrap();
    fs::write(dir.join("nested/b.rsx"), "fn b() { <span   /> }").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_outou"))
        .arg("fmt")
        .arg(&dir)
        .output()
        .expect("running the `outou` binary");

    let a = fs::read_to_string(dir.join("a.rsx")).unwrap();
    let b = fs::read_to_string(dir.join("nested/b.rsx")).unwrap();
    fs::remove_dir_all(&dir).ok();

    assert!(output.status.success());
    assert_eq!(a, "fn a() {\n    <div />\n}\n");
    assert_eq!(b, "fn b() {\n    <span />\n}\n");
}
