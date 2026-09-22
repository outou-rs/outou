//! The semantic token legend this server advertises, and how it is chosen.
//!
//! See `docs/adr/0012-semantic-tokens-legend-and-overlay.md` for the
//! reasoning. Summary: when a live rust-analyzer is attached, this
//! server's advertised legend is rust-analyzer's own legend **extended**
//! with any of Outou's five overlay type names it lacks
//! ([`Legend::extended_with_outou_types`]) — so every rust-analyzer
//! token's `tokenType`/`tokenModifiers` index passes through unchanged
//! (nothing already present is ever reordered), while Outou's own overlay
//! tokens (component, element, attribute, event, text) are still always
//! found by **name**. This module used to assume every rust-analyzer
//! legend is a superset of the standard LSP token type list; reproduced
//! live against rust-analyzer 1.98.1, that is false — its legend has
//! `type`, `property` and `string`, but not `class` or `event` (nor
//! `modifier`/`regexp`), so every component and event token was silently
//! dropped before this fix (issue #14 review, BLOCKING-4). In degraded
//! mode (no rust-analyzer at all), [`Legend::default_legend`] — the
//! standard LSP token type/modifier list, a superset of Outou's own five
//! names by construction — is used instead.

use serde_json::Value;

/// The token type names Outou's own AST-derived tokens use. Every one of
/// these is a standard LSP semantic token type name (`SemanticTokenType`'s
/// own constants), chosen per `docs/adr/0012...md`:
///
/// - a component reference (`<UserCard>`) -> `class` (it lowers to a
///   Rust path naming a function/struct-like construct)
/// - an intrinsic HTML element name (`<div>`) -> `type`
/// - a JSX attribute name -> `property`
/// - an event attribute (`onClick`) -> `event` (a real standard LSP type,
///   not an Outou invention)
/// - JSX text content -> `string`
pub const COMPONENT_TYPE: &str = "class";
pub const ELEMENT_TYPE: &str = "type";
pub const ATTRIBUTE_TYPE: &str = "property";
pub const EVENT_TYPE: &str = "event";
pub const TEXT_TYPE: &str = "string";

/// The standard LSP semantic token type names, in the LSP specification's
/// own declared order (`SemanticTokenType`'s constants, 3.16.0 plus the
/// 3.17.0 `decorator` addition) — used verbatim as [`Legend::default_legend`]
/// when no rust-analyzer is attached to translate against.
const STANDARD_TOKEN_TYPES: &[&str] = &[
    "namespace",
    "type",
    "class",
    "enum",
    "interface",
    "struct",
    "typeParameter",
    "parameter",
    "variable",
    "property",
    "enumMember",
    "event",
    "function",
    "method",
    "macro",
    "keyword",
    "modifier",
    "comment",
    "string",
    "number",
    "regexp",
    "operator",
    "decorator",
];

/// The standard LSP semantic token modifier names, same provenance as
/// [`STANDARD_TOKEN_TYPES`].
const STANDARD_TOKEN_MODIFIERS: &[&str] = &[
    "declaration",
    "definition",
    "readonly",
    "static",
    "deprecated",
    "abstract",
    "async",
    "modification",
    "documentation",
    "defaultLibrary",
];

/// A semantic token legend: the ordered type/modifier name lists a
/// `SemanticTokens` response's numeric indices are defined against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Legend {
    pub token_types: Vec<String>,
    pub token_modifiers: Vec<String>,
}

