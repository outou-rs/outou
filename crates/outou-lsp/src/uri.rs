//! Conversions between the two URI types this crate has to speak:
//! [`outou_sourcemap::Uri`] (a plain string, used by [`outou_sourcemap::Registry`]
//! and every source map) and [`lsp_types::Uri`] (backed by `fluent_uri`, used
//! on the wire to both the editor and rust-analyzer). Both are `file://` URIs
//! over the same percent-encoding, so conversion is just a string round-trip;
//! this module is the one place that does it, so a future encoding change
//! only has to be made here.

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
/// TODO(phase0) (issue #9 Gate 3 review, LOW-16): `strip_prefix("file://")`
/// turns a Windows drive-letter URI (`file:///C:/...`) into `/C:/...`,
/// which is not a valid Windows path. Not reachable on the platforms
/// Phase 0 runs on (macOS/Linux); fixing it properly needs a URL crate
/// dependency this server does not otherwise have a reason to take.
pub fn to_path(uri: &lsp_types::Uri) -> Option<PathBuf> {
    let text = uri.as_str();
    let rest = text.strip_prefix("file://")?;
    Some(PathBuf::from(percent_decode(rest)))
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
}
