//! Integration tests for `outou build`'s generated paths and file layout
//! (issue #8): every required build-pipeline fixture
//! (`tests/fixtures/modules/{mixed, cfg, cfg-duplicate, raw-ident,
//! root-name, inline, inline-dup, inline-dup-rs}`) and
//! `examples/phase0-app`, generated through the public `outou_cli::build`
//! API, plus stale-file cleanup and atomic-rebuild behavior.
//!
//! `tests/fixtures/modules/{path-attr,path-dirs,rs-to-rsx}` were built for
//! the resolver (`outou-modules`) and are exercised there and by
//! `crate::build::plan`'s own unit tests (the `RustDeclaresRsxChild`
//! error); they are not part of this build-pipeline suite. CLI-binary
//! behavior lives in `build_cli.rs`, real-compiler probes in
//! `build_compile.rs` (split out of one `tests/build.rs`, issue #8 fix
//! list step 10).

mod support;

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use outou_cli::build::{build, BuildOptions};

use support::{copy_to_temp, fixtures_dir, repo_root};

/// `files`, as paths relative to `dir`'s own `src/.generated`, forward
/// slash separated regardless of platform.
fn rel_generated_files(dir: &std::path::Path, files: &[PathBuf]) -> HashSet<String> {
    let generated_dir = dir.join("src/.generated");
    files
        .iter()
        .map(|f| {
            f.strip_prefix(&generated_dir)
                .unwrap_or(f)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

/// One required build-pipeline fixture: its name under
/// `tests/fixtures/modules/` and the generated `.rs` files (relative to
/// `src/.generated/`) `outou build` must produce for it.
struct FixtureCase {
    /// The fixture's directory name.
    name: &'static str,
    /// Every `.rs` file `outou build` must produce, relative to
    /// `src/.generated/`.
    expected_generated: &'static [&'static str],
}

/// Every required build-pipeline fixture (issue #8, Gate 2).
const FIXTURE_CASES: &[FixtureCase] = &[
    FixtureCase {
        name: "mixed",
        expected_generated: &["crate-root.rs", "components.rs", "components/user.rs"],
    },
    FixtureCase {
        name: "cfg",
        expected_generated: &["crate-root.rs", "optional.rs", "spaced.rs"],
    },
    FixtureCase {
        name: "cfg-duplicate",
        expected_generated: &["crate-root.rs", "imp.rs", "imp-1.rs"],
    },
    FixtureCase {
        name: "raw-ident",
        expected_generated: &["crate-root.rs", "type.rs", "type/child.rs"],
    },
    FixtureCase {
        name: "root-name",
        expected_generated: &["crate-root.rs"],
    },
    FixtureCase {
        name: "inline",
        expected_generated: &["crate-root.rs", "shell/panel.rs"],
    },
];

/// Asserts `text` (a generated `.rs` file's contents) has balanced
/// `{`/`}` — a cheap syntax sanity check that does not require `syn`.
/// `case`/`rel` name the fixture and file, for the failure message.
fn assert_balanced_braces(text: &str, case: &str, rel: &str) {
    let mut depth = 0i32;
    for byte in text.bytes() {
        match byte {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        assert!(depth >= 0, "fixture {case}: {rel} has unbalanced `}}`");
    }
    assert_eq!(depth, 0, "fixture {case}: {rel} has unbalanced braces");
}

#[test]
fn builds_every_required_fixture_with_the_expected_generated_files() {
    for case in FIXTURE_CASES {
        let dir = copy_to_temp(&fixtures_dir().join(case.name), case.name);
        let report = build(&BuildOptions::new(&dir))
            .unwrap_or_else(|e| panic!("building fixture {}: {e}", case.name));

        assert!(
            report.built,
            "fixture {} should have an rsx root",
            case.name
        );

        let got = rel_generated_files(&dir, &report.generated_files);
        let expected: HashSet<String> = case
            .expected_generated
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(got, expected, "fixture {}: generated .rs set", case.name);

        for rel in case.expected_generated {
            let generated = dir.join("src/.generated").join(rel);
            assert!(generated.exists(), "fixture {}: missing {rel}", case.name);
            let mut map = generated.clone();
            map.set_extension("rs.map.json");
            assert!(
                map.exists(),
                "fixture {}: missing source map for {rel}",
                case.name
            );
            // Every generated file is complete, balanced Rust text (a
            // cheap syntax sanity check that does not require `syn`).
            let text = fs::read_to_string(&generated).unwrap();
            assert_balanced_braces(&text, case.name, rel);
        }

        fs::remove_dir_all(&dir).ok();
    }
}

#[test]
fn mixed_fixture_mod_declarations_carry_the_right_path_attribute() {
    let dir = copy_to_temp(&fixtures_dir().join("mixed"), "mixed-paths");
    build(&BuildOptions::new(&dir)).expect("mixed builds");

    let crate_root = fs::read_to_string(dir.join("src/.generated/crate-root.rs")).unwrap();
    assert!(
        crate_root.contains("#[path = \"components.rs\"]\nmod components;"),
        "{crate_root}"
    );

    let components = fs::read_to_string(dir.join("src/.generated/components.rs")).unwrap();
    assert!(
        components.contains("#[path = \"components/user.rs\"]\npub mod user;"),
        "{components}"
    );
    assert!(
        components.contains("#[path = \"../components/button.rs\"]\npub mod button;"),
        "{components}"
    );
    // `button.rs` is plain Rust: never copied into `.generated/`, only
    // referenced from it via a relative `#[path]`.
    assert!(!dir.join("src/.generated/components/button.rs").exists());

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn cfg_fixture_preserves_cfg_attributes_on_the_generated_declaration() {
    let dir = copy_to_temp(&fixtures_dir().join("cfg"), "cfg-preserve");
    build(&BuildOptions::new(&dir)).expect("cfg builds");

    let crate_root = fs::read_to_string(dir.join("src/.generated/crate-root.rs")).unwrap();
    assert!(
        crate_root.contains("#[cfg(feature = \"x\")]\n#[path = \"optional.rs\"]\nmod optional;"),
        "{crate_root}"
    );
    // The original attribute's own (nonstandard) inner spacing is
    // reproduced verbatim; only `#[path]` is rewritten.
    assert!(
        crate_root.contains("#[ cfg(feature = \"y\") ]\n#[path = \"spaced.rs\"]\nmod spaced;"),
        "{crate_root}"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn cfg_duplicate_fixture_gives_each_branch_its_own_path() {
    let dir = copy_to_temp(&fixtures_dir().join("cfg-duplicate"), "cfg-dup-paths");
    build(&BuildOptions::new(&dir)).expect("cfg-duplicate builds");

    let crate_root = fs::read_to_string(dir.join("src/.generated/crate-root.rs")).unwrap();
    assert!(
        crate_root.contains("#[cfg(unix)]\n#[path = \"imp.rs\"]\nmod imp;"),
        "{crate_root}"
    );
    assert!(
        crate_root.contains("#[cfg(windows)]\n#[path = \"imp-1.rs\"]\nmod imp;"),
        "{crate_root}"
    );

    let unix_generated = fs::read_to_string(dir.join("src/.generated/imp.rs")).unwrap();
    assert!(unix_generated.contains("unix imp"), "{unix_generated}");
    let windows_generated = fs::read_to_string(dir.join("src/.generated/imp-1.rs")).unwrap();
    assert!(
        windows_generated.contains("windows imp"),
        "{windows_generated}"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn raw_ident_fixture_keeps_the_raw_spelling_on_the_generated_declaration() {
    let dir = copy_to_temp(&fixtures_dir().join("raw-ident"), "raw-ident");
    build(&BuildOptions::new(&dir)).expect("raw-ident builds");

    let crate_root = fs::read_to_string(dir.join("src/.generated/crate-root.rs")).unwrap();
    assert!(
        crate_root.contains("#[path = \"type.rs\"]\nmod r#type;"),
        "{crate_root}"
    );

    let type_generated = fs::read_to_string(dir.join("src/.generated/type.rs")).unwrap();
    assert!(
        type_generated.contains("#[path = \"type/child.rs\"]\nmod child;"),
        "{type_generated}"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn inline_fixture_leaves_the_inline_module_header_untouched_but_rewrites_its_file_child() {
    let dir = copy_to_temp(&fixtures_dir().join("inline"), "inline");
    build(&BuildOptions::new(&dir)).expect("inline builds");

    let crate_root = fs::read_to_string(dir.join("src/.generated/crate-root.rs")).unwrap();
    assert!(crate_root.contains("mod shell {"), "{crate_root}");
    // `"panel.rs"`, not `"shell/panel.rs"` (issue #8 fix list step 3,
    // HIGH-1): rustc resolves this `#[path]` relative to a base directory
    // that already includes the inline module's own segment
    // (`.generated/shell/`); writing the segment twice made rustc look
    // for the doubled `.generated/shell/shell/panel.rs`.
    assert!(
        crate_root.contains("#[path = \"panel.rs\"]\nmod panel;"),
        "{crate_root}"
    );
    assert!(dir.join("src/.generated/shell/panel.rs").exists());

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn root_name_fixture_does_not_collide_with_a_child_named_main() {
    let dir = copy_to_temp(&fixtures_dir().join("root-name"), "root-name");
    let report = build(&BuildOptions::new(&dir)).expect("root-name builds");

    // `mod main { ... }` is inline: it has no file, and no generated unit
    // of its own — only `crate-root.rs` is produced.
    assert_eq!(report.generated_files.len(), 1);
    assert_eq!(
        report.generated_files[0],
        dir.join("src/.generated/crate-root.rs")
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn builds_the_example_app_with_the_expected_generated_files() {
    let dir = copy_to_temp(&repo_root().join("examples/phase0-app"), "phase0-app");
    let report = build(&BuildOptions::new(&dir)).expect("phase0-app builds");

    let got = rel_generated_files(&dir, &report.generated_files);
    let expected: HashSet<String> = ["crate-root.rs", "components.rs"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_eq!(got, expected);

    let crate_root = fs::read_to_string(dir.join("src/.generated/crate-root.rs")).unwrap();
    assert!(
        crate_root.contains("#[path = \"components.rs\"]\nmod components;"),
        "{crate_root}"
    );
    assert!(!crate_root.contains("dioxus_rsx"));
    assert!(!crate_root.contains("PropsBuilder"));

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn stale_generated_files_are_removed_and_unmanaged_files_are_left_alone() {
    let dir = copy_to_temp(&fixtures_dir().join("mixed"), "stale-cleanup");
    fs::create_dir_all(dir.join("src/.generated")).unwrap();
    fs::write(
        dir.join("src/.generated/stale.rs"),
        "// stale, from a removed module",
    )
    .unwrap();
    fs::write(
        dir.join("src/.generated/notes.txt"),
        "not managed by outou build",
    )
    .unwrap();

    let report = build(&BuildOptions::new(&dir)).expect("mixed builds");

    assert!(report
        .removed_files
        .contains(&dir.join("src/.generated/stale.rs")));
    assert!(!dir.join("src/.generated/stale.rs").exists());
    assert!(dir.join("src/.generated/notes.txt").exists());

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_second_build_overwrites_atomically_and_stays_deterministic() {
    let dir = copy_to_temp(&fixtures_dir().join("mixed"), "atomic-rebuild");
    let first = build(&BuildOptions::new(&dir)).expect("first build");
    let first_text = fs::read_to_string(dir.join("src/.generated/crate-root.rs")).unwrap();

    let second = build(&BuildOptions::new(&dir)).expect("second build");
    let second_text = fs::read_to_string(dir.join("src/.generated/crate-root.rs")).unwrap();

    assert_eq!(first_text, second_text);
    assert_eq!(first.generated_files.len(), second.generated_files.len());
    assert!(second.removed_files.is_empty());

    fs::remove_dir_all(&dir).ok();
}

/// HIGH-2 (issue #8, fix list step 2): two inline modules that each
/// declare a same-named child (`mod a { pub mod helper; }`, `mod b { pub
/// mod helper; }`) must produce two distinct `#[path]` values in
/// `crate-root.rs`, not one silently clobbering the other.
#[test]
fn inline_dup_fixture_gives_each_same_named_inline_sibling_its_own_path() {
    let dir = copy_to_temp(&fixtures_dir().join("inline-dup"), "inline-dup");
    build(&BuildOptions::new(&dir)).expect("inline-dup builds");

    // Both `#[path]` values legitimately read `"helper.rs"` — each is
    // resolved against its own inline module's own directory
    // (`.generated/a/`, `.generated/b/`), not against one shared
    // directory (step 3's DirScope-aware fix) — but the two physical
    // files they point to must be the two distinct generated files.
    let crate_root = fs::read_to_string(dir.join("src/.generated/crate-root.rs")).unwrap();
    let path_values: Vec<&str> = crate_root
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            line.strip_prefix("#[path = \"")
                .and_then(|rest| rest.strip_suffix("\"]"))
        })
        .collect();
    assert_eq!(path_values, vec!["helper.rs", "helper.rs"], "{crate_root}");
    assert!(dir.join("src/.generated/a/helper.rs").exists());
    assert!(dir.join("src/.generated/b/helper.rs").exists());
    assert_ne!(
        fs::read_to_string(dir.join("src/.generated/a/helper.rs")).unwrap(),
        fs::read_to_string(dir.join("src/.generated/b/helper.rs")).unwrap(),
    );

    fs::remove_dir_all(&dir).ok();
}

/// Same shape, but `b::helper` is plain Rust (`.rs`): the pre-fix bug (N2)
/// silently rebound `a::helper`'s own `#[path]` at `b`'s file — a
/// wrong-module binding, not merely a missing entry.
#[test]
fn inline_dup_rs_fixture_gives_the_rsx_and_rust_sibling_distinct_paths() {
    let dir = copy_to_temp(&fixtures_dir().join("inline-dup-rs"), "inline-dup-rs");
    build(&BuildOptions::new(&dir)).expect("inline-dup-rs builds");

    let crate_root = fs::read_to_string(dir.join("src/.generated/crate-root.rs")).unwrap();
    let path_values: Vec<&str> = crate_root
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            line.strip_prefix("#[path = \"")
                .and_then(|rest| rest.strip_suffix("\"]"))
        })
        .collect();
    assert_eq!(path_values.len(), 2, "{crate_root}");
    assert_ne!(path_values[0], path_values[1], "{crate_root}");

    fs::remove_dir_all(&dir).ok();
}

/// MEDIUM-5 (issue #8): `emit` used to write each unit's file as soon as
/// it was generated, in plan order. If a later unit in the same rebuild
/// then failed, every earlier unit had already been overwritten with its
/// *new* text — a build failure left `src/.generated/` holding a mix of
/// old and new generation, which `cargo build` would silently compile.
/// `emit` must instead generate every unit first and write nothing at all
/// until every unit has succeeded.
#[test]
fn a_failed_rebuild_leaves_every_previously_generated_file_untouched() {
    let dir = copy_to_temp(&fixtures_dir().join("mixed"), "partial-rebuild");
    build(&BuildOptions::new(&dir)).expect("first build succeeds");
    let root_before = fs::read_to_string(dir.join("src/.generated/crate-root.rs")).unwrap();
    let components_before = fs::read_to_string(dir.join("src/.generated/components.rs")).unwrap();

    // Edit the root (planned *before* the child that will fail, so a
    // naive sequential writer would already have overwritten it) and
    // break a child in the same rebuild.
    let main_path = dir.join("src/main.rsx");
    let main_source = fs::read_to_string(&main_path).unwrap();
    fs::write(&main_path, format!("// edited\n{main_source}")).unwrap();
    fs::write(dir.join("src/components/user.rsx"), "fn f() { <div cl").unwrap();

    build(&BuildOptions::new(&dir)).expect_err("a broken child fails the rebuild");

    let root_after = fs::read_to_string(dir.join("src/.generated/crate-root.rs")).unwrap();
    let components_after = fs::read_to_string(dir.join("src/.generated/components.rs")).unwrap();
    assert_eq!(
        root_before, root_after,
        "a failed rebuild must not touch any previously generated file"
    );
    assert_eq!(components_before, components_after);

    fs::remove_dir_all(&dir).ok();
}
