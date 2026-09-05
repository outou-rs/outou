//! Filesystem probing and small attribute-text helpers used while
//! resolving one `mod` declaration: which of `CANDIDATES` exists
//! ([`resolve_by_candidates`]), whether a target lives under the crate
//! root ([`crate_relative`]), and attribute classification
//! ([`is_cfg_attribute`], [`conditional_path_attribute`]).

use std::fs;
use std::path::{Path, PathBuf};

use outou_syntax::Span;

use crate::{relative_to, ModuleError};

/// Index in [`crate::CANDIDATES`] at which the mod-rs-style patterns
/// (`name/mod.rsx`, `name/mod.rs`) start.
const MOD_RS_STYLE_START: usize = 2;

/// Whether `path` exists as an ordinary file (not a directory), for
/// evaluating one candidate (issue #7 finding 8). A `NotFound` metadata
/// error means the candidate simply does not exist (`Ok(false)`); a
/// directory of the same name is not a candidate either (`Ok(false)`) —
/// this is what stops a stray directory from being counted as an
/// `Ambiguous` sibling of a real file. Any other I/O failure (permission
/// denied, …) is surfaced as [`ModuleError::Io`] rather than silently
/// treated as "not found".
pub(crate) fn existing_file(
    path: &Path,
    name: &str,
    declared_in: &Path,
    span: Span,
    crate_dir: &Path,
) -> Result<bool, ModuleError> {
    match fs::metadata(path) {
        Ok(meta) => Ok(meta.is_file()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(ModuleError::Io {
            name: name.to_string(),
            path: relative_to(path, crate_dir),
            declared_in: relative_to(declared_in, crate_dir),
            span,
            source,
        }),
    }
}

/// Evaluates the four candidates for `mod {name};` under `lookup_dir`:
/// exactly one existing file is a match, more than one is
/// [`ModuleError::Ambiguous`] (ADR 0006), none is [`ModuleError::NotFound`].
/// Returns the matching path together with whether it was found in
/// mod-rs-style position (`name/mod.rs`/`name/mod.rsx`, the last two of
/// [`crate::CANDIDATES`]), which decides how the caller builds the child
/// [`crate::scope::DirScope`].
pub(crate) fn resolve_by_candidates(
    lookup_dir: &Path,
    name: &str,
    crate_dir: &Path,
    declared_in: &Path,
    span: Span,
) -> Result<(PathBuf, bool), ModuleError> {
    let all = crate::candidates(lookup_dir, name);
    let mut existing = Vec::new();
    for (index, candidate) in all.iter().enumerate() {
        if existing_file(candidate, name, declared_in, span, crate_dir)? {
            existing.push((candidate.clone(), index >= MOD_RS_STYLE_START));
        }
    }
    match existing.len() {
        0 => Err(ModuleError::NotFound {
            name: name.to_string(),
            declared_in: relative_to(declared_in, crate_dir),
            span,
            candidates: all.iter().map(|p| relative_to(p, crate_dir)).collect(),
        }),
        1 => Ok(existing.into_iter().next().expect("checked len == 1")),
        _ => Err(ModuleError::Ambiguous {
            name: name.to_string(),
            declared_in: relative_to(declared_in, crate_dir),
            span,
            candidates: existing
                .into_iter()
                .map(|(p, _)| relative_to(&p, crate_dir))
                .collect(),
        }),
    }
}

/// Whether an attribute's verbatim text is `#[cfg(…)]` (not `cfg_attr`,
/// which has its own, unrelated meaning). Delegates to
/// [`outou_syntax::parser::attribute_meta_path`] (issue #7 finding 9)
/// instead of an ad-hoc splitter, so `#[ cfg(...) ]` (leading trivia
/// inside the brackets) is recognized the same way the parser recognizes
/// it, instead of only matching a token that starts exactly at `cfg`.
pub(crate) fn is_cfg_attribute(text: &str) -> bool {
    outou_syntax::parser::attribute_meta_path(text) == Some("cfg")
}

/// If any of `attributes` is a `#[cfg_attr(condition, path = "…")]` naming
/// `path` as a top-level argument, returns its verbatim text (issue #7
/// finding 6 / decision 6). Phase 0 does not evaluate `cfg`, so it cannot
/// decide which of a conditional path's possible targets to resolve;
/// resolving the un-conditioned file instead would silently compile a
/// different file than the one the real build uses.
pub(crate) fn conditional_path_attribute(attributes: &[String]) -> Option<String> {
    attributes
        .iter()
        .find(|text| {
            outou_syntax::parser::attribute_meta_path(text) == Some("cfg_attr")
                && cfg_attr_names_a_path(text)
        })
        .cloned()
}

/// Whether a `#[cfg_attr(...)]` attribute's top-level argument list names
/// `path` as one of its comma-separated meta items.
fn cfg_attr_names_a_path(text: &str) -> bool {
    let Some(open) = text.find('(') else {
        return false;
    };
    let Some(close) = text.rfind(')') else {
        return false;
    };
    if close <= open {
        return false;
    }
    split_top_level_commas(&text[open + 1..close])
        .iter()
        .any(|segment| is_path_meta_item(segment.trim()))
}

/// Whether a single (trimmed) `cfg_attr` argument is the `path` meta item
/// (`path = "…"`) rather than, say, a `cfg`-style predicate.
fn is_path_meta_item(segment: &str) -> bool {
    match segment.strip_prefix("path") {
        Some(rest) => rest.is_empty() || rest.trim_start().starts_with('='),
        None => false,
    }
}

/// Splits `text` on top-level commas, tracking paren depth so that a
/// nested predicate such as `all(unix, target_os = "linux")` is not split
/// in the middle.
fn split_top_level_commas(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, ch) in text.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

/// Rewrites `path` relative to `crate_dir`, rejecting it with
/// [`ModuleError::OutsideCrate`] when it does not lexically resolve to
/// somewhere under `crate_dir` (issue #7 finding 10 / decision 9): an
/// absolute `#[path]`, or one escaping via enough `..` segments. `path`
/// and `crate_dir` are compared after a purely lexical `.`/`..`
/// normalization (no filesystem access), so this works for a target that
/// does not exist yet.
pub(crate) fn crate_relative(
    path: &Path,
    crate_dir: &Path,
    name: &str,
    declared_in: &Path,
    span: Span,
) -> Result<PathBuf, ModuleError> {
    let normalized = normalize_lexically(path);
    let normalized_crate = normalize_lexically(crate_dir);
    match normalized.strip_prefix(&normalized_crate) {
        Ok(rel) => Ok(rel.to_path_buf()),
        Err(_) => Err(ModuleError::OutsideCrate {
            name: name.to_string(),
            declared_in: relative_to(declared_in, crate_dir),
            span,
            path: path.to_path_buf(),
        }),
    }
}

/// Resolves `.` and `..` components lexically, without touching the
/// filesystem (so it works for paths that do not exist).
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    fn span() -> Span {
        Span::new(0, 1)
    }

    #[test]
    fn is_cfg_attribute_matches_cfg_but_not_cfg_attr() {
        assert!(is_cfg_attribute("#[cfg(feature = \"x\")]"));
        assert!(is_cfg_attribute("#[cfg(test)]"));
        assert!(is_cfg_attribute("#[ cfg(feature = \"y\") ]"));
        assert!(!is_cfg_attribute("#[cfg_attr(test, ignore)]"));
        assert!(!is_cfg_attribute("#[component]"));
        assert!(!is_cfg_attribute("#[path = \"a.rs\"]"));
    }

    #[test]
    fn conditional_path_attribute_finds_a_top_level_path_argument() {
        let attrs = vec!["#[cfg_attr(unix, path = \"unix.rsx\")]".to_string()];
        assert_eq!(
            conditional_path_attribute(&attrs).as_deref(),
            Some("#[cfg_attr(unix, path = \"unix.rsx\")]")
        );
    }

    #[test]
    fn conditional_path_attribute_ignores_cfg_attr_without_path() {
        let attrs = vec!["#[cfg_attr(test, ignore)]".to_string()];
        assert_eq!(conditional_path_attribute(&attrs), None);
    }

    #[test]
    fn conditional_path_attribute_ignores_a_nested_predicate_named_path() {
        // `path` only counts as the conditional-path hazard when it is a
        // top-level argument, not buried inside a nested predicate.
        let attrs = vec!["#[cfg_attr(all(unix, path), inline)]".to_string()];
        assert_eq!(conditional_path_attribute(&attrs), None);
    }

    #[test]
    fn existing_file_rejects_a_directory() {
        let tmp = TempDir::new("existing-file-dir");
        let dir_path = tmp.path().join("foo.rs");
        fs::create_dir(&dir_path).expect("creating decoy directory");
        let result = existing_file(
            &dir_path,
            "foo",
            Path::new("src/main.rs"),
            span(),
            tmp.path(),
        );
        assert!(!result.expect("existing_file should not error"));
    }

    #[cfg(unix)]
    #[test]
    fn existing_file_maps_permission_denied_to_io() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new("existing-file-perm");
        let blocked = tmp.path().join("blocked");
        fs::create_dir(&blocked).expect("creating blocked dir");
        let target = blocked.join("mod.rsx");
        fs::write(&target, "").expect("writing target");
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000))
            .expect("removing directory permissions");

        let result = existing_file(&target, "m", Path::new("src/main.rs"), span(), tmp.path());

        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o755))
            .expect("restoring permissions for cleanup");
        assert!(matches!(result, Err(ModuleError::Io { .. })), "{result:?}");
    }

    #[test]
    fn crate_relative_accepts_targets_under_the_crate() {
        let rel = crate_relative(
            Path::new("/repo/src/foo.rs"),
            Path::new("/repo"),
            "foo",
            Path::new("/repo/src/main.rsx"),
            span(),
        )
        .expect("target is under the crate");
        assert_eq!(rel, PathBuf::from("src/foo.rs"));
    }

    #[test]
    fn crate_relative_rejects_targets_outside_the_crate() {
        let err = crate_relative(
            Path::new("/repo/src/../../outside/foo.rs"),
            Path::new("/repo"),
            "foo",
            Path::new("/repo/src/main.rsx"),
            span(),
        )
        .expect_err("target escapes the crate root");
        assert!(matches!(err, ModuleError::OutsideCrate { .. }), "{err:?}");
    }

    #[test]
    fn crate_relative_rejects_an_absolute_target_outside_the_crate() {
        let err = crate_relative(
            Path::new("/etc/passwd.rs"),
            Path::new("/repo"),
            "thing",
            Path::new("/repo/src/main.rsx"),
            span(),
        )
        .expect_err("absolute target outside the crate");
        assert!(matches!(err, ModuleError::OutsideCrate { .. }), "{err:?}");
    }
}
