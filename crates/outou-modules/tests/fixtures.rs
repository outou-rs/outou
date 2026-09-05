//! Fixture tests for the module resolver (issue #7, required by Gate 2).
//!
//! Each fixture lives under `tests/fixtures/modules/<name>/src/` at the
//! repository root and is resolved starting at its crate root file.
//! `tests/fixtures/modules/README.md` documents what each one covers.

use std::path::{Path, PathBuf};

use outou_modules::{resolve, ModuleError, ModuleNode, SourceKind};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/outou-modules has a parent")
        .parent()
        .expect("crates/ has a parent")
        .to_path_buf()
}

fn fixture(name: &str) -> PathBuf {
    repo_root()
        .join("tests")
        .join("fixtures")
        .join("modules")
        .join(name)
}

fn node<'a>(children: &'a [ModuleNode], name: &str) -> &'a ModuleNode {
    children
        .iter()
        .find(|n| n.path.last().map(String::as_str) == Some(name))
        .unwrap_or_else(|| panic!("no child module named `{name}`"))
}

fn nodes<'a>(children: &'a [ModuleNode], name: &str) -> Vec<&'a ModuleNode> {
    children
        .iter()
        .filter(|n| n.path.last().map(String::as_str) == Some(name))
        .collect()
}

// ---------------------------------------------------------------------
// mixed/
// ---------------------------------------------------------------------

#[test]
fn mixed_resolves_the_expected_tree() {
    let root = fixture("mixed").join("src/main.rsx");
    let graph = resolve(&root).expect("mixed/ resolves");

    assert_eq!(graph.root.path, Vec::<String>::new());
    assert_eq!(graph.root.file, PathBuf::from("src/main.rsx"));
    assert_eq!(graph.root.kind, SourceKind::Rsx);
    assert_eq!(graph.root.declared_in, None);

    let components = node(&graph.root.children, "components");
    assert_eq!(components.path, vec!["components".to_string()]);
    assert_eq!(components.file, PathBuf::from("src/components.rsx"));
    assert_eq!(components.kind, SourceKind::Rsx);
    assert_eq!(components.declared_in, Some(PathBuf::from("src/main.rsx")));

    let button = node(&components.children, "button");
    assert_eq!(
        button.path,
        vec!["components".to_string(), "button".to_string()]
    );
    assert_eq!(button.file, PathBuf::from("src/components/button.rs"));
    assert_eq!(button.kind, SourceKind::Rust);
    assert!(button.children.is_empty());

    let user = node(&components.children, "user");
    assert_eq!(
        user.path,
        vec!["components".to_string(), "user".to_string()]
    );
    assert_eq!(user.file, PathBuf::from("src/components/user.rsx"));
    assert_eq!(user.kind, SourceKind::Rsx);
    assert!(user.children.is_empty());

    // Full depth-first node list, in order: root, components, button, user.
    let paths: Vec<Vec<String>> = graph.iter().map(|n| n.path.clone()).collect();
    assert_eq!(
        paths,
        vec![
            Vec::<String>::new(),
            vec!["components".to_string()],
            vec!["components".to_string(), "button".to_string()],
            vec!["components".to_string(), "user".to_string()],
        ]
    );

    let rsx_files: Vec<PathBuf> = graph.rsx_files().map(|n| n.file.clone()).collect();
    assert_eq!(
        rsx_files,
        vec![
            PathBuf::from("src/main.rsx"),
            PathBuf::from("src/components.rsx"),
            PathBuf::from("src/components/user.rsx"),
        ]
    );

    // Generated-path convention (ADR 0009 layout (b), issue #7 decision 3):
    // the root always uses the reserved `crate-root` stem.
    let src_dir = Path::new("src");
    assert_eq!(
        graph.root.generated_path(src_dir),
        PathBuf::from("src/.generated/crate-root.rs")
    );
    assert_eq!(
        components.generated_path(src_dir),
        PathBuf::from("src/.generated/components.rs")
    );
    assert_eq!(
        user.generated_path(src_dir),
        PathBuf::from("src/.generated/components/user.rs")
    );
}

// ---------------------------------------------------------------------
// ambiguous/
// ---------------------------------------------------------------------

