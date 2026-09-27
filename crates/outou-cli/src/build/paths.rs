//! Crate-relative display paths for generated output (ADR 0008).
//!
//! Generated Rust must be identical regardless of which machine generated
//! it, so a published crate's committed `src/.generated/` matches what CI
//! (or any consumer) regenerates. The generated `.rs` header
//! (`outou_codegen::GenerateOptions::source_display`, set from
//! [`PlannedUnit::source_display`] in [`super::plan`]) and the on-disk
//! `.rs.map.json` sidecar's own `generated`/`sources` fields
//! ([`relativize_map_json`], applied in [`super::emit`]) both need the
//! same crate-relative form. Both `outou-cli` and `crate::package`'s
//! in-memory drift check use this one implementation rather than each
//! reimplementing path relativization.

use std::path::Path;

/// `path`'s components relative to `crate_dir`, joined with `/`
/// regardless of platform.
///
/// Every path this pipeline relativizes was built by joining `crate_dir`
/// with a module-resolved relative path, so it is always actually under
/// `crate_dir`; a path that somehow is not falls back to its own
/// components, still forward-slash-joined, rather than panicking.
pub fn crate_relative_display(crate_dir: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(crate_dir).unwrap_or(path);
    relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Rewrites a source map JSON's `generated`/`sources` fields — written by
/// [`outou_sourcemap::SourceMap::to_json`] as absolute `file://` URIs —
/// to crate-relative display paths. `version` is left at 1: the v1 schema
/// already documents `generated`/`sources` as "a path or URI"
/// (`outou-sourcemap`'s `json.rs`), so a crate-relative path is still a
/// valid v1 document, and `SourceMapJson::into_source_map` only ever
/// accepts `version: 1`.
pub fn relativize_map_json(
    map: &mut outou_sourcemap::SourceMapJson,
    crate_dir: &Path,
    generated_file: &Path,
    source_files: &[&Path],
) {
    map.generated = crate_relative_display(crate_dir, generated_file);
    map.sources = source_files
        .iter()
        .map(|source| crate_relative_display(crate_dir, source))
        .collect();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn relativizes_a_path_under_the_crate_dir() {
        let crate_dir = PathBuf::from("/home/dev/proj/ui-kit");
        let path = crate_dir.join("src/widgets/button.rsx");
        assert_eq!(
            crate_relative_display(&crate_dir, &path),
            "src/widgets/button.rsx"
        );
    }

    #[test]
    fn two_different_absolute_prefixes_yield_the_same_display() {
        let a = PathBuf::from("/Users/dev/outou/ui-kit");
        let b = PathBuf::from("/home/runner/work/outou/outou/ui-kit");
        let target_a = a.join("src/lib.rsx");
        let target_b = b.join("src/lib.rsx");
        assert_eq!(
            crate_relative_display(&a, &target_a),
            crate_relative_display(&b, &target_b)
        );
    }

    #[test]
    fn relativize_map_json_rewrites_fields_and_keeps_version_1() {
        let crate_dir = PathBuf::from("/home/dev/proj/ui-kit");
        let generated = crate_dir.join("src/.generated/crate-root.rs");
        let source = crate_dir.join("src/lib.rsx");
        let mut map = outou_sourcemap::SourceMapJson {
            version: 1,
            generated: "file:///home/dev/proj/ui-kit/src/.generated/crate-root.rs".to_string(),
            sources: vec!["file:///home/dev/proj/ui-kit/src/lib.rsx".to_string()],
            comment: None,
            mappings: Vec::new(),
        };

        relativize_map_json(&mut map, &crate_dir, &generated, &[&source]);

        assert_eq!(map.version, 1);
        assert_eq!(map.generated, "src/.generated/crate-root.rs");
        assert_eq!(map.sources, vec!["src/lib.rsx".to_string()]);
    }
}
