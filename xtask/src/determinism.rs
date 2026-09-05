//! `cargo xtask determinism` (issue #8, Gate 2's last checklist item).
//!
//! Generates every required fixture and `examples/phase0-app` through two
//! independent code paths and asserts the results are identical after
//! normalizing away each temp directory's own absolute prefix:
//!
//! 1. **The build path**: `outou_cli::build::build`, the exact pipeline
//!    `outou build` runs, into a fresh temp copy — twice, into two
//!    separate copies, to also prove the build path alone is
//!    deterministic (the same source always yields the same bytes).
//! 2. **The language-server-shaped path**: `outou-lsp` does not exist yet
//!    (issue #9). Until it does, this resolves the same plan
//!    (`outou_cli::build::plan::plan` — "there is one compiler", so the
//!    plan itself, not just the codegen call, is shared) and then calls
//!    `outou_syntax::parse` + `DioxusBackend::generate` directly, one
//!    unit at a time, bypassing `outou_cli::build`'s atomic-write/clean
//!    machinery entirely — the shape a language server takes (generate
//!    one file, in memory, on demand), not the shape a full crate build
//!    takes. **TODO(phase0, issue #9):** once the real language server
//!    exists, replace this hand-rolled second path with a call into it.
//!
//! Every generated `.rs` file is compared byte-for-byte; every
//! `.rs.map.json` is compared structurally (parsed to a value, not
//! byte-for-byte, so pretty-printing whitespace never causes a false
//! failure). The first mismatch for a target is reported as a small
//! unified-diff-shaped message; every target is still checked (not just
//! the first failing one), so one run reports everything that disagrees.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use outou_backend_dioxus::DioxusBackend;
use outou_cli::build::emit::file_uri;
use outou_cli::build::plan::{find_crate_root, plan, CrateRoot};
use outou_cli::build::{build, BuildOptions};
use outou_codegen::{Backend, GenerateOptions, Mode};

