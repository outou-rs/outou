//! Conversions between the two URI types this crate has to speak:
//! [`outou_sourcemap::Uri`] (a plain string, used by [`outou_sourcemap::Registry`]
//! and every source map) and [`lsp_types::Uri`] (backed by `fluent_uri`, used
//! on the wire to both the editor and rust-analyzer). Both are `file://` URIs
//! over the same percent-encoding, so conversion is just a string round-trip;
//! this module is the one place that does it, so a future encoding change
//! only has to be made here.
//!
//! # Identity of a `.rsx` document
//!
//! Every internal key (`HashMap<String, _>` keys built from [`to_outou`],
//! `Registry::is_generated`, the equality checks in `crate::rename`/
//! `crate::references`/`crate::semantic_tokens`) is a **lexical** string
//! comparison. That is only sound if every `.rsx` URI the server sees has
//! one spelling, so the editor's URI is normalized at the boundary
//! ([`normalize_file_uri`]: percent-decode, canonicalize on disk, re-encode)
//! and the editor's own spelling is restored on the way out
//! (`crate::editor_uris`). Reproduced with a real editor: a workspace
//! opened through a symbolic link (or `/tmp` -> `/private/tmp` on macOS)
//! made every key mismatch.
//!
//! `TODO(alpha) (#31)`: a full RFC 3986 normalization (an equivalent
//! authority such as `file://localhost/`, Windows drive letters and UNC
//! paths) is not implemented; such a URI is passed through unchanged.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use outou_sourcemap::{file_uri, Uri as OutouUri};

/// Converts an [`lsp_types::Uri`] to this crate's own [`OutouUri`].
pub fn to_outou(uri: &lsp_types::Uri) -> OutouUri {
    OutouUri::new(uri.as_str())
}

/// Converts an [`OutouUri`] to an [`lsp_types::Uri`].
///
/// Panics only if `uri` is not a syntactically valid URI, which would mean
/// this server (or `outou_sourcemap::file_uri`) produced a malformed one —
/// an internal bug, not a possible client input.
pub fn to_lsp(uri: &OutouUri) -> lsp_types::Uri {
    lsp_types::Uri::from_str(uri.as_str())
        .unwrap_or_else(|e| panic!("internal error: {} is not a valid URI: {e}", uri.as_str()))
}

/// Builds an [`lsp_types::Uri`] for an absolute filesystem path.
pub fn path_to_lsp(path: &Path) -> lsp_types::Uri {
    to_lsp(&file_uri(path))
}

/// Recovers an absolute filesystem path from an [`OutouUri`]'s string
/// form. Used to key [`crate::documents::Workspace`]'s planning overlay
/// (`crate::plan::resolve_with_overlay`) by real filesystem paths, the
/// same identity `outou_modules` compares against.
pub fn outou_uri_str_to_path(uri: &str) -> Option<PathBuf> {
    let lsp_uri: lsp_types::Uri = uri.parse().ok()?;
    to_path(&lsp_uri)
}

/// Recovers an absolute filesystem path from a `file://` URI, reversing
/// [`outou_sourcemap::file_uri`]'s percent-encoding. Returns `None` for a
/// non-`file` scheme (nothing this server would ever ask about).
///
/// `TODO(alpha) (#31)` (issue #9 Gate 3 review, LOW-16): `strip_prefix("file://")`
/// turns a Windows drive-letter URI (`file:///C:/...`) into `/C:/...`,
/// which is not a valid Windows path. Not reachable on the platforms
/// the alpha runs on (macOS/Linux); fixing it properly needs a URL crate
/// dependency this server does not otherwise have a reason to take.
pub fn to_path(uri: &lsp_types::Uri) -> Option<PathBuf> {
    path_from_file_uri(uri.as_str())
}

fn path_from_file_uri(text: &str) -> Option<PathBuf> {
    let rest = text.strip_prefix("file://")?;
    Some(PathBuf::from(percent_decode(rest)))
}

/// Rewrites a `file://` URI into the one spelling this crate keys on: the
/// path is percent-decoded, resolved on disk with [`canonicalize_lenient`]
/// (symbolic links, `..`, the on-disk spelling of a case-insensitive file
/// system) and re-encoded by [`file_uri`]. Anything that is not an
/// absolute `file://` URI is returned unchanged.
pub fn normalize_file_uri(uri: &str) -> String {
    let Some(path) = path_from_file_uri(uri).filter(|p| p.is_absolute()) else {
        return uri.to_string();
    };
    file_uri(&canonicalize_lenient(&path)).as_str().to_string()
}