#[test]
fn ambiguous_fails_with_the_exact_expected_text() {
    let root = fixture("ambiguous").join("src/main.rsx");
    let expected = std::fs::read_to_string(fixture("ambiguous").join("expected.txt"))
        .expect("reading ambiguous/expected.txt");

    let err = resolve(&root).expect_err("ambiguous/ must fail to resolve");
    let rendered = format!("error: {err}");
    assert_eq!(rendered.trim_end(), expected.trim_end());

    match err {
        ModuleError::Ambiguous {
            name,
            declared_in,
            candidates,
            ..
        } => {
            assert_eq!(name, "widgets");
            assert_eq!(declared_in, PathBuf::from("src/main.rsx"));
            assert_eq!(
                candidates,
                vec![
                    PathBuf::from("src/widgets.rsx"),
                    PathBuf::from("src/widgets.rs"),
                ]
            );
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// path-attr/
// ---------------------------------------------------------------------

#[test]
fn path_attr_is_honored_for_rsx_and_rust_targets() {
    let root = fixture("path-attr").join("src/main.rsx");
    let graph = resolve(&root).expect("path-attr/ resolves");

    let thing = node(&graph.root.children, "thing");
    assert_eq!(thing.file, PathBuf::from("src/elsewhere/thing.rsx"));
    assert_eq!(thing.kind, SourceKind::Rsx);

    let other = node(&graph.root.children, "other");
    assert_eq!(other.file, PathBuf::from("src/somewhere/other.rs"));
    assert_eq!(other.kind, SourceKind::Rust);

    // A plain `.rs` file's own `mod` (no `#[path]`) still resolves
    // through the ordinary candidate search, including to a `.rsx`
    // target: the `.rs -> .rsx` transition.
    let nested = node(&other.children, "nested_from_rust");
    assert_eq!(
        nested.file,
        PathBuf::from("src/somewhere/nested_from_rust.rsx")
    );
    assert_eq!(nested.kind, SourceKind::Rsx);
}

#[test]
fn path_attr_children_are_siblings_of_the_path_target() {
    // A file reached via `#[path]` is mod-rs-like (issue #7 decision 2):
    // its own un-annotated children look up in *its own* directory, not
    // in a directory named after the declaring module.
    let root = fixture("path-attr").join("src/main.rsx");
    let graph = resolve(&root).expect("path-attr/ resolves");
    let other = node(&graph.root.children, "other");
    let nested = node(&other.children, "nested_from_rust");
    assert_eq!(
        nested.file,
        PathBuf::from("src/somewhere/nested_from_rust.rsx")
    );
}

// ---------------------------------------------------------------------
// cfg/
// ---------------------------------------------------------------------

#[test]
fn cfg_attribute_is_recorded_verbatim_and_not_evaluated() {
    let root = fixture("cfg").join("src/main.rsx");
    let graph = resolve(&root).expect("cfg/ resolves even though feature `x` is not enabled");

    let optional = node(&graph.root.children, "optional");
    assert_eq!(optional.file, PathBuf::from("src/optional.rsx"));
    assert_eq!(optional.cfg, vec!["#[cfg(feature = \"x\")]".to_string()]);
    assert_eq!(
        optional.attributes,
        vec!["#[cfg(feature = \"x\")]".to_string()]
    );
    assert!(optional.span.is_some());
}

#[test]
fn spaced_cfg_attribute_is_recorded() {
    // issue #7 finding 9: `#[ cfg(feature = "y") ]` (leading/trailing
    // trivia inside the brackets) must still be recognized as a `cfg`
    // attribute, not silently dropped from `ModuleNode::cfg`.
    let root = fixture("cfg").join("src/main.rsx");
    let graph = resolve(&root).expect("cfg/ resolves");

    let spaced = node(&graph.root.children, "spaced");
    assert_eq!(spaced.file, PathBuf::from("src/spaced.rsx"));
    assert_eq!(
        spaced.cfg,
        vec!["#[ cfg(feature = \"y\") ]".to_string()],
        "{:?}",
        spaced.attributes
    );
}

// ---------------------------------------------------------------------
// not-found/
// ---------------------------------------------------------------------

#[test]
fn not_found_when_no_candidate_exists() {
    let root = fixture("not-found").join("src/main.rs");
    let err = resolve(&root).expect_err("not-found/ has no file for `missing`");

    match err {
        ModuleError::NotFound {
            name,
            declared_in,
            candidates,
            ..
        } => {
            assert_eq!(name, "missing");
            assert_eq!(declared_in, PathBuf::from("src/main.rs"));
            assert_eq!(
                candidates,
                vec![
                    PathBuf::from("src/missing.rsx"),
                    PathBuf::from("src/missing.rs"),
                    PathBuf::from("src/missing/mod.rsx"),
                    PathBuf::from("src/missing/mod.rs"),
                ]
            );
        }
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[test]
fn errors_name_the_declaring_file() {
    // issue #7 fix list item 1: every error carries `declared_in` and a
    // `span` that actually slices the `mod` declaration out of that file
    // — not the parent's span misapplied to a child file, and not a span
    // that starts mid-whitespace.
    let root = fixture("not-found").join("src/main.rs");
    let source = std::fs::read_to_string(&root).expect("reading not-found/src/main.rs");
    let err = resolve(&root).expect_err("not-found/ has no file for `missing`");

    let (declared_in, span) = match &err {
        ModuleError::NotFound {
            declared_in, span, ..
        } => (declared_in.clone(), *span),
        other => panic!("expected NotFound, got {other:?}"),
    };
    assert_eq!(declared_in, PathBuf::from("src/main.rs"));
    let sliced = &source[span.start as usize..span.end as usize];
    assert_eq!(sliced, "mod missing;");
}

// ---------------------------------------------------------------------
// inline/
// ---------------------------------------------------------------------

#[test]
fn inline_module_resolves_a_nested_file_module() {
    let root = fixture("inline").join("src/main.rsx");
    let graph = resolve(&root).expect("inline/ resolves");

    let shell = node(&graph.root.children, "shell");
    // Inline modules have no file of their own: they are recorded against
    // the file the inline block is written in.
    assert_eq!(shell.file, PathBuf::from("src/main.rsx"));
    assert_eq!(shell.kind, SourceKind::Rsx);
    assert!(shell.is_inline);
    assert_eq!(shell.declared_in, Some(PathBuf::from("src/main.rsx")));

    let panel = node(&shell.children, "panel");
    assert_eq!(panel.path, vec!["shell".to_string(), "panel".to_string()]);
    assert_eq!(panel.file, PathBuf::from("src/shell/panel.rsx"));
    assert_eq!(panel.kind, SourceKind::Rsx);
    assert!(!panel.is_inline);
}

// ---------------------------------------------------------------------
// path-dirs/ (rustc-validated directory ownership, issue #7 decision 2)
// ---------------------------------------------------------------------

#[test]
fn path_dirs_matches_rustc_directory_ownership() {
    let root = fixture("path-dirs").join("src/main.rsx");
    let graph = resolve(&root).expect("path-dirs/ resolves");

    let all: Vec<&ModuleNode> = graph.iter().collect();
    let files: Vec<PathBuf> = all.iter().map(|n| n.file.clone()).collect();

    let expected = [
        "src/inline_host/inner.rsx",
        "src/thread_files/tls.rsx",
        "src/thread_files/extra.rsx",
        "src/somewhere/nested.rsx",
        "src/deep/shell/x.rsx",
        "src/plain/child/grand.rsx",
        "src/aside.rsx",
        "src/tf/leaf.rsx",
        "src/plain/boxed/in_box.rsx",
        "src/mod_style/sibling.rsx",
        "src/mod_style/inl/y.rsx",
    ];
    for path in expected {
        assert!(
            files.contains(&PathBuf::from(path)),
            "expected {path} among resolved files: {files:#?}"
        );
    }

    let decoys = [
        "src/inner.rsx",
        "src/tls.rsx",
        "src/thread/extra.rsx",
        "src/other/nested.rsx",
        "src/deep/loaded/shell/x.rsx",
        "src/plain/aside.rsx",
        "src/plain/tf/leaf.rsx",
        "src/in_box.rsx",
        "src/mod_style/y.rsx",
    ];
    for decoy in decoys {
        assert!(
            !files.contains(&PathBuf::from(decoy)),
            "decoy {decoy} must never be loaded: {files:#?}"
        );
    }
}

// ---------------------------------------------------------------------
// rs-to-rsx/
// ---------------------------------------------------------------------

#[test]
fn rs_to_rsx_transition_through_ordinary_candidates() {
    let root = fixture("rs-to-rsx").join("src/main.rsx");
    let graph = resolve(&root).expect("rs-to-rsx/ resolves");

    let plain = node(&graph.root.children, "plain");
    assert_eq!(plain.kind, SourceKind::Rust);
    let deep = node(&plain.children, "deep");
    assert_eq!(deep.file, PathBuf::from("src/plain/deep.rsx"));
    assert_eq!(deep.kind, SourceKind::Rsx);
}

// ---------------------------------------------------------------------
// cycle-self/, cycle-chain/
// ---------------------------------------------------------------------

#[test]
fn cycle_self_reference_reports_the_chain() {
    let dir = fixture("cycle-self");
    let root = dir.join("src/main.rsx");
    let expected =
        std::fs::read_to_string(dir.join("expected.txt")).expect("reading cycle-self/expected.txt");

    let err = resolve(&root).expect_err("cycle-self/ must not resolve");
    let rendered = format!("error: {err}");
    assert_eq!(rendered.trim_end(), expected.trim_end());

    match err {
        ModuleError::Circular { chain, .. } => {
            assert_eq!(
                chain,
                vec![PathBuf::from("src/main.rsx"), PathBuf::from("src/main.rsx")]
            );
        }
        other => panic!("expected Circular, got {other:?}"),
    }
}

#[test]
fn cycle_between_two_files_reports_the_chain() {
    let dir = fixture("cycle-chain");
    let root = dir.join("src/main.rsx");
    let expected = std::fs::read_to_string(dir.join("expected.txt"))
        .expect("reading cycle-chain/expected.txt");

    let err = resolve(&root).expect_err("cycle-chain/ must not resolve");
    let rendered = format!("error: {err}");
    assert_eq!(rendered.trim_end(), expected.trim_end());

    match err {
        ModuleError::Circular { chain, .. } => {
            assert_eq!(
                chain,
                vec![
                    PathBuf::from("src/main.rsx"),
                    PathBuf::from("src/b.rsx"),
                    PathBuf::from("src/main.rsx"),
                ]
            );
        }
        other => panic!("expected Circular, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// raw-ident/
// ---------------------------------------------------------------------

#[test]
fn raw_identifier_module_uses_the_unraw_file_name() {
    let root = fixture("raw-ident").join("src/main.rsx");
    let graph = resolve(&root).expect("raw-ident/ resolves");

    let type_module = node(&graph.root.children, "r#type");
    assert_eq!(type_module.path, vec!["r#type".to_string()]);
    assert_eq!(type_module.file, PathBuf::from("src/type.rsx"));
    assert_eq!(
        type_module.generated_path(Path::new("src")),
        PathBuf::from("src/.generated/type.rs")
    );

    let child = node(&type_module.children, "child");
    assert_eq!(child.file, PathBuf::from("src/type/child.rsx"));
}

// ---------------------------------------------------------------------
// cfg-attr-path/
// ---------------------------------------------------------------------

#[test]
fn cfg_attr_path_is_rejected_with_an_explicit_diagnostic() {
    let dir = fixture("cfg-attr-path");
    let root = dir.join("src/main.rsx");
    let expected = std::fs::read_to_string(dir.join("expected.txt"))
        .expect("reading cfg-attr-path/expected.txt");

    let err = resolve(&root).expect_err("cfg-attr-path/ must not resolve");
    let rendered = format!("error: {err}");
    assert_eq!(rendered.trim_end(), expected.trim_end());

    match err {
        ModuleError::ConditionalPath {
            name, declared_in, ..
        } => {
            assert_eq!(name, "platform");
            assert_eq!(declared_in, PathBuf::from("src/main.rsx"));
        }
        other => panic!("expected ConditionalPath, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// cfg-duplicate/
// ---------------------------------------------------------------------

#[test]
fn cfg_exclusive_duplicate_modules_get_distinct_generated_paths() {
    let root = fixture("cfg-duplicate").join("src/main.rsx");
    let graph = resolve(&root).expect("cfg-duplicate/ resolves: rustc accepts this idiom");

    let imps = nodes(&graph.root.children, "imp");
    assert_eq!(imps.len(), 2, "{imps:#?}");

    let files: Vec<PathBuf> = imps.iter().map(|n| n.file.clone()).collect();
    assert_eq!(
        files,
        vec![
            PathBuf::from("src/unix.rsx"),
            PathBuf::from("src/windows.rsx"),
        ]
    );

    let src_dir = Path::new("src");
    let generated_paths: Vec<PathBuf> = imps.iter().map(|n| n.generated_path(src_dir)).collect();
    assert_eq!(
        generated_paths,
        vec![
            PathBuf::from("src/.generated/imp.rs"),
            PathBuf::from("src/.generated/imp-1.rs"),
        ]
    );

    let all_generated: Vec<PathBuf> = graph
        .generated_units()
        .map(|n| n.generated_path(src_dir))
        .collect();
    let mut unique = all_generated.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        unique.len(),
        all_generated.len(),
        "generated_units() must not collide: {all_generated:#?}"
    );
}

// ---------------------------------------------------------------------
// root-name/
// ---------------------------------------------------------------------

#[test]
fn root_and_child_named_main_do_not_collide() {
    let root = fixture("root-name").join("src/main.rsx");
    let graph = resolve(&root).expect("root-name/ resolves");

    let src_dir = Path::new("src");
    assert_eq!(
        graph.root.generated_path(src_dir),
        PathBuf::from("src/.generated/crate-root.rs")
    );

    let child = node(&graph.root.children, "main");
    assert_eq!(
        child.generated_path(src_dir),
        PathBuf::from("src/.generated/main.rs")
    );
}

// ---------------------------------------------------------------------
// examples/phase0-app (generated_units must skip inline modules)
// ---------------------------------------------------------------------

#[test]
fn generated_units_skips_inline_modules() {
    let root = repo_root()
        .join("examples")
        .join("phase0-app")
        .join("src")
        .join("main.rsx");
    let graph = resolve(&root).expect("examples/phase0-app resolves");

    let units: Vec<PathBuf> = graph.generated_units().map(|n| n.file.clone()).collect();
    assert_eq!(
        units,
        vec![
            PathBuf::from("src/main.rsx"),
            PathBuf::from("src/components.rsx"),
        ]
    );
}
