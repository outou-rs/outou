//! Translates backend vocabulary out of diagnostic messages rust-analyzer
//! (or its `cargo check` flycheck) produced for generated code.
//!
//! `AGENTS.md`: "Backend vocabulary (`rsx! macro`, `PropsBuilder`,
//! `dioxus_rsx`, `GeneratedNode`, and similar) in any user-facing
//! diagnostic is a Phase 0 failure. Translate or hide it." This module is
//! the translation table `docs/backend-leakage.md` row 24 documents, and
//! is deliberately small: known messages get a specific rewrite, anything
//! else that still mentions a backend marker gets a generic message. The
//! original text is never preserved anywhere in the outbound payload
//! (see [`translate_diagnostic`]'s doc comment for why `data` doesn't
//! either).

use lsp_types::Diagnostic;

/// Substrings that mark a message as containing backend vocabulary. Kept
/// in one place so `docs/backend-leakage.md` and this module stay in
/// sync.
const BACKEND_MARKERS: &[&str] = &[
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
/// every outbound payload this server sanitizes — diagnostics
/// (`translate_message`), completion items
/// (`crate::response::map_completion_response`) and hover contents
/// (`crate::response::sanitize_hover`) — so a marker added here protects
/// all three at once (issue #9 Gate 3 review, M4/HIGH-9).
///
/// Deliberately does *not* also flag a `__`-prefixed or `…Props`-suffixed
/// name: that shape-based guess is cheap but not reliable — an ordinary
/// user type coincidentally named `MyProps` is not backend vocabulary at
/// all (issue #9 Gate 3 review, H3, confirmed live: hovering a `MyProps`
/// parameter returned nothing, every type line stripped). See
/// [`looks_like_generated_name`] (safe only for a completion label, where
/// a false positive costs one dropped suggestion) and
/// [`contains_component_props_marker`] (safe for hover text too, since it
/// is anchored to names this plan's own `#[component]` functions actually
/// generate a `Props`/`PropsBuilder` type for).
pub fn contains_backend_marker(text: &str) -> bool {
    BACKEND_MARKERS.iter().any(|marker| text.contains(marker))
}

/// A cheap, shape-based guess that `text` names something this backend
/// generated: a `__`-prefixed internal, or a name ending in `Props` (the
/// `<Name>Props`/`<Name>PropsBuilder` family this backend generates for
/// every component). Not anchored to any real component name, so it can
/// false-positive on an ordinary user type (`MyProps`) — safe enough to
/// drop one *completion label* among many (`crate::response::is_backend_leak`),
/// never safe enough to blank a *hover* (issue #9 Gate 3 review, H3): use
/// [`contains_component_props_marker`] there instead.
pub fn looks_like_generated_name(text: &str) -> bool {
    text.starts_with("__") || (text.ends_with("Props") && text.len() > "Props".len())
}

/// Whether `text` names a real backend-generated `Props`/`PropsBuilder`
/// type for one of `component_names` (issue #9 Gate 3 review, H3):
/// `<Name>PropsBuilder...` always contains `<Name>Props` as a prefix, so
/// checking for that one substring covers both forms. Narrower than
/// [`looks_like_generated_name`] and safe to use for hover text, where a
/// false positive on an ordinary user type would blank a legitimate
/// hover rather than merely drop one completion suggestion among many.
pub fn contains_component_props_marker(text: &str, component_names: &[String]) -> bool {
    component_names
        .iter()
        .any(|name| text.contains(&format!("{name}Props")))
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
/// overwhelming majority of rust-analyzer/rustc diagnostics for ordinary
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

/// Applies [`translate_message`] to one diagnostic, preserving its
/// `range`, `severity` and `code` (the architecture note: "Every mapped
/// diagnostic must keep its severity and code").
///
/// `data` is always cleared, never populated with the original message
/// (issue #9 Gate 3 review, M4/HIGH-9(a)): a diagnostic's `data` field is
/// forwarded to the client verbatim by every conformant editor (it is
/// part of the public `Diagnostic` shape, not a server-internal
/// scratchpad), so stashing backend vocabulary there was itself a leak —
/// exactly as visible to a user's editor as the message text it was
/// trying to hide. A developer who wants the original rustc/rust-analyzer
/// text can still get it from `outou-lsp`'s own stderr log.
pub fn translate_diagnostic(mut diagnostic: Diagnostic) -> Diagnostic {
    diagnostic.message = translate_message(&diagnostic.message);
    diagnostic.data = None;
    diagnostic
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{DiagnosticSeverity, NumberOrString, Position, Range};

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
    fn an_unknown_backend_marker_gets_the_generic_message() {
        let msg = "this error originates in the derive macro `Props`, dioxus_core::internals";
        assert_eq!(
            translate_message(msg),
            "the backend rejected this element; see the generated code"
        );
    }

    #[test]
    fn translate_diagnostic_keeps_severity_and_code_and_drops_data() {
        let diagnostic = Diagnostic {
            range: Range::new(Position::new(0, 0), Position::new(0, 1)),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("E0308".to_string())),
            source: Some("rustc".to_string()),
            message: "rsx! macro expansion failed".to_string(),
            data: Some(serde_json::json!({ "outouOriginalMessage": "leaked" })),
            ..Default::default()
        };
        let translated = translate_diagnostic(diagnostic.clone());
        assert_eq!(translated.severity, diagnostic.severity);
        assert_eq!(translated.code, diagnostic.code);
        assert_ne!(translated.message, diagnostic.message);
        assert!(
            translated.data.is_none(),
            "`data` must never carry the original backend-vocabulary text (M4/HIGH-9(a))"
        );
    }

    #[test]
    fn translate_diagnostic_leaves_an_untranslated_message_with_no_data() {
        let diagnostic = Diagnostic {
            range: Range::new(Position::new(0, 0), Position::new(0, 1)),
            message: "unused variable: `x`".to_string(),
            ..Default::default()
        };
        let translated = translate_diagnostic(diagnostic);
        assert!(translated.data.is_none());
    }

    /// H2: a hover's "Usage in rsx" section (with its `ChildComponent {}`
    /// brace-syntax example) must be recognized as backend vocabulary even
    /// though it names no `dioxus_*` path directly.
    #[test]
    fn contains_backend_marker_flags_the_usage_in_rsx_section() {
        assert!(contains_backend_marker("## Usage in rsx"));
        assert!(contains_backend_marker(
            "main { ChildComponent {} {raw_expression} }"
        ));
    }

    /// H3: the bare shape-based heuristic still flags a `…Props`-suffixed
    /// or `__`-prefixed name (used only for completion labels).
    #[test]
    fn looks_like_generated_name_flags_the_bare_shapes() {
        assert!(looks_like_generated_name("GreetingProps"));
        assert!(looks_like_generated_name("__TEMPLATE_ROOTS"));
        assert!(!looks_like_generated_name("unwrap"));
        assert!(
            !looks_like_generated_name("Props"),
            "must be longer than the bare suffix itself"
        );
    }

    /// H3: `contains_backend_marker` itself must no longer flag an
    /// ordinary user type just because its name ends in `Props` — that
    /// heuristic broke hover for `MyProps` (a plain user struct, not
    /// anything this backend generated).
    #[test]
    fn contains_backend_marker_no_longer_flags_a_bare_props_suffixed_name() {
        assert!(!contains_backend_marker("MyProps"));
        assert!(!contains_backend_marker("__user_private_helper"));
    }

    /// H3: the component-aware marker only flags a name derived from a
    /// real `#[component]` function, never an unrelated `…Props` type.
    #[test]
    fn contains_component_props_marker_is_anchored_to_real_component_names() {
        let names = vec!["Greeting".to_string()];
        assert!(contains_component_props_marker("GreetingProps", &names));
        assert!(contains_component_props_marker(
            "GreetingPropsBuilder<((String,),)>",
            &names
        ));
        assert!(!contains_component_props_marker("MyProps", &names));
    }
}
