//! Unit tests for `crate::build::plan` (split out per AGENTS.md file-size
//! guidance, opportunistically, as issue #8 fix list step 10 — steps 2
//! and 3 grew this module past a comfortable single-file size).

use super::*;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/modules")
}

#[test]
fn find_crate_root_locates_the_rsx_root() {
    let dir = fixtures_dir().join("mixed");
    let root = find_crate_root(&dir).expect("mixed has an rsx root");
    assert_eq!(root, CrateRoot::Rsx(dir.join("src/main.rsx")));
}

#[test]
fn find_crate_root_reports_no_root_when_only_plain_rust_exists() {
    let dir = std::env::temp_dir().join(format!(
        "outou-cli-plan-test-plain-only-{}-{}",
        std::process::id(),
        line!()
    ));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();

    let root = find_crate_root(&dir).expect("no error for a plain crate");

    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(root, CrateRoot::NoRsxRoot);
}

#[test]
fn find_crate_root_rejects_mixed_roots() {
    let dir = std::env::temp_dir().join(format!(
        "outou-cli-plan-test-mixed-roots-{}-{}",
        std::process::id(),
        line!()
    ));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
    std::fs::write(dir.join("src/main.rsx"), "fn main() {}").unwrap();

    let err = find_crate_root(&dir).expect_err("mixed roots must be rejected");

    std::fs::remove_dir_all(&dir).ok();
    assert!(matches!(err, PlanError::MixedRoots { .. }), "{err:?}");
}

/// The declaration `span.start` of the node at `module_path` in the
/// crate rooted at `dir`, for tests that need to look a
/// span-keyed [`PlannedUnit::module_paths`] entry up the same way
/// [`collect_module_paths`] computed its key.
fn span_start_of(dir: &Path, module_path: &[&str]) -> u32 {
    let root = match find_crate_root(dir).expect("root resolves") {
        CrateRoot::Rsx(root) => root,
        CrateRoot::NoRsxRoot => panic!("expected an rsx root"),
    };
    let graph = outou_modules::resolve(&root).expect("graph resolves");
    let wanted: Vec<String> = module_path.iter().map(|s| s.to_string()).collect();
    graph
        .iter()
        .find(|node| node.path == wanted)
        .unwrap_or_else(|| panic!("no node at {module_path:?}"))
        .span
        .expect("non-root node has a span")
        .start
}

#[test]
fn plan_mixed_fixture_computes_module_paths_for_rsx_and_rust_children() {
    let dir = fixtures_dir().join("mixed");
    let root = dir.join("src/main.rsx");
    let planned = plan(&dir, &root).expect("mixed resolves");

    assert_eq!(planned.units.len(), 3);

    let root_unit = &planned.units[0];
    assert!(root_unit.module_path.is_empty());
    assert_eq!(
        root_unit.generated_file,
        dir.join("src/.generated/crate-root.rs")
    );
    let components_key = span_start_of(&dir, &["components"]);
    assert_eq!(
        root_unit
            .module_paths
            .get(&components_key)
            .map(String::as_str),
        Some("components.rs")
    );

    let components_unit = planned
        .units
        .iter()
        .find(|u| u.module_path == vec!["components".to_string()])
        .expect("components unit");
    let user_key = span_start_of(&dir, &["components", "user"]);
    let button_key = span_start_of(&dir, &["components", "button"]);
    assert_eq!(
        components_unit
            .module_paths
            .get(&user_key)
            .map(String::as_str),
        Some("components/user.rs")
    );
    assert_eq!(
        components_unit
            .module_paths
            .get(&button_key)
            .map(String::as_str),
        Some("../components/button.rs")
    );
}

#[test]
fn plan_rejects_a_plain_rust_file_declaring_an_rsx_child() {
    let dir = fixtures_dir().join("rs-to-rsx");
    let root = dir.join("src/main.rsx");
    let err = plan(&dir, &root).expect_err("plain.rs declares plain/deep.rsx");
    match err {
        PlanError::RustDeclaresRsxChild { name, declared_in } => {
            assert_eq!(name, "deep");
            assert_eq!(declared_in, PathBuf::from("src/plain.rs"));
        }
        other => panic!("expected RustDeclaresRsxChild, got {other:?}"),
    }
}

