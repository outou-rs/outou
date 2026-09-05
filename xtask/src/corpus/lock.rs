//! A hand-rolled parser for `corpus.lock`'s minimal TOML subset: `#` line
//! comments, blank lines, `[[corpus]]` table headers, and `key = "value"`
//! string assignments naming `name`, `repo`, `path`, and either `commit`
//! or `tag`. This crate does not otherwise need a TOML dependency, so a
//! short hand parser is used instead of pulling one in for four fields
//! (issue #12).

use std::fmt;

/// One corpus entry from `corpus.lock`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusEntry {
    /// Name used for the `.corpus/<name>/` checkout directory.
    pub name: String,
    /// Git URL to clone.
    pub repo: String,
    /// Path, relative to the repository root, sparse-checked-out and
    /// scanned by `corpus test`.
    pub path: String,
    /// The pinned tag or commit.
    pub revision: Revision,
}

/// The pinned revision of a [`CorpusEntry`]. `Tag` is preferred (issue
/// #12: pin to a stable tag instead of a moving commit); `Commit` is
/// still accepted for an entry that needs an exact SHA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revision {
    /// A tag such as `1.98.1`.
    Tag(String),
    /// A commit hash.
    Commit(String),
}

impl Revision {
    /// The value recorded in a `.outou-revision` marker file, and
    /// compared against on the next `corpus fetch` to decide whether the
    /// existing checkout is already at the right revision.
    pub fn marker(&self) -> String {
        match self {
            Revision::Tag(tag) => format!("tag:{tag}"),
            Revision::Commit(commit) => format!("commit:{commit}"),
        }
    }
}

impl fmt::Display for Revision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Revision::Tag(tag) => write!(f, "tag {tag}"),
            Revision::Commit(commit) => write!(f, "commit {commit}"),
        }
    }
}

/// Parses `corpus.lock`'s full text into its entries. Fails on the first
/// structural problem (unknown key, missing field, both `tag` and
/// `commit` given, or a key outside any `[[corpus]]` table), naming the
/// offending line.
pub fn parse(text: &str) -> Result<Vec<CorpusEntry>, String> {
    let mut entries = Vec::new();
    let mut current: Option<PartialEntry> = None;

    for (index, raw_line) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[corpus]]" {
            if let Some(partial) = current.take() {
                entries.push(partial.finish(line_number)?);
            }
            current = Some(PartialEntry::default());
            continue;
        }
        let Some(partial) = current.as_mut() else {
            return Err(format!(
                "corpus.lock:{line_number}: expected `[[corpus]]` before any key, found `{line}`"
            ));
        };
        let (key, value) = parse_assignment(line, line_number)?;
        match key {
            "name" => partial.name = Some(value),
            "repo" => partial.repo = Some(value),
            "path" => partial.path = Some(value),
            "tag" => partial.tag = Some(value),
            "commit" => partial.commit = Some(value),
            other => return Err(format!("corpus.lock:{line_number}: unknown key `{other}`")),
        }
    }
    if let Some(partial) = current.take() {
        entries.push(partial.finish(text.lines().count())?);
    }
    if entries.is_empty() {
        return Err("corpus.lock: no `[[corpus]]` entries found".to_string());
    }
    Ok(entries)
}

#[derive(Default)]
struct PartialEntry {
    name: Option<String>,
    repo: Option<String>,
    path: Option<String>,
    tag: Option<String>,
    commit: Option<String>,
}

impl PartialEntry {
    fn finish(self, line_number: usize) -> Result<CorpusEntry, String> {
        let name = self.name.ok_or_else(|| {
            format!("corpus.lock: entry ending near line {line_number} is missing `name`")
        })?;
        let repo = self
            .repo
            .ok_or_else(|| format!("corpus.lock: entry `{name}` is missing `repo`"))?;
        let path = self
            .path
            .ok_or_else(|| format!("corpus.lock: entry `{name}` is missing `path`"))?;
        let revision = match (self.tag, self.commit) {
            (Some(tag), None) => Revision::Tag(tag),
            (None, Some(commit)) => Revision::Commit(commit),
            (Some(_), Some(_)) => {
                return Err(format!(
                    "corpus.lock: entry `{name}` has both `tag` and `commit`; pick one"
                ))
            }
            (None, None) => {
                return Err(format!(
                    "corpus.lock: entry `{name}` is missing `tag` or `commit`"
                ))
            }
        };
        Ok(CorpusEntry {
            name,
            repo,
            path,
            revision,
        })
    }
}

