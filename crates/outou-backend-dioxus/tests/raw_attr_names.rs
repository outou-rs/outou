//! Issue #31: an attribute written as a raw identifier (`r#type`, `r#key`,
//! `r#class`) names the same attribute as the plain spelling, so `r#key` is
//! the `key` attribute (typed, evaluated first) and never bypasses that rule.

use outou_backend_dioxus::DioxusBackend;
use outou_codegen::{Backend, GenerateOptions, Mode};
use outou_sourcemap::Uri;

fn generate(source: &str) -> String {
    let parsed = outou_syntax::parse(source);
    DioxusBackend
        .generate(
            &parsed,
            source,
            Mode::Strict,
            &GenerateOptions::new(
                Uri::new("file:///gen/main.rs"),
                Uri::new("file:///src/main.rsx"),
            ),
        )
        .expect("generation succeeds")
        .rust
}

fn body(attributes: &str) -> String {
    generate(&format!(
        "fn f() -> Element {{\n    <div {attributes} />\n}}\n"
    ))
}

#[test]
fn a_raw_key_is_the_typed_key() {
    let raw = body("r#key=\"a\"");
    let plain = body("key=\"a\"");
    assert_eq!(raw, plain);
    assert!(raw.contains("::key(&("), "{raw}");
    assert!(!raw.contains("r#key"), "{raw}");
}

#[test]
fn a_raw_ordinary_name_is_lowered_as_the_plain_name() {
    assert_eq!(body("r#class=\"c\""), body("class=\"c\""));
    assert!(body("r#class=\"c\"").contains("class: \"c\""));
    assert_eq!(body("r#type=\"t\""), body("type=\"t\""));
    assert!(body("r#type=\"t\"").contains("r#type: \"t\""));
    assert_eq!(body("r#for=\"f\""), body("for=\"f\""));
}

#[test]
fn a_raw_name_rust_refuses_as_a_raw_identifier_and_hyphenated_names_use_a_string_key() {
    assert!(body("r#self=\"s\"").contains("\"self\": \"s\""));
    assert!(body("data-r#id=\"x\"").contains("\"data-id\": \"x\""));
}

#[test]
fn a_raw_select_value_is_the_select_value() {
    let lowered = generate(
        "fn f() -> Element {\n    <select r#value={v}><option value=\"a\">a</option></select>\n}\n",
    );
    assert!(lowered.contains("select_value(&(v))"), "{lowered}");
    assert!(lowered.contains("option_selected("), "{lowered}");
    assert!(!lowered.contains("r#value"), "{lowered}");
}