#[test]
fn plan_rejects_an_rsx_child_declared_via_path_from_a_rust_file() {
    let dir = fixtures_dir().join("path-attr");
    let root = dir.join("src/main.rsx");
    let err = plan(&dir, &root).expect_err("somewhere/other.rs declares nested_from_rust.rsx");
    assert!(
        matches!(err, PlanError::RustDeclaresRsxChild { .. }),
        "{err:?}"
    );
}

#[test]
fn plan_cfg_duplicate_fixture_disambiguates_generated_names() {
    let dir = fixtures_dir().join("cfg-duplicate");
    let root = dir.join("src/main.rsx");
    let planned = plan(&dir, &root).expect("cfg-duplicate resolves");

    // Both `mod imp;` declarations share a name, but each is a
    // distinct declaration at its own source position, so keying
    // `module_paths` by declaration span (rather than by name)
    // disambiguates them into two distinct entries without needing
    // either declaration's own `#[path]` attribute text at all.
    let root_unit = &planned.units[0];
    assert_eq!(planned.units.len(), 3);
    assert_eq!(root_unit.module_paths.len(), 2);
    let values: Vec<&str> = root_unit
        .module_paths
        .values()
        .map(String::as_str)
        .collect();
    assert!(values.contains(&"imp.rs"), "{values:?}");
    assert!(values.contains(&"imp-1.rs"), "{values:?}");

    let generated_names: Vec<PathBuf> = planned
        .units
        .iter()
        .map(|u| u.generated_file.clone())
        .collect();
    assert!(generated_names.contains(&dir.join("src/.generated/imp.rs")));
    assert!(generated_names.contains(&dir.join("src/.generated/imp-1.rs")));
}

#[test]
fn plan_inline_fixture_has_no_module_path_entry_for_inline_children() {
    let dir = fixtures_dir().join("inline");
    let root = dir.join("src/main.rsx");
    let planned = plan(&dir, &root).expect("inline resolves");

    // `mod shell { mod panel; }`: `shell` is inline (no file of its
    // own), so the root's module_paths has no entry keyed by
    // `shell`'s own span, and `shell`'s own generated unit (its
    // parent's file, `crate-root.rs`) carries the `panel` entry
    // instead.
    //
    // The value is `"panel.rs"`, not `"shell/panel.rs"` (issue #8
    // fix list step 3, HIGH-1): rustc resolves an inline module's
    // child `#[path]` relative to a base directory that *already*
    // includes the inline module's own segment
    // (`src/.generated/shell/`), so writing `"shell/panel.rs"` there
    // makes rustc look for the doubled `shell/shell/panel.rs` —
    // confirmed against real rustc in the fix-list investigation.
    let root_unit = &planned.units[0];
    let shell_key = span_start_of(&dir, &["shell"]);
    assert!(!root_unit.module_paths.contains_key(&shell_key));
    let panel_key = span_start_of(&dir, &["shell", "panel"]);
    assert_eq!(
        root_unit.module_paths.get(&panel_key).map(String::as_str),
        Some("panel.rs")
    );
    // N1: the inline module's own base directory must physically
    // exist even though no generated *file* is ever written directly
    // into it (only `#[path]` values written inside `crate-root.rs`
    // point through it).
    assert!(root_unit
        .inline_base_dirs
        .contains(&dir.join("src/.generated/shell")));
}

/// HIGH-2 (issue #8): two inline siblings (`mod a { pub mod helper; }`,
/// `mod b { pub mod helper; }`) declare a same-named child. A
/// name-only `module_paths` key cannot tell them apart — the second
/// declaration silently clobbers the first's entry. Keying by
/// declaration span (decision 2) keeps both, regardless of what
/// directory each one's path string still resolves to (that part is
/// step 3's concern).
#[test]
fn plan_inline_dup_fixture_keeps_both_same_named_inline_siblings_distinct() {
    let dir = fixtures_dir().join("inline-dup");
    let root = dir.join("src/main.rsx");
    let planned = plan(&dir, &root).expect("inline-dup resolves");

    assert_eq!(planned.units.len(), 3, "root + a::helper + b::helper");
    let root_unit = &planned.units[0];
    assert_eq!(root_unit.module_paths.len(), 2);

    let a_helper_key = span_start_of(&dir, &["a", "helper"]);
    let b_helper_key = span_start_of(&dir, &["b", "helper"]);
    assert_ne!(a_helper_key, b_helper_key);

    // Both entries survive under their own key — HIGH-2's clobber is
    // gone. Their *string values* legitimately coincide (`"helper.rs"`
    // each): with the DirScope-aware base directory (step 3), each
    // `#[path]` is resolved against its own inline module's directory
    // (`.generated/a/`, `.generated/b/`), so identical relative
    // strings correctly point at two different physical files.
    assert_eq!(
        root_unit
            .module_paths
            .get(&a_helper_key)
            .map(String::as_str),
        Some("helper.rs"),
        "a::helper keeps its own module_paths entry"
    );
    assert_eq!(
        root_unit
            .module_paths
            .get(&b_helper_key)
            .map(String::as_str),
        Some("helper.rs"),
        "b::helper keeps its own module_paths entry"
    );
    assert!(root_unit
        .inline_base_dirs
        .contains(&dir.join("src/.generated/a")));
    assert!(root_unit
        .inline_base_dirs
        .contains(&dir.join("src/.generated/b")));
}