impl Legend {
    /// The standard LSP token type/modifier list, used when no live
    /// rust-analyzer legend is available to reuse.
    pub fn default_legend() -> Self {
        Self {
            token_types: STANDARD_TOKEN_TYPES.iter().map(|s| s.to_string()).collect(),
            token_modifiers: STANDARD_TOKEN_MODIFIERS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }

    /// Reads a legend verbatim from rust-analyzer's `initialize` result
    /// (`capabilities.semanticTokensProvider.legend`), or `None` if
    /// rust-analyzer did not advertise semantic tokens support at all.
    pub fn from_ra_initialize_result(result: &Value) -> Option<Self> {
        let legend = result.pointer("/capabilities/semanticTokensProvider/legend")?;
        let token_types = string_array(legend.get("tokenTypes")?)?;
        let token_modifiers = string_array(legend.get("tokenModifiers")?)?;
        Some(Self {
            token_types,
            token_modifiers,
        })
    }

    /// Returns a new legend equal to `self` with any of Outou's five
    /// overlay type names ([`COMPONENT_TYPE`], [`ELEMENT_TYPE`],
    /// [`ATTRIBUTE_TYPE`], [`EVENT_TYPE`], [`TEXT_TYPE`]) that are not
    /// already present **appended** at the end (issue #14 review,
    /// BLOCKING-4): rust-analyzer 1.98.1's real legend does not include
    /// `class` or `event`, contrary to this module's own former
    /// assumption that every rust-analyzer legend is a superset of the
    /// standard LSP list — every component/event token was silently
    /// dropped as a result (`type_index` returning `None`). Appending
    /// rather than inserting anywhere else keeps every existing name's
    /// index unchanged, so rust-analyzer's own tokens (already encoded
    /// against its original legend) never need re-indexing.
    pub fn extended_with_outou_types(&self) -> Self {
        let mut token_types = self.token_types.clone();
        for name in [
            COMPONENT_TYPE,
            ELEMENT_TYPE,
            ATTRIBUTE_TYPE,
            EVENT_TYPE,
            TEXT_TYPE,
        ] {
            if !token_types.iter().any(|t| t == name) {
                token_types.push(name.to_string());
            }
        }
        Self {
            token_types,
            token_modifiers: self.token_modifiers.clone(),
        }
    }

    /// The index of `name` in [`Legend::token_types`], if present.
    pub fn type_index(&self, name: &str) -> Option<u32> {
        self.token_types
            .iter()
            .position(|t| t == name)
            .map(|i| i as u32)
    }

    /// Converts to the `lsp_types` shape this server advertises in its own
    /// `initialize` response.
    pub fn to_lsp(&self) -> lsp_types::SemanticTokensLegend {
        lsp_types::SemanticTokensLegend {
            token_types: self
                .token_types
                .iter()
                .cloned()
                .map(lsp_types::SemanticTokenType::from)
                .collect(),
            token_modifiers: self
                .token_modifiers
                .iter()
                .cloned()
                .map(lsp_types::SemanticTokenModifier::from)
                .collect(),
        }
    }
}

fn string_array(value: &Value) -> Option<Vec<String>> {
    value
        .as_array()?
        .iter()
        .map(|v| v.as_str().map(str::to_string))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_legend_includes_every_type_outou_tokens_names() {
        let legend = Legend::default_legend();
        for name in [
            COMPONENT_TYPE,
            ELEMENT_TYPE,
            ATTRIBUTE_TYPE,
            EVENT_TYPE,
            TEXT_TYPE,
        ] {
            assert!(
                legend.type_index(name).is_some(),
                "default legend is missing standard type {name}"
            );
        }
    }

    #[test]
    fn from_ra_initialize_result_reads_a_real_legend() {
        let result = json!({
            "capabilities": {
                "semanticTokensProvider": {
                    "legend": {
                        "tokenTypes": ["namespace", "type", "class"],
                        "tokenModifiers": ["declaration"]
                    }
                }
            }
        });
        let legend = Legend::from_ra_initialize_result(&result).expect("a legend");
        assert_eq!(legend.token_types, vec!["namespace", "type", "class"]);
        assert_eq!(legend.type_index("class"), Some(2));
    }

    #[test]
    fn from_ra_initialize_result_is_none_without_semantic_tokens_support() {
        let result = json!({ "capabilities": {} });
        assert!(Legend::from_ra_initialize_result(&result).is_none());
    }

    #[test]
    fn type_index_is_none_for_an_unknown_name() {
        let legend = Legend::default_legend();
        assert_eq!(legend.type_index("not-a-real-type"), None);
    }

    /// Issue #14 review (BLOCKING-4): rust-analyzer 1.98.1's real legend
    /// does not include `class` or `event` (nor `modifier`/`regexp`) —
    /// only a subset of the standard LSP list. Appending whatever Outou
    /// names are missing must never disturb an index rust-analyzer's own
    /// tokens already rely on.
    #[test]
    fn extended_with_outou_types_appends_missing_names_without_disturbing_ra_indices() {
        let ra_legend = Legend {
            token_types: vec![
                "comment".to_string(),
                "type".to_string(),
                "property".to_string(),
                "string".to_string(),
            ],
            token_modifiers: vec!["declaration".to_string()],
        };
        let extended = ra_legend.extended_with_outou_types();

        // `type` (already present, index 1) is untouched.
        assert_eq!(extended.type_index("type"), Some(1));
        // `class` and `event` (missing from RA's legend) are appended, not
        // inserted, so every existing index stays valid.
        assert!(extended.type_index(COMPONENT_TYPE).is_some());
        assert!(extended.type_index(EVENT_TYPE).is_some());
        assert_eq!(
            extended.token_types[..ra_legend.token_types.len()],
            ra_legend.token_types[..]
        );
    }

    /// A legend that already has every Outou type name is returned
    /// unchanged (no duplicate entries appended).
    #[test]
    fn extended_with_outou_types_appends_nothing_when_already_present() {
        let legend = Legend::default_legend();
        let extended = legend.extended_with_outou_types();
        assert_eq!(extended.token_types, legend.token_types);
    }
}
