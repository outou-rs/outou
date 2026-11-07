//! D4 (#31): an edit that moves a `mod` declaration without changing the
//! module set must keep the `#[path]` rewrite in the text regenerated for the
//! unit and sent to rust-analyzer. rust-analyzer is the logging mock of
//! `progress_tests`.

use std::fs;

use lsp_server::Connection;

use super::notifications::handle_rsx_change;
use super::plan_error_tests::{crate_dir, generated_text, key, load, with_mock_ra};
use super::progress_tests::ra_received;
use super::State;

/// The `#[path = ...]` and `mod ...;` lines of generated `text`, in order.
fn module_lines(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| line.contains("#[path") || is_mod_line(line))
        .map(|line| line.trim().to_string())
        .collect()
}

/// Whether `line` declares a module (`mod`, `pub mod`, `pub(crate) mod`).
fn is_mod_line(line: &str) -> bool {
    let line = line.trim_start();
    let line = match line.strip_prefix("pub") {
        Some(rest) => rest
            .strip_prefix('(')
            .and_then(|r| r.split_once(')'))
            .map_or(rest, |(_, after)| after),
        None => line,
    };
    line.trim_start().starts_with("mod ")
}

/// Opens `unit_suffix`, types `prefix` above the whole text (no `mod`
/// added or removed) and checks the regenerated text and what rust-analyzer
/// received keep the unit's `#[path]` lines exactly as planned.
fn assert_typing_above_keeps_module_paths(tag: &str, unit_suffix: &str, prefix: &str) {
    let dir = tag_dir(tag);
    let mut state = load(&dir);
    let log = with_mock_ra(&mut state, tag);
    let unit = key(&state, unit_suffix);
    let original = state.workspace.as_ref().unwrap().rsx[&unit]
        .line_index
        .text()
        .to_string();
    let planned = module_lines(&generated_text(&state, &unit));
    assert!(
        planned.iter().any(|l| l.contains("#[path")),
        "the plan must rewrite a `#[path]` for {unit_suffix}: {planned:?}"
    );

    let (connection, _client) = Connection::memory();
    handle_rsx_change(
        &mut state,
        &connection,
        unit.clone(),
        format!("{prefix}{original}"),
        2,
    );

    assert_eq!(
        module_lines(&generated_text(&state, &unit)),
        planned,
        "typing above a `mod` must not change its `#[path]` rewrite"
    );
    let sent = ra_received(&log, "didChange");
    let last = sent
        .iter()
        .rev()
        .find_map(|m| m["params"]["contentChanges"][0]["text"].as_str())
        .expect("rust-analyzer received the regenerated unit");
    assert_eq!(module_lines(last), planned, "text sent to rust-analyzer");
    fs::remove_dir_all(&dir).ok();
}

fn tag_dir(tag: &str) -> std::path::PathBuf {
    layout_dir(tag, tag)
}

