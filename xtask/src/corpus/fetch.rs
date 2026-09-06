//! `cargo xtask corpus fetch`: clones each `corpus.lock` entry at its
//! pinned tag or commit into `.corpus/<name>/`, shallow and sparse so
//! only the entry's `path` reaches disk. Nothing here is vendored into
//! the repository; `.corpus/` is git-ignored.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use super::lock::{CorpusEntry, Revision};

/// Marker file written into each corpus checkout recording the revision
/// it was fetched at, so a repeat `corpus fetch` can skip a checkout that
/// is already current instead of re-cloning it.
const REVISION_MARKER: &str = ".outou-revision";

/// A shallow clone taking longer than this is reported (not failed) as a
/// candidate for dropping down to a plainer fetch strategy (issue #12).
const SLOW_FETCH_WARNING: Duration = Duration::from_secs(600);

pub fn run(root: &Path, entries: &[CorpusEntry]) -> Result<(), String> {
    let corpus_dir = root.join(".corpus");
    fs::create_dir_all(&corpus_dir)
        .map_err(|e| format!("creating {}: {e}", corpus_dir.display()))?;

    let mut fetched = 0usize;
    let mut skipped = 0usize;
    for entry in entries {
        let dest = corpus_dir.join(&entry.name);
        if is_up_to_date(&dest, entry) {
            println!(
                "corpus fetch: {} already at {} (skipped)",
                entry.name, entry.revision
            );
            skipped += 1;
            continue;
        }
        if dest.exists() {
            fs::remove_dir_all(&dest)
                .map_err(|e| format!("removing stale checkout {}: {e}", dest.display()))?;
        }

        let start = Instant::now();
        clone_sparse(entry, &dest)?;
        let elapsed = start.elapsed();

        // F8 (issue #12 corpus review, MEDIUM): `corpus.lock:9`'s pinned
        // commit used to live only in a comment, never actually checked —
        // a retagged upstream would silently change the corpus, exactly
        // the thing a `.lock` file exists to prevent. Verify the freshly
        // cloned tag's resolved commit against `entry.verified_commit`
        // (when set) and fail loudly on a mismatch, before this checkout
        // is accepted as fetched.
        let resolved_commit = rev_parse_head(&dest)?;
        if let Some(expected) = &entry.verified_commit {
            if &resolved_commit != expected {
                return Err(format!(
                    "corpus fetch: {} ({}) resolved to commit {resolved_commit}, but corpus.lock's \
                     verified_commit says {expected} — upstream may have retagged; if this is a \
                     deliberate pin move, update corpus.lock's `tag` and `verified_commit` together",
                    entry.name, entry.revision
                ));
            }
        }

        // The marker records both the pinned revision (compared by
        // `is_up_to_date` to decide whether a repeat fetch can skip this
        // checkout) and the commit it actually resolved to, so a later
        // `corpus test` run can report it as provenance (F9) without
        // needing to invoke `git` again.
        let marker = format!("{}\n{resolved_commit}\n", entry.revision.marker());
        fs::write(dest.join(REVISION_MARKER), marker)
            .map_err(|e| format!("writing revision marker for {}: {e}", entry.name))?;

        println!(
            "corpus fetch: {} fetched at {} (commit {resolved_commit}) in {:.1}s",
            entry.name,
            entry.revision,
            elapsed.as_secs_f64()
        );
        if elapsed > SLOW_FETCH_WARNING {
            println!(
                "corpus fetch: {} took {:.1}s (> {}s); consider a plainer `--depth 1` sparse checkout without the blob filter for this entry",
                entry.name,
                elapsed.as_secs_f64(),
                SLOW_FETCH_WARNING.as_secs()
            );
        }
        fetched += 1;
    }
    println!("corpus fetch: {fetched} fetched, {skipped} up to date");
    Ok(())
}

