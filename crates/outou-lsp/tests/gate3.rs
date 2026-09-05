//! Gate 3 (issue #9, `docs/phase0/issues/09-integrated-lsp.md`): "through
//! the real parser, completion / hover / definition / diagnostics do not
//! work for `let user = load_user(); <UserCard user={user} />` -> STOP."
//!
//! Drives the real `outou-lsp` binary against Gate 3's fixed target
//! program, `examples/phase0-app`, through
//! `spikes/rust-analyzer/client/outou-lsp-client.mjs` (a sibling of the
//! Week 1 spike's `ra-client.mjs`, reusing its framing/readiness design
//! but adapted to `outou-lsp`'s own protocol shape — see that file's own
//! doc comment for why it is a separate script rather than a `ra-client.mjs`
//! flag).
//!
//! Ignored by default: the full run spawns `outou-lsp` (which itself
//! spawns rust-analyzer) eight times, each waiting out an indexing
//! settle window, well over the ~60s the repository's other tests expect
//! to finish in. Run explicitly:
//!
//! ```text
//! cargo test -p outou-lsp --test gate3 -- --ignored --nocapture
//! ```
//!
//! Skips (printing why, rather than failing) if `node` or a
//! `rust-analyzer` binary is not available, since neither is a Rust
//! toolchain component this repository can assume in every environment
//! that runs `cargo test --workspace`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// One probe per Gate 3 criterion, matching
/// `outou-lsp-client.mjs`'s `--probe` names.
const PROBES: &[&str] = &[
    "hover-user",
    "definition-load-user",
    "definition-user-card",
    "completion-member",
    "completion-component",
    "completion-prop",
    "diagnostic-type-error",
    "diagnostic-syntax-error",
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root exists")
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Resolves a rust-analyzer binary the same way `outou-lsp` itself does:
/// `OUTOU_RUST_ANALYZER` if set, otherwise `PATH`.
fn rust_analyzer_binary() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("OUTOU_RUST_ANALYZER") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join("rust-analyzer"))
        .find(|candidate| candidate.is_file())
}