/// Runs the full determinism check. `Err` names every target that
/// disagreed; the caller is responsible for exiting non-zero.
pub fn run() -> Result<(), String> {
    let root = repo_root();
    let mut targets: Vec<(String, PathBuf)> = Vec::new();
    for name in [
        "mixed",
        "cfg",
        "cfg-duplicate",
        "raw-ident",
        "root-name",
        "inline",
        "inline-dup",
        "inline-dup-rs",
    ] {
        targets.push((
            name.to_string(),
            root.join("tests/fixtures/modules").join(name),
        ));
    }
    targets.push(("phase0-app".to_string(), root.join("examples/phase0-app")));
    // issue #10: the workspace fixture's library member. Only `ui-kit`
    // is added (not the fixture's `app` member, and not the fixture's
    // own workspace root, which `check_target` cannot build directly —
    // it calls `build()` on one crate directory at a time, matching
    // every other target in this list).
    targets.push((
        "workspace-fixture-ui-kit".to_string(),
        root.join("tests/fixtures/workspace/ui-kit"),
    ));

    let mut failures = Vec::new();
    for (name, dir) in &targets {
        if let Err(message) = check_target(name, dir) {
            failures.push(message);
        }
    }

    if failures.is_empty() {
        println!("determinism: {} target(s) OK", targets.len());
        Ok(())
    } else {
        let count = failures.len();
        let total = targets.len();
        for message in &failures {
            eprintln!("{message}\n");
        }
        Err(format!(
            "determinism check failed for {count} of {total} target(s)"
        ))
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask/ has a parent")
        .to_path_buf()
}

/// Checks one target: build path run 1, build path run 2, and the
/// independent second path, all into separate temp copies, then compares
/// them pairwise. Temp copies are removed before returning, on every path.
fn check_target(name: &str, source_dir: &Path) -> Result<(), String> {
    let a1 = copy_to_temp(source_dir, &format!("{name}-a1"));
    let a2 = copy_to_temp(source_dir, &format!("{name}-a2"));
    let b = copy_to_temp(source_dir, &format!("{name}-b"));

    let result = run_target(name, &a1, &a2, &b);

    for dir in [&a1, &a2, &b] {
        fs::remove_dir_all(dir).ok();
    }
    result
}

fn run_target(name: &str, a1: &Path, a2: &Path, b: &Path) -> Result<(), String> {
    let report_a1 = build(&BuildOptions::new(a1))
        .map_err(|e| format!("{name}: build path (run 1) failed: {e}"))?;
    if !report_a1.built {
        return Err(format!("{name}: expected an `.rsx` crate root, found none"));
    }
    build(&BuildOptions::new(a2)).map_err(|e| format!("{name}: build path (run 2) failed: {e}"))?;
    generate_via_independent_path(name, b)?;

    compare_trees(
        a1,
        a2,
        name,
        "build path, run 1 vs run 2 (self-determinism)",
    )?;
    compare_trees(a1, b, name, "build path vs independent (LSP-shaped) path")?;
    Ok(())
}

/// The independent second code path: resolves the same plan as the build
/// path, then calls the two primitives (`outou_syntax::parse`,
/// `DioxusBackend::generate`) directly per unit — no `outou_cli::build`
/// emit/clean machinery involved — and writes the results itself.
fn generate_via_independent_path(name: &str, manifest_dir: &Path) -> Result<(), String> {
    let canonical = manifest_dir
        .canonicalize()
        .map_err(|e| format!("{name}: canonicalizing {}: {e}", manifest_dir.display()))?;
    let root = match find_crate_root(&canonical)
        .map_err(|e| format!("{name}: finding crate root: {e}"))?
    {
        CrateRoot::NoRsxRoot => {
            return Err(format!("{name}: expected an `.rsx` crate root, found none"))
        }
        CrateRoot::Rsx(root) => root,
    };
    let planned = plan(&canonical, &root).map_err(|e| format!("{name}: planning: {e}"))?;

    fs::create_dir_all(&planned.generated_dir)
        .map_err(|e| format!("{name}: creating {}: {e}", planned.generated_dir.display()))?;

    for unit in &planned.units {
        // N1 (issue #8 fix list step 3): the same directory-existence
        // requirement `outou_cli::build::emit` handles for the build
        // path, needed here too since this independent path never goes
        // through `emit`.
        for dir in &unit.inline_base_dirs {
            fs::create_dir_all(dir)
                .map_err(|e| format!("{name}: creating {}: {e}", dir.display()))?;
        }
        let source = fs::read_to_string(&unit.source_file)
            .map_err(|e| format!("{name}: reading {}: {e}", unit.source_file.display()))?;
        let parsed = outou_syntax::parse(&source);
        let mut opts =
            GenerateOptions::new(file_uri(&unit.generated_file), file_uri(&unit.source_file));
        opts.module_paths = unit.module_paths.clone();

        let generated = DioxusBackend
            .generate(&parsed, &source, Mode::Strict, &opts)
            .map_err(|e| {
                format!(
                    "{name}: independent-path generation failed for {}: {e}",
                    unit.source_file.display()
                )
            })?;
        let map_json = generated
            .source_map
            .to_json(&generated.rust, &[&source])
            .map_err(|e| {
                format!(
                    "{name}: building source map json for {}: {e}",
                    unit.generated_file.display()
                )
            })?;
        let map_text = serde_json::to_string_pretty(&map_json)
            .expect("SourceMapJson always serializes")
            + "\n";

        if let Some(parent) = unit.generated_file.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("{name}: creating {}: {e}", parent.display()))?;
        }
        fs::write(&unit.generated_file, &generated.rust)
            .map_err(|e| format!("{name}: writing {}: {e}", unit.generated_file.display()))?;
        fs::write(&unit.map_file, &map_text)
            .map_err(|e| format!("{name}: writing {}: {e}", unit.map_file.display()))?;
    }

    Ok(())
}

/// Compares every managed file (`*.rs`, `*.rs.map.json`) under
/// `left/src/.generated` and `right/src/.generated`, after normalizing
/// away each tree's own absolute path prefix. `label` describes which two
/// code paths are being compared, for the failure message.
fn compare_trees(left: &Path, right: &Path, target: &str, label: &str) -> Result<(), String> {
    let left_generated = left.join("src/.generated");
    let right_generated = right.join("src/.generated");
    let left_files = list_managed_files(&left_generated);
    let right_files = list_managed_files(&right_generated);

    if left_files != right_files {
        let only_left: Vec<_> = left_files.difference(&right_files).collect();
        let only_right: Vec<_> = right_files.difference(&left_files).collect();
        return Err(format!(
            "{target} ({label}): generated file sets differ\n  only in first: {only_left:?}\n  only in second: {only_right:?}"
        ));
    }

    for relative in &left_files {
        let left_path = left_generated.join(relative);
        let right_path = right_generated.join(relative);
        let left_text = fs::read_to_string(&left_path)
            .map_err(|e| format!("{target}: reading {}: {e}", left_path.display()))?;
        let right_text = fs::read_to_string(&right_path)
            .map_err(|e| format!("{target}: reading {}: {e}", right_path.display()))?;
        let left_normalized = normalize(&left_text, left);
        let right_normalized = normalize(&right_text, right);

        let matches = if relative.to_string_lossy().ends_with(".map.json") {
            structurally_equal_json(&left_normalized, &right_normalized)
        } else {
            left_normalized == right_normalized
        };

        if !matches {
            return Err(format!(
                "{target} ({label}): {} differs\n{}",
                relative.display(),
                unified_diff(&left_normalized, &right_normalized)
            ));
        }
    }

    Ok(())
}