/// A fresh crate directory named for `dir_tag` (every test needs its own: the
/// directory is removed when it is created) holding the `layout` sources.
fn layout_dir(dir_tag: &str, layout: &str) -> std::path::PathBuf {
    let dir = crate_dir(&format!("d4-{dir_tag}"));
    match layout {
        "mirrored" => {
            fs::write(dir.join("src/main.rsx"), "mod a;\nfn main() {}\n").unwrap();
            fs::write(dir.join("src/a.rsx"), "pub fn a() {}\n").unwrap();
        }
        "explicit-path" => {
            fs::create_dir_all(dir.join("src/sub")).unwrap();
            fs::write(
                dir.join("src/main.rsx"),
                "#[path = \"sub/util.rsx\"]\nmod a;\nfn main() {}\n",
            )
            .unwrap();
            fs::write(dir.join("src/sub/util.rsx"), "pub fn a() {}\n").unwrap();
        }
        "renamed" => {
            fs::write(
                dir.join("src/main.rsx"),
                "#[cfg(unix)]\n#[path = \"imp_unix.rsx\"]\nmod imp;\n\
                 #[cfg(not(unix))]\n#[path = \"imp_other.rsx\"]\nmod imp;\nfn main() {}\n",
            )
            .unwrap();
            fs::write(dir.join("src/imp_unix.rsx"), "pub fn f() {}\n").unwrap();
            fs::write(dir.join("src/imp_other.rsx"), "pub fn f() {}\n").unwrap();
        }
        "nested" => {
            fs::create_dir_all(dir.join("src/a")).unwrap();
            fs::write(dir.join("src/main.rsx"), "mod a;\nfn main() {}\n").unwrap();
            fs::write(dir.join("src/a.rsx"), "mod b;\npub fn a() {}\n").unwrap();
            fs::write(dir.join("src/a/b.rsx"), "pub fn b() {}\n").unwrap();
        }
        "inline" => {
            fs::write(
                dir.join("src/main.rsx"),
                "mod outer {\n    pub mod helper;\n}\n#[path = \"elsewhere\"]\nmod other {\n    pub mod helper;\n}\nfn main() {}\n",
            )
            .unwrap();
            fs::create_dir_all(dir.join("src/outer")).unwrap();
            fs::create_dir_all(dir.join("src/elsewhere")).unwrap();
            fs::write(dir.join("src/outer/helper.rsx"), "pub fn f() {}\n").unwrap();
            fs::write(dir.join("src/elsewhere/helper.rsx"), "pub fn g() {}\n").unwrap();
        }
        other => panic!("unknown layout {other}"),
    }
    dir
}

#[test]
fn mirrored_layout_keeps_its_path_when_typing_above_the_mod() {
    assert_typing_above_keeps_module_paths("mirrored", "main.rsx", "// typed\n");
}

#[test]
fn explicit_path_module_keeps_its_rewrite_when_typing_above_the_mod() {
    assert_typing_above_keeps_module_paths("explicit-path", "main.rsx", "// typed\n");
}

#[test]
fn renamed_generated_file_keeps_its_rewrite_when_typing_above_the_mods() {
    assert_typing_above_keeps_module_paths("renamed", "main.rsx", "// typed\n");
}

#[test]
fn nested_module_unit_keeps_its_path_when_typing_above_the_mod() {
    assert_typing_above_keeps_module_paths("nested", "/a.rsx", "// typed\n");
}

#[test]
fn same_named_modules_in_different_inline_blocks_keep_their_own_paths() {
    assert_typing_above_keeps_module_paths("inline", "main.rsx", "// typed above both\n");
}

/// Typing between the two declarations moves only the second one; each must
/// keep its own rewrite (a swap or a shared value would show here).
#[test]
fn typing_between_two_modules_keeps_each_modules_own_path() {
    let dir = layout_dir("renamed-between", "renamed");
    let mut state = load(&dir);
    let unit = key(&state, "main.rsx");
    let planned = module_lines(&generated_text(&state, &unit));
    let original = state.workspace.as_ref().unwrap().rsx[&unit]
        .line_index
        .text()
        .to_string();
    let edited = original.replace("#[cfg(not(unix))]", "// between\n#[cfg(not(unix))]");
    assert_ne!(edited, original);

    let (connection, _client) = Connection::memory();
    handle_rsx_change(&mut state, &connection, unit.clone(), edited, 2);

    assert_eq!(module_lines(&generated_text(&state, &unit)), planned);
    fs::remove_dir_all(&dir).ok();
}

/// L2: every key of a real plan's `module_paths` is a declaration the unit
/// recorded in `plan_module_spans`, or the re-key would silently drop it.
#[test]
fn every_planned_module_path_key_is_a_recorded_declaration_span() {
    for layout in ["mirrored", "explicit-path", "renamed", "nested", "inline"] {
        let dir = layout_dir(&format!("keys-{layout}"), layout);
        let state = load(&dir);
        let workspace = state.workspace.as_ref().unwrap();
        let mut total = 0;
        for unit in workspace.generated.values() {
            for key in unit.planned.module_paths.keys() {
                total += 1;
                assert!(
                    unit.plan_module_spans.contains(key),
                    "{layout}: key {key} not in {:?}",
                    unit.plan_module_spans
                );
            }
        }
        assert!(total > 0, "{layout} must plan at least one rewrite");
        fs::remove_dir_all(&dir).ok();
    }
}

