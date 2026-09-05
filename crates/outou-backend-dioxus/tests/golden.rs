//! Golden tests (issue #6, deliverable 5a): `examples/phase0-app`'s two
//! `.rsx` files and every `tests/fixtures/formatting/*/input.rsx` fixture,
//! generated in Strict mode and compared byte-for-byte against a
//! hand-reviewed `expected.rs`/`expected.map.json` pair under
//! `tests/golden/<name>/`.
//!
//! Run with `BLESS=1 cargo test -p outou-backend-dioxus --test golden` to
//! (re)write the golden files from the current generator output; review
//! the diff before committing it.

mod support;

use std::fs;
use std::path::PathBuf;

use outou_backend_dioxus::DioxusBackend;
use outou_codegen::{Backend, GenerateOptions, Generated, Mode};
use outou_sourcemap::Uri;

struct Case {
    name: String,
    source_path: PathBuf,
    module_paths: Vec<(String, String)>,
}

fn cases() -> Vec<Case> {
    let root = support::repo_root();
    let mut cases = vec![
        Case {
            name: "main".to_string(),
            source_path: root.join("examples/phase0-app/src/main.rsx"),
            module_paths: vec![("components".to_string(), "components.rs".to_string())],
        },
        Case {
            name: "components".to_string(),
            source_path: root.join("examples/phase0-app/src/components.rsx"),
            module_paths: vec![],
        },
    ];

    let formatting_dir = root.join("tests/fixtures/formatting");
    let mut names: Vec<String> = fs::read_dir(&formatting_dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", formatting_dir.display()))
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for name in names {
        cases.push(Case {
            name: format!("formatting-{name}"),
            source_path: formatting_dir.join(&name).join("input.rsx"),
            module_paths: vec![],
        });
    }
    cases
}

fn golden_dir(case: &Case) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(&case.name)
}

fn generate(case: &Case) -> (String, Generated) {
    let source = fs::read_to_string(&case.source_path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", case.source_path.display()));
    let parsed = outou_syntax::parse(&source);
    let mut opts = GenerateOptions::new(
        Uri::new(format!("file:///gen/{}.rs", case.name)),
        Uri::new(format!("file:///src/{}", case.name)),
    );
    for (module, path) in &case.module_paths {
        opts = opts.with_module_path(module.clone(), path.clone());
    }
    let generated = DioxusBackend
        .generate(&parsed, &source, Mode::Strict, &opts)
        .unwrap_or_else(|e| panic!("generating case {}: {e}", case.name));
    (source, generated)
}

#[test]
fn golden_files_match() {
    let bless = std::env::var_os("BLESS").is_some();
    let mut checked = 0;
    for case in cases() {
        let (source, generated) = generate(&case);
        let dir = golden_dir(&case);
        let expected_rs_path = dir.join("expected.rs");
        let expected_map_path = dir.join("expected.map.json");

        let map_json = generated
            .source_map
            .to_json(&generated.rust, &[&source])
            .unwrap_or_else(|e| panic!("case {}: building source map json: {e}", case.name));
        let map_text = serde_json::to_string_pretty(&map_json).expect("json serializes") + "\n";

        if bless {
            fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("creating {}: {e}", dir.display()));
            fs::write(&expected_rs_path, &generated.rust)
                .unwrap_or_else(|e| panic!("writing {}: {e}", expected_rs_path.display()));
            fs::write(&expected_map_path, &map_text)
                .unwrap_or_else(|e| panic!("writing {}: {e}", expected_map_path.display()));
            continue;
        }

        let expected_rs = fs::read_to_string(&expected_rs_path).unwrap_or_else(|e| {
            panic!(
                "missing golden file {} ({e}); run `BLESS=1 cargo test -p outou-backend-dioxus \
                 --test golden` to create it, then review the diff",
                expected_rs_path.display()
            )
        });
        assert_eq!(
            generated.rust, expected_rs,
            "case {}: generated Rust no longer matches tests/golden/{}/expected.rs",
            case.name, case.name
        );

        let expected_map = fs::read_to_string(&expected_map_path)
            .unwrap_or_else(|e| panic!("missing golden file {}: {e}", expected_map_path.display()));
        assert_eq!(
            map_text, expected_map,
            "case {}: source map no longer matches tests/golden/{}/expected.map.json",
            case.name, case.name
        );
        checked += 1;
    }
    if !bless {
        assert!(checked > 0, "no golden cases were found");
    }
}

/// Deliverable 5(e): generated output never mentions the backend by name.
#[test]
fn no_case_mentions_dioxus() {
    for case in cases() {
        let (_source, generated) = generate(&case);
        assert!(
            !generated.rust.contains("dioxus"),
            "case {} contains the substring \"dioxus\":\n{}",
            case.name,
            generated.rust
        );
    }
}

/// Deliverable 5(d): generating twice yields identical bytes and maps.
#[test]
fn generation_is_deterministic_for_every_case() {
    for case in cases() {
        let (_source, first) = generate(&case);
        let (_source, second) = generate(&case);
        assert_eq!(first.rust, second.rust, "case {}", case.name);
        assert_eq!(first.source_map, second.source_map, "case {}", case.name);
    }
}
