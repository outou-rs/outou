use std::collections::BTreeMap;

use crate::{SourceMap, Uri};

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
}
