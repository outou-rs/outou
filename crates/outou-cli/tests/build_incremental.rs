//! Issue #20 (A3): on the `outou build` CLI path, a single-file edit must
//! not rewrite a sibling unit's generated files. Byte-identical content is
//! not enough (that already held); the sibling's mtime must be untouched.

mod support;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use support::{copy_to_temp, fixtures_dir};

/// A fixed timestamp well in the past, so "was rewritten" is detectable
/// without depending on filesystem timestamp granularity.
fn long_ago() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_000_000_000)
}

fn set_mtime(path: &Path, when: SystemTime) {
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap_or_else(|e| panic!("opening {}: {e}", path.display()))
        .set_modified(when)
        .unwrap_or_else(|e| panic!("setting mtime of {}: {e}", path.display()));
}

fn mtime(path: &Path) -> SystemTime {
    fs::metadata(path)
        .unwrap_or_else(|e| panic!("stat {}: {e}", path.display()))
        .modified()
        .unwrap()
}

fn run_build(dir: &Path) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_outou"))
        .arg("build")
        .arg("--manifest-dir")
        .arg(dir)
        .output()
        .expect("running the `outou` binary");
    assert!(
        output.status.success(),
        "outou build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn editing_one_rsx_file_does_not_rewrite_sibling_units() {
    let dir = copy_to_temp(&fixtures_dir().join("mixed"), "incremental");
    run_build(&dir);

    let gen = dir.join("src/.generated");
    let edited = [
        gen.join("components/user.rs"),
        gen.join("components/user.rs.map.json"),
    ];
    let siblings = [
        gen.join("crate-root.rs"),
        gen.join("crate-root.rs.map.json"),
        gen.join("components.rs"),
        gen.join("components.rs.map.json"),
    ];
    for file in edited.iter().chain(&siblings) {
        set_mtime(file, long_ago());
    }

    let user = dir.join("src/components/user.rsx");
    let text = fs::read_to_string(&user).unwrap();
    fs::write(&user, text.replace("class=\"user\"", "class=\"user-card\"")).unwrap();

    let output = run_build(&dir);

    for file in &siblings {
        assert_eq!(
            mtime(file),
            long_ago(),
            "{} was rewritten by an edit to a different unit",
            file.display()
        );
    }
    for file in &edited {
        assert_ne!(
            mtime(file),
            long_ago(),
            "{} must be rewritten",
            file.display()
        );
    }
    assert!(fs::read_to_string(&edited[0])
        .unwrap()
        .contains("user-card"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("wrote 2 file(s), 4 unchanged,"),
        "summary should report written and unchanged counts: {stdout}"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_unchanged_rebuild_writes_nothing() {
    let dir = copy_to_temp(&fixtures_dir().join("mixed"), "incremental-noop");
    run_build(&dir);
    let gen = dir.join("src/.generated");
    let files = [
        gen.join("crate-root.rs"),
        gen.join("crate-root.rs.map.json"),
        gen.join("components.rs"),
        gen.join("components.rs.map.json"),
        gen.join("components/user.rs"),
        gen.join("components/user.rs.map.json"),
    ];
    for file in &files {
        set_mtime(file, long_ago());
    }

    let output = run_build(&dir);

    for file in &files {
        assert_eq!(mtime(file), long_ago(), "{} was rewritten", file.display());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("wrote 0 file(s), 6 unchanged, removed 0 stale file(s)"),
        "unexpected summary: {stdout}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_changed_generated_file_is_restored_even_when_the_source_is_untouched() {
    let dir = copy_to_temp(&fixtures_dir().join("mixed"), "incremental-restore");
    run_build(&dir);
    let target = dir.join("src/.generated/components.rs");
    let expected = fs::read_to_string(&target).unwrap();
    fs::write(&target, "// hand edited").unwrap();

    run_build(&dir);

    assert_eq!(fs::read_to_string(&target).unwrap(), expected);
    fs::remove_dir_all(&dir).ok();
}

/// Staleness guard: a unit's output also depends on the plan, not just its
/// own source. Adding a child module changes the parent's `#[path]`
/// declarations even though the parent's `.rsx` text is the only thing
/// edited; its generated file must be rewritten, and the new child written.
#[test]
fn a_plan_level_change_still_rewrites_the_affected_parent() {
    let dir = copy_to_temp(&fixtures_dir().join("mixed"), "incremental-plan");
    run_build(&dir);
    let parent = dir.join("src/.generated/components.rs");
    set_mtime(&parent, long_ago());

    let components = dir.join("src/components.rsx");
    let text = fs::read_to_string(&components).unwrap();
    fs::write(&components, format!("pub mod extra;\n{text}")).unwrap();
    fs::write(
        dir.join("src/components/extra.rsx"),
        "pub fn answer() -> u32 { 42 }\n",
    )
    .unwrap();

    run_build(&dir);

    assert_ne!(mtime(&parent), long_ago(), "parent must be rewritten");
    assert!(fs::read_to_string(&parent).unwrap().contains("extra"));
    assert!(dir.join("src/.generated/components/extra.rs").exists());
    fs::remove_dir_all(&dir).ok();
}