/// Parses one `key = "value"` line. Values are plain double-quoted
/// strings; the fields this file uses (URLs, path fragments, tags,
/// commit hashes) never need an escape.
fn parse_assignment(line: &str, line_number: usize) -> Result<(&str, String), String> {
    let (key, rest) = line.split_once('=').ok_or_else(|| {
        format!("corpus.lock:{line_number}: expected `key = \"value\"`, found `{line}`")
    })?;
    let key = key.trim();
    let rest = rest.trim();
    let value = rest
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| {
            format!("corpus.lock:{line_number}: value for `{key}` must be a double-quoted string, found `{rest}`")
        })?;
    Ok((key, value.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_single_tag_pinned_entry() {
        let text = r#"
# a comment
[[corpus]]
name = "rust-ui-tests"
repo = "https://github.com/rust-lang/rust.git"
path = "tests/ui"
tag = "1.98.1"
"#;
        let entries = parse(text).expect("valid lock file");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "rust-ui-tests");
        assert_eq!(entries[0].repo, "https://github.com/rust-lang/rust.git");
        assert_eq!(entries[0].path, "tests/ui");
        assert_eq!(entries[0].revision, Revision::Tag("1.98.1".to_string()));
        assert_eq!(entries[0].revision.marker(), "tag:1.98.1");
    }

    #[test]
    fn parses_a_commit_pinned_entry() {
        let text = r#"
[[corpus]]
name = "example"
repo = "https://example.invalid/repo.git"
path = "src"
commit = "abc123"
"#;
        let entries = parse(text).expect("valid lock file");
        assert_eq!(entries[0].revision, Revision::Commit("abc123".to_string()));
    }

    #[test]
    fn parses_multiple_entries() {
        let text = r#"
[[corpus]]
name = "a"
repo = "https://example.invalid/a.git"
path = "a"
tag = "1.0.0"

[[corpus]]
name = "b"
repo = "https://example.invalid/b.git"
path = "b"
commit = "deadbeef"
"#;
        let entries = parse(text).expect("valid lock file");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "a");
        assert_eq!(entries[1].name, "b");
    }

    #[test]
    fn rejects_both_tag_and_commit() {
        let text = r#"
[[corpus]]
name = "a"
repo = "https://example.invalid/a.git"
path = "a"
tag = "1.0.0"
commit = "deadbeef"
"#;
        let error = parse(text).unwrap_err();
        assert!(error.contains("both `tag` and `commit`"), "{error}");
    }

    #[test]
    fn rejects_missing_revision() {
        let text = r#"
[[corpus]]
name = "a"
repo = "https://example.invalid/a.git"
path = "a"
"#;
        let error = parse(text).unwrap_err();
        assert!(error.contains("missing `tag` or `commit`"), "{error}");
    }

    #[test]
    fn rejects_key_outside_any_table() {
        let text = "name = \"a\"\n";
        let error = parse(text).unwrap_err();
        assert!(error.contains("expected `[[corpus]]`"), "{error}");
    }

    #[test]
    fn rejects_unknown_key() {
        let text = r#"
[[corpus]]
name = "a"
repo = "https://example.invalid/a.git"
path = "a"
tag = "1.0.0"
branch = "main"
"#;
        let error = parse(text).unwrap_err();
        assert!(error.contains("unknown key `branch`"), "{error}");
    }

    #[test]
    fn rejects_empty_lock_file() {
        let error = parse("# nothing here\n").unwrap_err();
        assert!(error.contains("no `[[corpus]]` entries"), "{error}");
    }

    #[test]
    fn the_real_corpus_lock_parses() {
        let text = include_str!("../../../corpus.lock");
        let entries = parse(text).expect("the repository's corpus.lock must parse");
        assert!(!entries.is_empty());
        for entry in &entries {
            assert!(
                matches!(entry.revision, Revision::Tag(_)),
                "entry `{}` should be pinned to a tag, not a moving commit (issue #12)",
                entry.name
            );
        }
    }
}
