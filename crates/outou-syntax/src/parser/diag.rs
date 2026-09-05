//! The Outou diagnostic message catalogue (grammar §9, §10), verbatim.
//!
//! Every message the parser can produce is built here, in one place, so
//! that the exact wording required by `tests/fixtures/{incomplete,
//! diagnostics}/*.expected` is never duplicated (and never drifts) across
//! the parsing code that decides *when* to emit it.

/// `unexpected end of file inside tag `<{name}>`, expected an attribute or `>``
pub fn eof_inside_tag(name: &str) -> String {
    format!("unexpected end of file inside tag `<{name}>`, expected an attribute or `>`")
}

/// `unexpected `}` inside tag `<{name}>`, expected an attribute or `>``
pub fn rbrace_inside_tag(name: &str) -> String {
    format!("unexpected `}}` inside tag `<{name}>`, expected an attribute or `>`")
}

/// A stray, uncatalogued byte inside a tag (grammar §4.1's `<div!>` row: "the
/// `!` is recovered as an error node"). Not one of the exact messages
/// grammar §9 mandates verbatim; phrased analogously to
/// [`eof_inside_tag`]/[`rbrace_inside_tag`] since no other wording is
/// specified for this case.
pub fn unexpected_inside_tag(what: &str, name: &str) -> String {
    format!("unexpected `{what}` inside tag `<{name}>`, expected an attribute or `>`")
}

/// `missing closing tag `</{name}>``
pub fn missing_closing_tag(name: &str) -> String {
    format!("missing closing tag `</{name}>`")
}

/// `closing tag `</{found}>` does not match opening tag `<{expected}>``
pub fn mismatched_closing_tag(found: &str, expected: &str) -> String {
    format!("closing tag `</{found}>` does not match opening tag `<{expected}>`")
}

/// `closing tag `</{name}>` has no matching opening tag`
pub fn stray_closing_tag(name: &str) -> String {
    format!("closing tag `</{name}>` has no matching opening tag")
}

/// `unexpected end of file inside closing tag, expected `>``
pub fn eof_inside_closing_tag() -> String {
    "unexpected end of file inside closing tag, expected `>`".to_string()
}

/// `unexpected `}` inside closing tag, expected `>`` — the `}` analogue of
/// [`eof_inside_closing_tag`], per grammar §2.2's general terminator rule
/// (only the EOF and `<` interruptions are given verbatim examples in the
/// table; this follows the same pattern).
pub fn rbrace_inside_closing_tag() -> String {
    "unexpected `}` inside closing tag, expected `>`".to_string()
}

/// `unexpected `<` inside closing tag, expected `>``
pub fn lt_inside_closing_tag() -> String {
    "unexpected `<` inside closing tag, expected `>`".to_string()
}

/// `unterminated string in attribute value`
pub fn unterminated_string_in_attribute_value() -> String {
    "unterminated string in attribute value".to_string()
}

/// `unexpected end of file, expected `}` to close the value of attribute `{name}``
pub fn eof_in_attribute_value_island(name: &str) -> String {
    format!("unexpected end of file, expected `}}` to close the value of attribute `{name}`")
}

/// `expected an expression for the value of attribute `{name}``
pub fn empty_attribute_value_island(name: &str) -> String {
    format!("expected an expression for the value of attribute `{name}`")
}

/// `unexpected end of file, expected `}` to close this expression`
pub fn eof_in_island() -> String {
    "unexpected end of file, expected `}` to close this expression".to_string()
}

/// `fragments are not supported in Phase 0`
pub fn fragments_not_supported() -> String {
    "fragments are not supported in Phase 0".to_string()
}

/// `dotted tag names are not supported in Phase 0`
pub fn dotted_tag_name_not_supported() -> String {
    "dotted tag names are not supported in Phase 0".to_string()
}

/// `namespaced names are not supported in Phase 0`
pub fn namespaced_name_not_supported() -> String {
    "namespaced names are not supported in Phase 0".to_string()
}

/// `spread attributes are not supported in Phase 0`
pub fn spread_attribute_not_supported() -> String {
    "spread attributes are not supported in Phase 0".to_string()
}

/// `generic arguments on a tag are not supported in Phase 0`
pub fn generic_arguments_not_supported() -> String {
    "generic arguments on a tag are not supported in Phase 0".to_string()
}

/// `a JSX expression cannot be followed by `.`, `?`, `(` or `[`; parenthesize it`
pub fn postfix_on_jsx_not_supported() -> String {
    "a JSX expression cannot be followed by `.`, `?`, `(` or `[`; parenthesize it".to_string()
}

/// `duplicate attribute `{name}` on this tag`
pub fn duplicate_attribute(name: &str) -> String {
    format!("duplicate attribute `{name}` on this tag")
}

/// `` `Self` is not a valid component name ``
pub fn self_not_a_valid_component_name() -> String {
    "`Self` is not a valid component name".to_string()
}

/// `attribute values must be double-quoted strings or `{…}` expressions`
pub fn single_quoted_attribute_value() -> String {
    "attribute values must be double-quoted strings or `{…}` expressions".to_string()
}

/// `attribute values must be double-quoted strings or `{…}` expressions`
///
/// The wording is identical to [`single_quoted_attribute_value`] — the
/// grammar gives one message for every shape of invalid attribute value
/// (missing, unquoted, a char or numeric literal, …), not one per shape —
/// but this is its own function, matching every other "what went wrong"
/// diagnostic in this module having its own name, and leaving room for a
/// more specific, name-mentioning message later without another call-site
/// sweep.
pub fn invalid_attribute_value() -> String {
    "attribute values must be double-quoted strings or `{…}` expressions".to_string()
}

/// `` unexpected `}` here; write `{"}"}` to include a literal `}` in text ``
pub fn stray_rbrace_in_text() -> String {
    "unexpected `}` here; write `{\"}\"}` to include a literal `}` in text".to_string()
}

/// The maximum JSX element nesting depth and inline-`mod` nesting depth
/// Outou supports before it stops recursing and diagnoses instead
/// (decision D4, grammar §9). Measured to give roughly 3x margin over the
/// deepest nesting observed to overflow a 2 MiB thread stack in a debug
/// build (500 JSX levels, 1000 inline-module levels) while comfortably
/// exceeding any real UI's nesting.
pub const MAX_NESTING: u32 = 128;

/// `this element is nested too deeply (Outou supports at most 128 levels)`
pub fn jsx_nested_too_deeply() -> String {
    format!("this element is nested too deeply (Outou supports at most {MAX_NESTING} levels)")
}

/// `modules are nested too deeply (Outou supports at most 128 levels)`
pub fn modules_nested_too_deeply() -> String {
    format!("modules are nested too deeply (Outou supports at most {MAX_NESTING} levels)")
}