/// `fs::canonicalize`, except that a path which does not exist (yet) is
/// resolved through its nearest existing ancestor, so a new file in a
/// symlinked directory still gets its directory's real spelling. A path
/// with no resolvable ancestor is returned as written.
pub fn canonicalize_lenient(path: &Path) -> PathBuf {
    if let Ok(real) = path.canonicalize() {
        return real;
    }
    for ancestor in path.ancestors().skip(1) {
        let Ok(real) = ancestor.canonicalize() else {
            continue;
        };
        return match path.strip_prefix(ancestor) {
            Ok(rest) => real.join(rest),
            Err(_) => path.to_path_buf(),
        };
    }
    path.to_path_buf()
}

/// Percent-decodes a URI path component back into raw UTF-8 bytes,
/// tolerating (by passing through unchanged) any `%` not followed by two
/// hex digits rather than treating it as malformed input.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The value of one hex digit, or `None` if `b` is not one.
fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_plain_path() {
        let path = Path::new("/a/b/c.rsx");
        let lsp_uri = path_to_lsp(path);
        let back = to_path(&lsp_uri).unwrap();
        assert_eq!(back, path);
    }

    #[test]
    fn round_trips_a_path_with_a_space() {
        let path = Path::new("/a b/c.rsx");
        let lsp_uri = path_to_lsp(path);
        assert!(lsp_uri.as_str().contains("%20"));
        let back = to_path(&lsp_uri).unwrap();
        assert_eq!(back, path);
    }

    #[test]
    fn outou_and_lsp_uris_round_trip() {
        let outou_uri = OutouUri::new("file:///app/src/main.rsx");
        let lsp_uri = to_lsp(&outou_uri);
        assert_eq!(to_outou(&lsp_uri), outou_uri);
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("outou-lsp-uri-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn normalizes_a_symlinked_path_to_the_real_one() {
        let root = scratch_dir("link");
        let real = root.join("real");
        std::fs::create_dir_all(real.join("src")).unwrap();
        std::fs::write(real.join("src/main.rsx"), "").unwrap();
        std::os::unix::fs::symlink(&real, root.join("link")).unwrap();

        let via_link = file_uri(&root.join("link/src/main.rsx"));
        let expected = file_uri(&real.join("src/main.rsx"));
        assert_eq!(normalize_file_uri(via_link.as_str()), expected.as_str());
        // Already normal: unchanged.
        assert_eq!(normalize_file_uri(expected.as_str()), expected.as_str());
    }

    #[cfg(unix)]
    #[test]
    fn normalizes_a_link_to_a_link_and_dot_dot_segments() {
        let root = scratch_dir("chain");
        let real = root.join("real");
        std::fs::create_dir_all(real.join("src")).unwrap();
        std::fs::write(real.join("src/a.rsx"), "").unwrap();
        std::os::unix::fs::symlink(&real, root.join("l1")).unwrap();
        std::os::unix::fs::symlink(root.join("l1"), root.join("l2")).unwrap();

        let expected = file_uri(&real.join("src/a.rsx"));
        let chained = file_uri(&root.join("l2/src/a.rsx"));
        assert_eq!(normalize_file_uri(chained.as_str()), expected.as_str());
        let dotted = file_uri(&root.join("l2/src/../src/a.rsx"));
        assert_eq!(normalize_file_uri(dotted.as_str()), expected.as_str());
    }

    #[cfg(unix)]
    #[test]
    fn normalizes_percent_encoding_spelling() {
        let root = scratch_dir("pct");
        let dir = root.join("a b");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("x.rsx"), "").unwrap();
        let expected = file_uri(&dir.join("x.rsx"));
        // An unnecessarily escaped letter.
        let odd = expected.as_str().replace("/x.", "/%78.");
        assert_ne!(odd, expected.as_str());
        assert_eq!(normalize_file_uri(&odd), expected.as_str());
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_file_is_resolved_through_its_existing_directory() {
        let root = scratch_dir("missing");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, root.join("link")).unwrap();

        let via_link = file_uri(&root.join("link/new.rsx"));
        let expected = file_uri(&real.join("new.rsx"));
        assert_eq!(normalize_file_uri(via_link.as_str()), expected.as_str());
    }

    #[test]
    fn a_wholly_nonexistent_path_is_left_alone() {
        let uri = "file:///outou-no-such-root-xyz/a/b.rsx";
        assert_eq!(normalize_file_uri(uri), uri);
    }

    #[test]
    fn non_file_uris_are_returned_unchanged() {
        for uri in [
            "untitled:Untitled-1",
            "vscode-notebook://x/y",
            "file://relative",
        ] {
            assert_eq!(normalize_file_uri(uri), uri);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_tmp_resolves_to_private_tmp() {
        let dir = Path::new("/tmp").join(format!("outou-lsp-uri-tmp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("f.rsx"), "").unwrap();
        let via_tmp = file_uri(&dir.join("f.rsx"));
        let real = file_uri(
            &Path::new("/private/tmp")
                .join(dir.strip_prefix("/tmp").unwrap())
                .join("f.rsx"),
        );
        assert_eq!(normalize_file_uri(via_tmp.as_str()), real.as_str());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