/// Runs one probe through `outou-lsp-client.mjs` and returns its parsed
/// JSON output.
fn run_probe(root: &Path, outou_lsp: &Path, ra: &Path, probe: &str) -> serde_json::Value {
    let client = root.join("spikes/rust-analyzer/client/outou-lsp-client.mjs");
    let output = Command::new("node")
        .arg(&client)
        .arg("--root")
        .arg(root.join("examples/phase0-app"))
        .arg("--outou-lsp")
        .arg(outou_lsp)
        .arg("--ra")
        .arg(ra)
        .arg("--probe")
        .arg(probe)
        .arg("--settle")
        .arg("12000")
        .arg("--timeout")
        .arg("45000")
        .output()
        .unwrap_or_else(|e| panic!("running outou-lsp-client.mjs for probe {probe}: {e}"));

    if !output.status.success() {
        panic!(
            "outou-lsp-client.mjs failed for probe {probe} (status {}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("parsing outou-lsp-client.mjs output for probe {probe}: {e}"))
}

/// Writes `json` gzip-compressed to
/// `spikes/rust-analyzer/results/gate3-<probe>.json.gz`, matching the
/// Gate 0 results' own convention (`spikes/rust-analyzer/results/README.md`):
/// shells out to the system `gzip`, since this repository has no gzip
/// crate dependency and the Gate 0 raw results were produced the same way.
fn save_gzipped(root: &Path, probe: &str, json: &serde_json::Value) {
    let results_dir = root.join("spikes/rust-analyzer/results");
    let pretty = serde_json::to_vec_pretty(json).expect("gate3 results always serialize");

    let tmp_path =
        std::env::temp_dir().join(format!("outou-gate3-{probe}-{}.json", std::process::id()));
    std::fs::write(&tmp_path, &pretty).expect("writing temp file before gzip");

    let status = Command::new("gzip")
        .arg("-9")
        .arg("-f")
        .arg(&tmp_path)
        .status()
        .expect("running gzip");
    assert!(status.success(), "gzip failed for probe {probe}");

    let gz_tmp = tmp_path.with_extension("json.gz");
    let dest = results_dir.join(format!("gate3-{probe}.json.gz"));
    std::fs::create_dir_all(&results_dir).expect("results dir exists");
    std::fs::rename(&gz_tmp, &dest)
        .unwrap_or_else(|e| panic!("moving {} to {}: {e}", gz_tmp.display(), dest.display()));
}

/// Snapshots `examples/phase0-app/src/.generated/{crate-root.rs,components.rs}`
/// on construction and restores (or removes, if a file did not exist
/// before) them on drop — including on an early return or a panicking
/// assertion, since this runs after every probe in the loop, not just at
/// the end of a successful one.
struct RestoreGeneratedFiles {
    files: Vec<(PathBuf, Option<Vec<u8>>)>,
}

impl RestoreGeneratedFiles {
    fn snapshot(root: &Path) -> Self {
        let generated_dir = root.join("examples/phase0-app/src/.generated");
        let files = ["crate-root.rs", "components.rs"]
            .into_iter()
            .map(|name| {
                let path = generated_dir.join(name);
                let contents = std::fs::read(&path).ok();
                (path, contents)
            })
            .collect();
        Self { files }
    }
}

impl Drop for RestoreGeneratedFiles {
    fn drop(&mut self) {
        for (path, contents) in &self.files {
            match contents {
                Some(bytes) => {
                    let _ = std::fs::write(path, bytes);
                }
                None => {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }
}

#[test]
#[ignore = "spawns outou-lsp + rust-analyzer 8 times; run explicitly, see module docs"]
fn gate3_probes_pass_through_the_real_parser_and_pipeline() {
    if !node_available() {
        eprintln!("gate3: skipping — `node` is not available on PATH");
        return;
    }
    let Some(ra) = rust_analyzer_binary() else {
        eprintln!(
            "gate3: skipping — no rust-analyzer binary found (checked OUTOU_RUST_ANALYZER and PATH)"
        );
        return;
    };

    let root = repo_root();
    let outou_lsp = PathBuf::from(env!("CARGO_BIN_EXE_outou-lsp"));

    // The `diagnostic-type-error` probe exercises `textDocument/didSave`,
    // which (per the architecture note) writes the current generated text
    // to disk so rust-analyzer's flycheck can see it — that includes the
    // probe's own injected type error, so `examples/phase0-app`'s
    // `.generated/` files must be restored afterward rather than left
    // holding a deliberately broken build.
    let _restore_generated = RestoreGeneratedFiles::snapshot(&root);

    let mut summary = String::new();
    let mut failures: Vec<String> = Vec::new();

    for &probe in PROBES {
        eprintln!("gate3: running probe `{probe}`...");
        let result = run_probe(&root, &outou_lsp, &ra, probe);
        save_gzipped(&root, probe, &result);

        if let Err(message) = check_probe(probe, &result) {
            failures.push(format!("{probe}: {message}"));
        }

        writeln!(
            summary,
            "{probe}: latencyMs={}",
            serde_json::to_string(result.get("latencyMs").unwrap_or(&serde_json::Value::Null))
                .unwrap_or_default()
        )
        .unwrap();
    }

    println!("Gate 3 probe summary:\n{summary}");
    assert!(
        failures.is_empty(),
        "Gate 3 probe(s) failed:\n{}",
        failures.join("\n")
    );
}

/// Checks the one thing that matters for each probe's Gate 3 criterion,
/// returning `Err` with a short reason on failure rather than panicking
/// immediately, so a single run reports every failing probe at once.
fn check_probe(probe: &str, result: &serde_json::Value) -> Result<(), String> {
    match probe {
        "hover-user" => {
            let value = result
                .pointer("/hover/contents/value")
                .and_then(|v| v.as_str())
                .ok_or("no hover contents")?;
            if !value.contains("Option") {
                return Err(format!("hover did not mention `Option`: {value}"));
            }
            Ok(())
        }
        "definition-load-user" => {
            let uri = result
                .pointer("/definition/0/uri")
                .and_then(|v| v.as_str())
                .ok_or("no definition location")?;
            if !uri.ends_with("main.rsx") {
                return Err(format!("definition did not land in main.rsx: {uri}"));
            }
            Ok(())
        }
        "definition-user-card" => {
            let uri = result
                .pointer("/definition/0/uri")
                .and_then(|v| v.as_str())
                .ok_or("no cross-file definition location")?;
            if !uri.ends_with("components.rsx") {
                return Err(format!(
                    "cross-file definition did not land in components.rsx: {uri}"
                ));
            }
            Ok(())
        }
        "completion-member" => has_completion_item(result, "unwrap")
            .then_some(())
            .ok_or_else(|| "member completion did not include `unwrap`".to_string()),
        "completion-component" => has_completion_item(result, "UserCard")
            .then_some(())
            .ok_or_else(|| "component completion did not include `UserCard`".to_string()),
        "completion-prop" => has_completion_item(result, "user")
            .then_some(())
            .ok_or_else(|| "prop-value completion did not include `user`".to_string()),
        "diagnostic-type-error" => {
            let has_type_error = result
                .get("diagnostics")
                .and_then(|d| d.as_object())
                .into_iter()
                .flat_map(|d| d.values())
                .flat_map(|v| v.as_array().into_iter().flatten())
                .any(|diag| {
                    diag.get("message")
                        .and_then(|m| m.as_str())
                        .is_some_and(|m| m.contains("mismatched types"))
                });
            has_type_error
                .then_some(())
                .ok_or_else(|| "no mismatched-types diagnostic was published".to_string())
        }
        "diagnostic-syntax-error" => {
            let has_outou_diag = result
                .get("diagnostics")
                .and_then(|d| d.as_object())
                .into_iter()
                .flat_map(|d| d.values())
                .flat_map(|v| v.as_array().into_iter().flatten())
                .any(|diag| diag.get("source").and_then(|s| s.as_str()) == Some("outou"));
            if !has_outou_diag {
                return Err("no Outou syntax diagnostic was published".to_string());
            }
            let definition_still_works = result
                .pointer("/definitionDespiteSyntaxError/0/uri")
                .and_then(|v| v.as_str())
                .is_some_and(|u| u.ends_with("main.rsx"));
            if !definition_still_works {
                return Err(
                    "definition elsewhere in the file stopped working after the syntax error"
                        .to_string(),
                );
            }
            Ok(())
        }
        other => Err(format!("unknown probe: {other}")),
    }
}

fn has_completion_item(result: &serde_json::Value, label: &str) -> bool {
    let completion = result.get("completion");
    let items = completion.and_then(|c| c.as_array()).or_else(|| {
        completion
            .and_then(|c| c.get("items"))
            .and_then(|i| i.as_array())
    });
    items
        .into_iter()
        .flatten()
        .any(|item| item.get("label").and_then(|l| l.as_str()) == Some(label))
}