/// Runs `git rev-parse HEAD` in `dir`, returning the resolved commit SHA.
fn rev_parse_head(dir: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .map_err(|e| format!("running `git rev-parse HEAD` in {}: {e}", dir.display()))?;
    if !output.status.success() {
        return Err(format!(
            "`git rev-parse HEAD` in {} exited with {}: {}",
            dir.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Reads the resolved commit `corpus fetch` recorded for `entry` (F8/F9),
/// if the checkout has a marker with one. `None` for a checkout that
/// predates this field, or that has not been fetched at all — callers
/// that need provenance should treat that the same as "unknown".
pub fn resolved_commit(root: &Path, entry: &CorpusEntry) -> Option<String> {
    let dest = root.join(".corpus").join(&entry.name);
    let content = fs::read_to_string(dest.join(REVISION_MARKER)).ok()?;
    content.lines().nth(1).map(str::to_string)
}

fn is_up_to_date(dest: &Path, entry: &CorpusEntry) -> bool {
    match fs::read_to_string(dest.join(REVISION_MARKER)) {
        // The marker's first line is the pinned revision (F8: a second
        // line, the resolved commit, may follow — irrelevant here).
        Ok(content) => content.lines().next() == Some(entry.revision.marker().as_str()),
        Err(_) => false,
    }
}

/// Clones `entry` into `dest` at its pinned revision, then narrows the
/// working tree to `entry.path` with sparse-checkout: `git clone
/// --filter=blob:none --no-checkout --depth 1 --branch <tag>` (or, for a
/// commit-pinned entry, an init/fetch/checkout sequence, since `git
/// clone --branch` only accepts a ref name, not an arbitrary SHA) +
/// `git sparse-checkout set <path>` + `git checkout`.
fn clone_sparse(entry: &CorpusEntry, dest: &Path) -> Result<(), String> {
    match &entry.revision {
        Revision::Tag(tag) => {
            let dest_str = dest
                .to_str()
                .ok_or_else(|| format!("{}: destination path is not valid UTF-8", entry.name))?;
            run_git(
                None,
                &[
                    "clone",
                    "--filter=blob:none",
                    "--no-checkout",
                    "--depth",
                    "1",
                    "--branch",
                    tag,
                    &entry.repo,
                    dest_str,
                ],
            )?;
        }
        Revision::Commit(commit) => {
            fs::create_dir_all(dest).map_err(|e| format!("creating {}: {e}", dest.display()))?;
            run_git(Some(dest), &["init", "--quiet"])?;
            run_git(Some(dest), &["remote", "add", "origin", &entry.repo])?;
            run_git(
                Some(dest),
                &[
                    "fetch",
                    "--filter=blob:none",
                    "--depth",
                    "1",
                    "origin",
                    commit,
                ],
            )?;
            run_git(Some(dest), &["checkout", "--quiet", "FETCH_HEAD"])?;
        }
    }
    // Both of these matter, not just for tidiness — measured against
    // rust-lang/rust's `tests/ui` (~20,000 files, tag `1.98.1`):
    //
    // 1. Cone mode. A plain (non-cone) `sparse-checkout set` combined
    //    with `--filter=blob:none` fetched missing blobs one at a time
    //    and did not finish within 10 minutes. `init --cone` first lets
    //    the promisor remote fetch the sparse directory's blobs as one
    //    batch, finishing in well under a minute.
    // 2. No trailing pathspec on the checkout itself. Even in cone mode,
    //    `git checkout HEAD -- .` re-triggered the same one-blob-at-a-time
    //    fetch (observed: still incomplete after 5+ minutes, killed).
    //    Checking out `HEAD` from the index with no pathspec is what lets
    //    git read the whole sparse tree from a single fetch instead of
    //    resolving `.` path by path.
    //
    // Together these mean the documented ">10 min, fall back to a
    // plainer `--depth 1` clone without the blob filter" path (issue #12)
    // is not needed once fetched this way from the start.
    run_git(Some(dest), &["sparse-checkout", "init", "--cone"])?;
    run_git(Some(dest), &["sparse-checkout", "set", &entry.path])?;
    run_git(Some(dest), &["checkout", "HEAD"])?;
    Ok(())
}

fn run_git(cwd: Option<&Path>, args: &[&str]) -> Result<(), String> {
    let mut command = Command::new("git");
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let status = command
        .status()
        .map_err(|e| format!("running `git {}`: {e}", args.join(" ")))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`git {}` exited with {status}", args.join(" ")))
    }
}

/// Where a corpus entry's checked-out content lives once fetched:
/// `.corpus/<name>/<path>`.
pub fn checkout_dir(root: &Path, entry: &CorpusEntry) -> PathBuf {
    root.join(".corpus").join(&entry.name).join(&entry.path)
}
