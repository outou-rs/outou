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

/// [`BACKEND_MARKERS`] entries excluded from [`translate_message`]'s
/// stronger guarantee (issue #12 corpus review, F12/MEDIUM): each is an
/// ordinary English word or a name a real user could plausibly give their
/// own type, so a message that only happens to contain one of these three
/// must be left untouched rather than silently replaced by "the backend
/// rejected this element" — which would destroy an unrelated, legitimate
/// diagnostic (F12's own example: a user's own type named `ChildComponent`
/// failing to type-check for a reason that has nothing to do with this
/// backend).
///
/// [`contains_backend_marker`] (built from the full, broader
/// `BACKEND_MARKERS`) is unchanged and still uses all of these: dropping
/// one ambiguous completion candidate, or blanking a hover that happens to
/// mention one of these three words, is an acceptable trade-off in the
/// LSP's suppression-only paths (`outou-lsp/src/response.rs`'s
/// completion/hover filtering — `outou-lsp/src/translate.rs`'s own H3 doc
/// comment already draws exactly this "safe to over-suppress a
/// completion/hover, never safe to destroy a diagnostic message" line).
/// Only [`translate_message`]'s full-message replacement is narrowed.
pub const AMBIGUOUS_MARKERS: &[&str] = &["Properties", "ChildComponent", "Usage in rsx"];

/// Whether `text` contains a [`BACKEND_MARKERS`] substring reliable enough
/// to justify [`translate_message`] replacing the whole message: every
/// marker except [`AMBIGUOUS_MARKERS`], computed from `BACKEND_MARKERS`
/// itself (rather than kept as a second, hand-maintained list) so the two
/// tables cannot drift apart.
fn contains_reliable_marker(text: &str) -> bool {
    BACKEND_MARKERS
        .iter()
        .filter(|marker| !AMBIGUOUS_MARKERS.contains(marker))
        .any(|marker| text.contains(marker))
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

/// Rewrites `message` if it contains backend vocabulary reliable enough to
/// act on ([`contains_reliable_marker`], not the broader
/// [`contains_backend_marker`] — F12), per the table above. Returns the
/// message unchanged when no
/// reliable marker is present — the overwhelming majority of
/// rustc/rust-analyzer diagnostics for ordinary Rust code, which need no
/// translation, plus the deliberately excluded ambiguous-marker case.
pub fn translate_message(message: &str) -> String {
    if !contains_reliable_marker(message) {
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

    /// F12 (issue #12 corpus review, MEDIUM): a message whose only backend
    /// marker is one of the three ambiguous, English-word-shaped ones
    /// (`Properties`, `ChildComponent`, `Usage in rsx`) must be left
    /// completely untouched by `translate_message`, not silently replaced
    /// by the generic fallback — that would destroy an unrelated
    /// legitimate diagnostic (a user's own type happens to be named
    /// `ChildComponent`, or a message happens to use the ordinary English
    /// word "properties"/"Properties").
    #[test]
    fn an_ambiguous_marker_alone_is_left_completely_untouched() {
        let msg = "mismatched types: expected `Properties`, found `u32`";
        assert_eq!(translate_message(msg), msg);
        let msg = "cannot find type `ChildComponent` in this scope";
        assert_eq!(translate_message(msg), msg);
        let msg = "## Usage in rsx";
        assert_eq!(translate_message(msg), msg);
    }

    /// The same three ambiguous markers still count for
    /// `contains_backend_marker` (`BACKEND_MARKERS` is the broader,
    /// suppression-only list — `outou-lsp/src/response.rs`'s
    /// completion/hover filtering still needs them), even though they are
    /// excluded from `translate_message`'s stronger guarantee above.
    #[test]
    fn contains_backend_marker_still_flags_the_three_ambiguous_markers() {
        assert!(contains_backend_marker("Properties"));
        assert!(contains_backend_marker("ChildComponent"));
        assert!(contains_backend_marker("Usage in rsx"));
    }

    /// An ambiguous marker alongside a reliable one still gets translated
    /// (the reliable marker alone is enough to justify it).
    #[test]
    fn an_ambiguous_marker_alongside_a_reliable_one_still_translates() {
        let msg = "the trait bound `GreetingProps: dioxus_core::Properties` is not satisfied";
        assert_eq!(
            translate_message(msg),
            "the backend rejected this element; see the generated code"
        );
    }
}
