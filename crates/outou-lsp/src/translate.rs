//! Translates backend vocabulary out of diagnostic messages rust-analyzer
//! (or its `cargo check` flycheck) produced for generated code.
//!
//! `AGENTS.md`: "Backend vocabulary (`rsx! macro`, `PropsBuilder`,
//! `dioxus_rsx`, `GeneratedNode`, and similar) in any user-facing
//! diagnostic is a Phase 0 failure. Translate or hide it." This module is
//! the translation table `docs/backend-leakage.md` row 24 documents, and
//! is deliberately small: known messages get a specific rewrite, anything
//! else that still mentions a backend marker gets a generic message with
//! the original text preserved in the diagnostic's `data` field for a
//! developer who wants it, never shown by default.

use lsp_types::Diagnostic;
use serde_json::json;

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
    "dioxus::",
    " dioxus ",
];

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
    if !BACKEND_MARKERS
        .iter()
        .any(|marker| message.contains(marker))
    {
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
/// diagnostic must keep its severity and code") and, only when the
/// message was rewritten, stashing the original text in `data` behind the
/// `outouOriginalMessage` key rather than showing it by default.
pub fn translate_diagnostic(mut diagnostic: Diagnostic) -> Diagnostic {
    let translated = translate_message(&diagnostic.message);
    if translated != diagnostic.message {
        diagnostic.data = Some(json!({ "outouOriginalMessage": diagnostic.message }));
        diagnostic.message = translated;
    }
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
    fn translate_diagnostic_keeps_severity_and_code_and_hides_the_original() {
        let diagnostic = Diagnostic {
            range: Range::new(Position::new(0, 0), Position::new(0, 1)),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("E0308".to_string())),
            source: Some("rustc".to_string()),
            message: "rsx! macro expansion failed".to_string(),
            ..Default::default()
        };
        let translated = translate_diagnostic(diagnostic.clone());
        assert_eq!(translated.severity, diagnostic.severity);
        assert_eq!(translated.code, diagnostic.code);
        assert_ne!(translated.message, diagnostic.message);
        assert!(translated.data.is_some());
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
}
