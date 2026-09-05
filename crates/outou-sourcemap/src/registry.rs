use std::collections::BTreeMap;

use crate::{SourceMap, Span, Uri};

/// Workspace-wide index: generated file URI → [`SourceMap`] → original URIs.
///
/// Every reverse-mapping decision goes through here, including "go to
/// definition" results that land in a *different* generated file than the
/// one the user is editing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Registry {
    by_generated: BTreeMap<Uri, SourceMap>,
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a new registry that also contains `map`, replacing any
    /// previous map for the same generated URI.
    pub fn with_map(self, map: SourceMap) -> Self {
        let mut by_generated = self.by_generated;
        by_generated.insert(map.generated.clone(), map);
        Self { by_generated }
    }

    /// The source map for a generated file, if a `.rsx` file produced it.
    ///
    /// `None` means the location must be returned to the user untouched.
    pub fn map_for_generated(&self, generated: &Uri) -> Option<&SourceMap> {
        self.by_generated.get(generated)
    }

    /// Original `.rsx` URIs of a generated file, or an empty slice for
    /// plain Rust files.
    pub fn sources_of(&self, generated: &Uri) -> &[Uri] {
        self.map_for_generated(generated)
            .map(|m| m.sources.as_slice())
            .unwrap_or(&[])
    }

    /// Whether `uri` is a generated file known to this registry.
    pub fn is_generated(&self, uri: &Uri) -> bool {
        self.by_generated.contains_key(uri)
    }

    /// Reverse-maps a location in generated Rust, per ADR 0007
    /// (`docs/adr/0007-source-map-many-to-many.md`):
    ///
    /// - If `generated_uri` is not a file this registry produced, returns
    ///   `None`. The caller must return the location untouched: a plain
    ///   `.rs` module or a dependency crate location must not be corrupted.
    /// - Otherwise returns `Some`, resolving every source span the location
    ///   maps to (see [`SourceMap::map_range`]) to its source `Uri`. The
    ///   list is empty when the location is synthesized code with no
    ///   source, which is an ordinary, expected result, not an error.
    pub fn reverse(&self, generated_uri: &Uri, span: Span) -> Option<Vec<(Uri, Span)>> {
        let map = self.map_for_generated(generated_uri)?;
        let mapped = map.map_range(span);
        let resolved = mapped
            .sources
            .into_iter()
            .filter_map(|source| {
                map.source_uri(source.source)
                    .map(|uri| (uri.clone(), source.span))
            })
            .collect();
        Some(resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_files_are_left_alone() {
        let reg = Registry::new();
        let dep = Uri::new("file:///home/.cargo/registry/src/foo/lib.rs");
        assert!(reg.map_for_generated(&dep).is_none());
        assert!(reg.sources_of(&dep).is_empty());
    }

    #[test]
    fn generated_files_resolve_to_their_sources() {
        let generated = Uri::new("file:///app/src/.generated/components.rs");
        let source = Uri::new("file:///app/src/components.rsx");
        let reg = Registry::new().with_map(SourceMap::new(generated.clone(), vec![source.clone()]));
        assert_eq!(reg.sources_of(&generated), &[source]);
    }

    /// A location in a dependency crate (or any other file this registry
    /// never generated) must pass through untouched: `reverse` returns
    /// `None`, and the caller is expected to keep using the original
    /// location rather than substitute anything.
    #[test]
    fn dependency_locations_pass_through_untouched() {
        use crate::Span;

        let reg = Registry::new();
        let dep = Uri::new("file:///home/.cargo/registry/src/foo/lib.rs");
        assert_eq!(reg.reverse(&dep, Span::new(0, 10)), None);
    }

    #[test]
    fn reverse_resolves_generated_locations_to_source_uris() {
        use crate::{Mapping, MappingKind, SourceId, SourceSpan, Span};

        let generated = Uri::new("file:///app/src/.generated/App.rs");
        let source = Uri::new("file:///app/src/App.rsx");
        let map =
            SourceMap::new(generated.clone(), vec![source.clone()]).with_mapping(Mapping::new(
                Span::new(0, 8),
                vec![SourceSpan::new(SourceId(0), Span::new(1, 9))],
                MappingKind::Identifier,
            ));
        let reg = Registry::new().with_map(map);

        let resolved = reg
            .reverse(&generated, Span::new(2, 3))
            .expect("a known generated file resolves to Some");
        assert_eq!(resolved, vec![(source, Span::new(1, 9))]);
    }

    #[test]
    fn reverse_of_synthesized_code_is_an_empty_but_known_result() {
        use crate::{Mapping, MappingKind, Span};

        let generated = Uri::new("file:///app/src/.generated/App.rs");
        let map = SourceMap::new(generated.clone(), vec![]).with_mapping(Mapping::new(
            Span::new(0, 4),
            vec![],
            MappingKind::Other,
        ));
        let reg = Registry::new().with_map(map);

        assert_eq!(reg.reverse(&generated, Span::new(1, 2)), Some(vec![]));
    }
}
