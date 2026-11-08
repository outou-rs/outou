//! Translates backend vocabulary out of diagnostic messages rust-analyzer
//! (or its `cargo check` flycheck) produced for generated code.
//!
//! `AGENTS.md`: "Backend vocabulary (`rsx! macro`, `PropsBuilder`,
//! `dioxus_rsx`, `GeneratedNode`, and similar) in any user-facing
//! diagnostic is a Phase 0 failure. Translate or hide it." This module is
//! the translation table `docs/backend-leakage.md` row 24 documents.
//!
//! The table itself (`BACKEND_MARKERS`, `contains_backend_marker`,
//! `translate_message`) moved to `outou_syntax::vocabulary` (issue #11)
//! so `outou-cli`'s UI/leak test harness can reuse the exact same
//! translations instead of duplicating them; this module re-exports them
//! under their original names so every existing call site here keeps
//! compiling unchanged. It is deliberately small: known messages get a
//! specific rewrite, anything else that still mentions a backend marker
//! gets a generic message. The original text is never preserved anywhere
//! in the outbound payload (see [`translate_diagnostic`]'s doc comment
//! for why `data` doesn't either).

use lsp_types::Diagnostic;

pub use outou_syntax::vocabulary::{contains_backend_marker, translate_message};

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

/// Crates that belong to the backend and its builder, by exact crate name
/// (every crate whose name starts with `dioxus` is covered separately, see
/// [`is_backend_crate`]).
///
/// The list is the part of `dioxus`'s dependency closure that exists for
/// Dioxus and that a user would not depend on or want to jump into:
/// `generational-box` (its signals' storage), `sledgehammer_*` (its DOM
/// interpreter), `warnings` (its memo helper), `subsecond*` (hot patching),
/// `manganis*` and `lazy-js-bundle` (its assets), `longest-increasing-
/// subsequence` (its diffing) and `typed-builder*` (the props builder).
/// `const_format*` is here because the interpreter's generated code reaches
/// it. General-purpose crates that a user may legitimately depend on and
/// jump into (`slab`, `slotmap`, `serde`, `tracing`, `futures-*`,
/// `keyboard-types`, `konst`, ...) are deliberately left out: dropping a
/// real definition is worse than showing a dependency.
///
/// To regenerate when the Dioxus version changes: run
/// `cargo tree -p dioxus --prefix none -e normal --all-features | sort -u`
/// in a workspace that depends on `outou` (for instance
/// `examples/dogfood-board`), and add every crate that is specific to
/// Dioxus. No test compares it with `cargo metadata`: that would tie the unit
/// tests to an example workspace's lockfile and a populated cargo registry.
const BACKEND_CRATES: &[&str] = &[
    "const_format",
    "const_format_proc_macros",
    "generational-box",
    "lazy-js-bundle",
    "longest-increasing-subsequence",
    "manganis",
    "manganis-core",
    "manganis-macro",
    "sledgehammer_bindgen",
    "sledgehammer_bindgen_macro",
    "sledgehammer_utils",
    "subsecond",
    "subsecond-types",
    "typed-builder",
    "typed-builder-macro",
    "warnings",
    "warnings-macro",
];

/// Whether `name` (a crate name, or a crate name with a version or a hash
/// after its last `-`) is one of the backend's crates.
fn is_backend_crate(dir: &str) -> bool {
    let without_suffix = dir.rsplit_once('-').map(|(name, _)| name);
    [Some(dir), without_suffix]
        .into_iter()
        .flatten()
        .any(|name| name.starts_with("dioxus") || BACKEND_CRATES.contains(&name))
}

