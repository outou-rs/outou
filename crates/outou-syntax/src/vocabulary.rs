//! Shared table for translating backend vocabulary out of diagnostic
//! messages, wherever one is rendered to a user: `outou check`'s own
//! semantic/backend diagnostic layers (`outou-cli`'s UI test harness,
//! issue #11) and `outou-lsp`'s `translate_diagnostic`
//! (`docs/backend-leakage.md` row 24), which re-exports this module's
//! items rather than keeping its own copy.
//!
//! `AGENTS.md`: "Backend vocabulary (`rsx! macro`, `PropsBuilder`,
//! `dioxus_rsx`, `GeneratedNode`, and similar) in any user-facing
//! diagnostic is a Phase 0 failure. Translate or hide it." This table is
//! deliberately small: known messages get a specific rewrite, anything
//! else that still mentions a backend marker gets a generic message.
//!
//! This module previously lived only in `outou-lsp/src/translate.rs`;
//! moved here (issue #11) so `outou-cli`'s UI/leak tests can reuse the
//! exact same table instead of duplicating it — "there is one compiler"
//! extends to "there is one translation table".

/// Substrings that mark a message as containing backend vocabulary. Kept
/// in one place so `docs/backend-leakage.md` and every consumer of this
/// module stay in sync.
pub const BACKEND_MARKERS: &[&str] = &[
    "rsx!",
    "PropsBuilder",
    "dioxus_rsx",
    "GeneratedNode",
    "IntoDynNode",
    "dioxus_core",
    "dioxus_elements",
    "dioxus_signals",
    "dioxus_html",
    "dioxus::",
    " dioxus ",
    "VNode",
    "RenderError",
    "Properties",
    "__template",
    "_completions",
    "typed_builder",
    "::outou::__private::",
    // Issue #9 Gate 3 review, H2: an HTML element's rustdoc routinely
    // carries a "## Usage in rsx" section with a `ChildComponent {}`
    // example in backend brace syntax; neither substring was covered by
    // any marker above, so a hover whose *only* other content was the
    // element's bare module path (already stripped as a path prefix,
    // never a whole-line marker on its own) survived sanitization intact
    // for some elements (`<p>`) while an unrelated element (`<main>`)
    // happened to have no such section and was already suppressed.
    "Usage in rsx",
    "ChildComponent",
];

/// Whether `text` contains any [`BACKEND_MARKERS`] substring. Shared by
/// every outbound payload this table protects — `outou-lsp` diagnostics,
/// completion items and hover contents, and `outou-cli`'s UI test
/// harness's own semantic/backend diagnostic layers.
///
/// Deliberately does *not* also flag a `__`-prefixed or `…Props`-suffixed
/// name: that shape-based guess is cheap but not reliable — an ordinary
/// user type coincidentally named `MyProps` is not backend vocabulary at
/// all (issue #9 Gate 3 review, H3). `outou-lsp::translate` keeps its own
/// `looks_like_generated_name`/`contains_component_props_marker` for that
/// LSP-specific, completion/hover-only heuristic; it is not part of this
/// shared table.
pub fn contains_backend_marker(text: &str) -> bool {
    BACKEND_MARKERS.iter().any(|marker| text.contains(marker))
}

/// One specific, known rewrite: a substring to look for and the
/// Outou-vocabulary replacement text to use instead of the generic
/// fallback.
const KNOWN_REWRITES: &[(&str, &str)] = &[
    (
        "PropsBuilder",
        "this component is missing a required property",
    ),
    (
        "IntoDynNode",
        "this value cannot be used as element content here",
    ),
];

/// Rewrites `message` if it contains backend vocabulary, per the table
/// above. Returns the message unchanged when no marker is present — the
/// overwhelming majority of rustc/rust-analyzer diagnostics for ordinary
/// Rust code, which need no translation.
pub fn translate_message(message: &str) -> String {
    if !contains_backend_marker(message) {
        return message.to_string();
    }
    for (marker, replacement) in KNOWN_REWRITES {
        if message.contains(marker) {
            return (*replacement).to_string();
        }
    }
    "the backend rejected this element; see the generated code".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_rustc_message_is_untouched() {
        assert_eq!(
            translate_message("mismatched types: expected `String`, found `u32`"),
            "mismatched types: expected `String`, found `u32`"
        );
    }

    #[test]
    fn a_props_builder_message_gets_the_specific_rewrite() {
        let msg = "no method named `build` found for struct `GreetingPropsBuilder`";
        assert_eq!(
            translate_message(msg),
            "this component is missing a required property"
        );
    }

    #[test]
    fn an_into_dyn_node_message_gets_the_specific_rewrite() {
        let msg = "the trait bound `i32: IntoDynNode<_>` is not satisfied";
        assert_eq!(
            translate_message(msg),
            "this value cannot be used as element content here"
        );
    }

    #[test]
    fn an_unknown_backend_marker_gets_the_generic_message() {
        let msg = "this error originates in the derive macro `Props`, dioxus_core::internals";
        assert_eq!(
            translate_message(msg),
            "the backend rejected this element; see the generated code"
        );
    }

    #[test]
    fn contains_backend_marker_flags_the_usage_in_rsx_section() {
        assert!(contains_backend_marker("## Usage in rsx"));
        assert!(contains_backend_marker(
            "main { ChildComponent {} {raw_expression} }"
        ));
    }

    #[test]
    fn contains_backend_marker_no_longer_flags_a_bare_props_suffixed_name() {
        // H3 (issue #9 Gate 3 review): an ordinary user type coincidentally
        // named `MyProps` is not backend vocabulary.
        assert!(!contains_backend_marker("MyProps"));
        assert!(!contains_backend_marker("__user_private_helper"));
    }
}