/// Replaces every occurrence of `root`'s own absolute path with `<root>`,
/// so two trees generated into different temp directories become
/// comparable (the header comment and every source-map URI embed the
/// generating/source file's absolute path).
///
/// This only normalizes away *location*-dependence between two temp
/// copies of the same source tree; it does not (and is not meant to)
/// paper over actual *non*-determinism, since both trees are normalized
/// the same way (LOW-9, issue #8: raised and rejected as misframed — a
/// global string replace here cannot hide a real difference in generated
/// bytes, only a difference in where the two copies happened to live on
/// disk).
///
/// TODO(phase0, ADR 0008): the generated header still embeds an absolute
/// `file://` source URI by design (`outou_sourcemap::file_uri`), which is
/// exactly what makes this normalization necessary in the first place. `outou
/// package` (ADR 0008, pre-generated publish artifacts) will need a
/// relative or repo-root-relative form instead, since a published crate's
/// generated file cannot embed the path of the machine that generated it.
fn normalize(text: &str, root: &Path) -> String {
    text.replace(&root.display().to_string(), "<root>")
}

/// Parses both texts as JSON and compares them structurally (field order
/// and whitespace do not matter, only values do).
fn structurally_equal_json(left: &str, right: &str) -> bool {
    let left_value: Result<serde_json::Value, _> = serde_json::from_str(left);
    let right_value: Result<serde_json::Value, _> = serde_json::from_str(right);
    match (left_value, right_value) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

/// A minimal unified-diff-shaped report of the first line that differs
/// between `left` and `right`: enough to point a reader at the problem
/// without pulling in a diff library for a repository-internal tool.
fn unified_diff(left: &str, right: &str) -> String {
    let left_lines: Vec<&str> = left.lines().collect();
    let right_lines: Vec<&str> = right.lines().collect();
    let max = left_lines.len().max(right_lines.len());
    for i in 0..max {
        let l = left_lines.get(i).copied().unwrap_or("<end of file>");
        let r = right_lines.get(i).copied().unwrap_or("<end of file>");
        if l != r {
            return format!("--- first\n+++ second\n@@ line {} @@\n-{l}\n+{r}\n", i + 1);
        }
    }
    "(no line-level difference found; texts differ only in trailing content)".to_string()
}

/// Every file under `generated_dir` (recursively) whose name ends in
/// `.rs` or `.map.json` — the files `outou build` itself manages — as
/// paths relative to `generated_dir`. An absent directory yields an empty
/// set.
fn list_managed_files(generated_dir: &Path) -> BTreeSet<PathBuf> {
    let mut out = BTreeSet::new();
    if !generated_dir.exists() {
        return out;
    }
    let mut stack = vec![generated_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                stack.push(path);
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.ends_with(".rs") || name.ends_with(".map.json") {
                if let Ok(relative) = path.strip_prefix(generated_dir) {
                    out.insert(relative.to_path_buf());
                }
            }
        }
    }
    out
}

fn copy_to_temp(src: &Path, label: &str) -> PathBuf {
    let dest = std::env::temp_dir().join(format!(
        "outou-xtask-determinism-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    copy_dir(src, &dest);
    dest.canonicalize()
        .unwrap_or_else(|e| panic!("canonicalizing {}: {e}", dest.display()))
}

fn copy_dir(src: &Path, dest: &Path) {
    fs::create_dir_all(dest).unwrap_or_else(|e| panic!("creating {}: {e}", dest.display()));
    for entry in fs::read_dir(src).unwrap_or_else(|e| panic!("reading {}: {e}", src.display())) {
        let entry = entry.expect("reading directory entry");
        let target = dest.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target)
                .unwrap_or_else(|e| panic!("copying {}: {e}", entry.path().display()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fast subset of the full check (one fixture), run as part of the
    /// normal test suite; the full `cargo xtask determinism` run (every
    /// fixture plus the example app) is exercised by CI directly.
    #[test]
    fn mixed_fixture_is_deterministic_across_both_paths() {
        let root = repo_root();
        let dir = root.join("tests/fixtures/modules/mixed");
        check_target("mixed", &dir).expect("the `mixed` fixture must be deterministic");
    }

    #[test]
    fn cfg_duplicate_fixture_is_deterministic_across_both_paths() {
        let root = repo_root();
        let dir = root.join("tests/fixtures/modules/cfg-duplicate");
        check_target("cfg-duplicate", &dir)
            .expect("the `cfg-duplicate` fixture must be deterministic");
    }
}
