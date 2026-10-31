//! `outou build` and `outou cargo` print the front end's warnings the way
//! `outou check` does (issue #31): to standard error, at the `.rsx`
//! position, without changing the exit code. A rebuild that writes nothing
//! prints them again, because they come from the source, not from the write.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A `<select value>` whose `<option>` has no `value`: a front-end warning.
const WARNING_SOURCE: &str = r#"use outou::prelude::*;

#[component]
fn App() -> Element {
    <select value="a">
        <option>A</option>
    </select>
}
"#;

const WARNING_TEXT: &str = "warning: this `<option>` has no `value`";
const WARNING_POSITION: &str = " --> src/main.rsx:6:10";
const CARGO_MARKER: &str = "FAKE-CARGO-RAN";
const BROKEN_SOURCE: &str =
    "use outou::prelude::*;\n\n#[component]\nfn App() -> Element {\n    <div>\n}\n";

struct Crate {
    dir: PathBuf,
    fake_cargo: PathBuf,
}

impl Crate {
    fn new(label: &str, source: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("outou-cli-warn-{label}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/main.rsx"), source).unwrap();
        let dir = dir.canonicalize().unwrap();
        let fake_cargo = dir.join("fake-cargo.sh");
        fs::write(
            &fake_cargo,
            format!(
                "#!/bin/sh\necho {CARGO_MARKER} >&2\ntouch \"{}/cargo-ran\"\nexit 0\n",
                dir.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755)).unwrap();
        Self { dir, fake_cargo }
    }

    fn outou(&self, args: &[&str]) -> Output {
        self.outou_in(&self.dir, args)
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.dir.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn cargo_ran(&self) -> bool {
        self.dir.join("cargo-ran").exists()
    }

    fn outou_in(&self, cwd: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_outou"))
            .args(args)
            .current_dir(cwd)
            .env("CARGO", &self.fake_cargo)
            .env("NO_COLOR", "1")
            .output()
            .expect("running the `outou` binary")
    }
}

impl Drop for Crate {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.dir).ok();
    }
}

fn assert_warns(output: &Output) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "warnings must not fail: {stderr}");
    assert_eq!(stderr.matches(WARNING_TEXT).count(), 1, "stderr: {stderr}");
    assert!(stderr.contains(WARNING_POSITION), "stderr was: {stderr}");
    for forbidden in [".generated", "dioxus", "rsx!"] {
        assert!(!stderr.contains(forbidden), "{forbidden} leaked: {stderr}");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("warning:"), "stdout was: {stdout}");
}

#[test]
fn build_prints_front_end_warnings_to_stderr_and_succeeds() {
    let krate = Crate::new("build-warn", WARNING_SOURCE);
    assert_warns(&krate.outou(&["build"]));
}

#[test]
fn build_prints_the_warnings_again_when_nothing_is_rewritten() {
    let krate = Crate::new("build-warn-twice", WARNING_SOURCE);
    krate.outou(&["build"]);
    let second = krate.outou(&["build"]);
    assert!(String::from_utf8_lossy(&second.stdout).contains("wrote 0 file(s)"));
    assert_warns(&second);
}

#[test]
fn cargo_check_prints_front_end_warnings_before_running_cargo() {
    let krate = Crate::new("cargo-warn", WARNING_SOURCE);
    let output = krate.outou(&["cargo", "check"]);
    assert_warns(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let warning = stderr.find(WARNING_TEXT).expect("warning printed");
    let cargo = stderr.find(CARGO_MARKER).expect("fake cargo ran");
    assert!(warning < cargo, "warning must precede cargo: {stderr}");
}

fn two_member_workspace(label: &str, second_source: &str) -> Crate {
    let krate = Crate::new(label, WARNING_SOURCE);
    fs::remove_file(krate.dir.join("src/main.rsx")).unwrap();
    krate.write("Cargo.toml", "[workspace]\nmembers = [\"a\", \"b\"]\n");
    for (name, source) in [("a", WARNING_SOURCE), ("b", second_source)] {
        krate.write(
            &format!("{name}/Cargo.toml"),
            &format!("[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n"),
        );
        krate.write(&format!("{name}/src/main.rsx"), source);
    }
    krate
}

#[test]
fn a_workspace_root_shows_each_members_warning_once_with_its_path() {
    let krate = two_member_workspace(
        "ws-warn",
        "use outou::prelude::*;\n\n#[component]\nfn App() -> Element {\n    <div></div>\n}\n",
    );
    let output = krate.outou(&["build"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr: {stderr}");
    assert_eq!(stderr.matches(WARNING_TEXT).count(), 1, "stderr: {stderr}");
    assert!(
        stderr.contains(" --> a/src/main.rsx:6:10"),
        "stderr: {stderr}"
    );
}

#[test]
fn a_later_unit_with_a_syntax_error_fails_and_cargo_does_not_run() {
    let krate = two_member_workspace("ws-error", BROKEN_SOURCE);
    let output = krate.outou(&["cargo", "check"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error"), "stderr: {stderr}");
    assert!(!stderr.contains(CARGO_MARKER), "stderr: {stderr}");
    assert!(!krate.cargo_ran());
}

#[test]
fn a_crate_outside_the_current_directory_shows_its_absolute_path() {
    let krate = Crate::new("outside", WARNING_SOURCE);
    krate.write("elsewhere/keep", "");
    let cwd = krate.dir.join("elsewhere");
    let outside = Crate::new("outside-cwd", "");
    let target = outside.dir.display().to_string();
    fs::write(outside.dir.join("src/main.rsx"), WARNING_SOURCE).unwrap();
    let output = krate.outou_in(&cwd, &["build", "--manifest-dir", &target]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr: {stderr}");
    let expected = format!(" --> {target}/src/main.rsx:6:10");
    assert!(stderr.contains(&expected), "stderr: {stderr}");
}

#[test]
fn cargo_with_a_manifest_path_outside_the_current_directory_shows_the_absolute_path() {
    let krate = Crate::new("outside-cargo", WARNING_SOURCE);
    let outside = Crate::new("outside-cargo-target", WARNING_SOURCE);
    let manifest = outside.dir.join("Cargo.toml");
    fs::write(&manifest, "[package]\nname = \"x\"\nversion = \"0.0.0\"\n").unwrap();
    let output = krate.outou_in(
        &krate.dir,
        &[
            "cargo",
            "check",
            "--manifest-path",
            &manifest.display().to_string(),
        ],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected = format!(" --> {}/src/main.rsx:6:10", outside.dir.display());
    assert!(stderr.contains(&expected), "stderr: {stderr}");
}

#[test]
fn a_clean_source_prints_no_warning() {
    let krate = Crate::new(
        "build-clean",
        "use outou::prelude::*;\n\n#[component]\nfn App() -> Element {\n    <div></div>\n}\n",
    );
    let output = krate.outou(&["build"]);
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("warning"));
}