/// Same shape as above, but `b::helper` is plain Rust (`.rs`) rather
/// than `.rsx` (N2, issue #8): under the old name-only key this
/// silently rebound `a::helper`'s `#[path]` at `b`'s file, a
/// wrong-module binding, not merely a missing one.
#[test]
fn plan_inline_dup_rs_fixture_keeps_the_rsx_and_rust_siblings_distinct() {
    let dir = fixtures_dir().join("inline-dup-rs");
    let root = dir.join("src/main.rsx");
    let planned = plan(&dir, &root).expect("inline-dup-rs resolves");

    // Only `a::helper` (`.rsx`) is its own generation unit; `b::helper`
    // is plain Rust and never generates.
    assert_eq!(planned.units.len(), 2, "root + a::helper");
    let root_unit = &planned.units[0];
    assert_eq!(root_unit.module_paths.len(), 2);

    let a_helper_key = span_start_of(&dir, &["a", "helper"]);
    let b_helper_key = span_start_of(&dir, &["b", "helper"]);
    assert_ne!(a_helper_key, b_helper_key);

    let a_path = root_unit.module_paths.get(&a_helper_key).unwrap();
    let b_path = root_unit.module_paths.get(&b_helper_key).unwrap();
    assert_ne!(a_path, b_path);
    // `b::helper`'s target is plain Rust, referenced from `src/b/helper.rs`
    // rather than generated — it must never point into `.generated/`.
    assert!(!b_path.contains(".generated"));
}

/// Decision 2's safety net: two module declarations sharing one
/// `module_paths` key (impossible under a correctly resolved module
/// graph, since a declaration span is unique within its own file) is
/// reported as an internal error rather than one silently clobbering
/// the other.
#[test]
fn collect_module_paths_rejects_a_duplicate_declaration_span() {
    use outou_syntax::Span;

    fn leaf(span_start: u32, generated: &str, kind: SourceKind) -> ModuleNode {
        ModuleNode {
            path: vec![generated.to_string()],
            file: PathBuf::from(format!("src/{generated}.rs")),
            kind,
            declared_in: Some(PathBuf::from("src/main.rsx")),
            visibility: None,
            is_inline: false,
            cfg: Vec::new(),
            attributes: Vec::new(),
            span: Some(Span::new(span_start, span_start + 1)),
            generated: vec![generated.to_string()],
            children: Vec::new(),
        }
    }

    // Two distinct children that happen to share a declaration span:
    // unreachable from a real resolved graph, but a direct check of
    // the defensive branch itself.
    let root = ModuleNode {
        path: Vec::new(),
        file: PathBuf::from("src/main.rsx"),
        kind: SourceKind::Rsx,
        declared_in: None,
        visibility: None,
        is_inline: false,
        cfg: Vec::new(),
        attributes: Vec::new(),
        span: None,
        generated: Vec::new(),
        children: vec![
            leaf(10, "first", SourceKind::Rust),
            leaf(10, "second", SourceKind::Rust),
        ],
    };

    let crate_dir = PathBuf::from("/crate");
    let src_dir = crate_dir.join("src");
    let unit_generated_dir = src_dir.join(".generated");
    let unit_source = crate_dir.join("src/main.rsx");
    let mut out = BTreeMap::new();
    let mut inline_base_dirs = Vec::new();
    let err = collect_module_paths(
        &root,
        &unit_generated_dir,
        &crate_dir,
        &src_dir,
        &unit_source,
        &mut out,
        &mut inline_base_dirs,
    )
    .expect_err("duplicate span must be rejected");
    assert!(
        matches!(
            err,
            PlanError::DuplicateModuleDeclaration { span_start: 10, .. }
        ),
        "{err:?}"
    );
}