/// Whether `uri` is a source file of the backend's own crates in the cargo
/// registry, a git checkout or a `cargo vendor` directory, e.g.
/// `.../registry/src/index.crates.io-xxx/dioxus-html-0.7.10/src/elements.rs`.
///
/// rust-analyzer answers definition and references with the file it
/// expanded a name from, and for a name that only exists in the expansion
/// (an attribute, a component call) that file is the backend's. A user
/// crate never lists the backend (ADR 0010), so such a location can only be
/// a leak and is dropped (`docs/backend-leakage.md` row 62). Only these
/// three layouts count: a workspace directory that happens to be called
/// `dioxus-playground` is not touched, nor is a `[patch]` checkout of the
/// backend at an arbitrary path (a known gap, ledger row 64).
pub fn is_backend_source_uri(uri: &str) -> bool {
    let crate_dir = if let Some((_, rest)) = uri.split_once("/registry/src/") {
        // `<registry index directory>/<crate directory>/...`
        rest.split('/').nth(1)
    } else if let Some((_, rest)) = uri.split_once("/git/checkouts/") {
        rest.split('/').next()
    } else if let Some((_, rest)) = uri.rsplit_once("/vendor/") {
        // `cargo vendor`: `vendor/<crate>/...`
        rest.split('/').next()
    } else {
        None
    };
    crate_dir.is_some_and(is_backend_crate)
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

    fn message_of(text: &str) -> String {
        translate_diagnostic(Diagnostic {
            range: Range::new(Position::new(0, 0), Position::new(0, 1)),
            message: text.to_string(),
            ..Default::default()
        })
        .message
    }

    #[test]
    fn a_state_that_is_not_thread_safe_is_worded_as_a_state() {
        assert_eq!(
            message_of(
                "`RefCell<StorageEntry<RefCellStorageEntryData>>` cannot be shared between threads safely"
            ),
            "a `State` cannot be sent to or shared with another thread"
        );
    }

    #[test]
    fn a_user_type_named_storage_entry_keeps_its_message() {
        let msg = "no method named `lenn` found for reference `&StorageEntry` in the current scope";
        assert_eq!(message_of(msg), msg);
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

    #[test]
    fn is_backend_source_uri_matches_the_rest_of_the_backend_closure_and_the_vendor_layout() {
        let registry = "file:///Users/me/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f";
        for krate in [
            "generational-box-0.7.3",
            "sledgehammer_bindgen-0.6.0",
            "sledgehammer_utils-0.3.1",
            "const_format-0.2.34",
            "warnings-0.2.1",
            "manganis-0.7.10",
            "lazy-js-bundle-0.7.0",
        ] {
            assert!(
                is_backend_source_uri(&format!("{registry}/{krate}/src/lib.rs")),
                "{krate}"
            );
        }
        for krate in [
            "slab-0.4.9",
            "slotmap-1.0.7",
            "konst-0.3.16",
            "warnings_ext-1.0.0",
        ] {
            assert!(
                !is_backend_source_uri(&format!("{registry}/{krate}/src/lib.rs")),
                "{krate}"
            );
        }
        assert!(is_backend_source_uri(
            "file:///home/me/app/vendor/dioxus-core/src/lib.rs"
        ));
        assert!(is_backend_source_uri(
            "file:///home/me/app/vendor/generational-box/src/lib.rs"
        ));
        assert!(!is_backend_source_uri(
            "file:///home/me/app/vendor/slab/src/lib.rs"
        ));
        // A `[patch]` checkout at an arbitrary path is a known gap.
        assert!(!is_backend_source_uri(
            "file:///home/me/src/dioxus/packages/core/src/lib.rs"
        ));
    }

    #[test]
    fn is_backend_source_uri_handles_a_windows_style_file_uri() {
        assert!(is_backend_source_uri(
            "file:///c%3A/Users/x/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/dioxus-html-0.7.10/src/elements.rs"
        ));
        assert!(!is_backend_source_uri(
            "file:///c%3A/Users/x/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/serde-1.0.200/src/lib.rs"
        ));
    }

    #[test]
    fn is_backend_source_uri_matches_only_backend_crates_in_a_registry_or_checkout() {
        let registry = "file:///Users/me/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f";
        assert!(is_backend_source_uri(&format!(
            "{registry}/dioxus-core-macro-0.7.10/src/lib.rs"
        )));
        assert!(is_backend_source_uri(&format!(
            "{registry}/dioxus-html-0.7.10/src/elements.rs"
        )));
        assert!(is_backend_source_uri(&format!(
            "{registry}/typed-builder-0.20.0/src/lib.rs"
        )));
        assert!(is_backend_source_uri(
            "file:///opt/cargo/git/checkouts/dioxus-ab12cd/f00/packages/html/src/lib.rs"
        ));
        // Other dependencies are the user's own business.
        assert!(!is_backend_source_uri(&format!(
            "{registry}/serde-1.0.200/src/lib.rs"
        )));
        // A workspace that merely has the name in its path is not a registry.
        assert!(!is_backend_source_uri(
            "file:///home/me/dioxus-playground/app/src/main.rsx"
        ));
        assert!(!is_backend_source_uri(
            "file:///home/me/registry/src/dioxus-notes/src/main.rsx"
        ));
    }

    /// H3: the bare shape-based heuristic still flags a `…Props`-suffixed
    /// or `__`-prefixed name (used only for completion labels).
    #[test]
    fn looks_like_generated_name_flags_the_bare_shapes() {
        assert!(looks_like_generated_name("GreetingProps"));
        assert!(looks_like_generated_name("__TEMPLATE_ROOTS"));
        // Issue #26: the synthesized binding of a `key` stays out of completion.
        assert!(looks_like_generated_name("__outou_key_0"));
        assert!(!looks_like_generated_name("unwrap"));
        assert!(
            !looks_like_generated_name("Props"),
            "must be longer than the bare suffix itself"
        );
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