fn write_files(dir: &std::path::Path, files: &[(&str, &str)]) {
    for (rel, text) in files {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
}

const NESTED_BEFORE: &str = "mod a {\n    pub mod b;\n}\nfn main() {}\n";
const NESTED_AFTER: &str = "mod a {\n}\npub mod b;\nfn main() {}\n";

/// M1: moving `pub mod b;` out of `mod a {}` keeps names, paths and order,
/// so only the nesting tells the shape changed. With no `src/b.rsx` the
/// re-plan fails and the editor is told.
#[test]
fn moving_a_mod_out_of_an_inline_module_replans_and_shows_the_plan_error() {
    let dir = crate_dir("d4-m1-error");
    write_files(
        &dir,
        &[
            ("src/main.rsx", NESTED_BEFORE),
            ("src/a/b.rsx", "pub fn b() {}\n"),
        ],
    );
    let mut state = load(&dir);
    let main = key(&state, "main.rsx");
    assert!(state
        .workspace
        .as_ref()
        .unwrap()
        .module_shape_changed(&main, NESTED_AFTER));

    let (connection, _client) = Connection::memory();
    handle_rsx_change(&mut state, &connection, main, NESTED_AFTER.to_string(), 2);

    assert!(state.workspace.as_ref().unwrap().plan_error.is_some());
    fs::remove_dir_all(&dir).ok();
}

/// M1: with `src/b.rsx` present the move re-plans and the unit set follows.
#[test]
fn moving_a_mod_out_of_an_inline_module_reloads_the_unit_set() {
    let dir = crate_dir("d4-m1-units");
    write_files(
        &dir,
        &[
            ("src/main.rsx", NESTED_BEFORE),
            ("src/a/b.rsx", "pub fn inner() {}\n"),
            ("src/b.rsx", "pub fn outer() {}\n"),
        ],
    );
    let mut state = load(&dir);
    let main = key(&state, "main.rsx");
    let units_of = |state: &State| {
        let workspace = state.workspace.as_ref().unwrap();
        let mut keys: Vec<String> = workspace
            .generated
            .keys()
            .map(|uri| uri.rsplit("/.generated/").next().unwrap().to_string())
            .collect();
        keys.sort();
        keys
    };
    assert!(units_of(&state).contains(&"a/b.rs".to_string()));

    let (connection, _client) = Connection::memory();
    handle_rsx_change(&mut state, &connection, main, NESTED_AFTER.to_string(), 2);

    assert!(state.workspace.as_ref().unwrap().plan_error.is_none());
    let units = units_of(&state);
    assert!(units.contains(&"b.rs".to_string()), "{units:?}");
    assert!(!units.contains(&"a/b.rs".to_string()), "{units:?}");
    fs::remove_dir_all(&dir).ok();
}

/// M1: moving an inline module that holds a file module out of its parent.
#[test]
fn moving_an_inline_module_out_of_its_parent_changes_the_module_shape() {
    let dir = crate_dir("d4-m1-inline");
    let before = "mod a {\n    mod c {\n        pub mod d;\n    }\n}\nfn main() {}\n";
    let after = "mod a {\n}\nmod c {\n    pub mod d;\n}\nfn main() {}\n";
    write_files(
        &dir,
        &[
            ("src/main.rsx", before),
            ("src/a/c/d.rsx", "pub fn d() {}\n"),
        ],
    );
    let state = load(&dir);
    let main = key(&state, "main.rsx");
    assert!(state
        .workspace
        .as_ref()
        .unwrap()
        .module_shape_changed(&main, after));
    fs::remove_dir_all(&dir).ok();
}
