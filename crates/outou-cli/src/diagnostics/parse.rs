//! Reading `cargo --message-format=json*` lines.

use std::path::PathBuf;

use serde_json::Value;

/// One span of a rustc diagnostic, reduced to what translation needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSpan {
    /// File name exactly as rustc reports it (workspace-relative for
    /// workspace members, absolute otherwise).
    pub file_name: String,
    /// Byte range inside that file.
    pub byte_start: u32,
    /// Exclusive end of the byte range.
    pub byte_end: u32,
    /// 1-based first line.
    pub line_start: usize,
    /// 1-based last line.
    pub line_end: usize,
    /// 1-based column (in characters) of the start.
    pub column_start: usize,
    /// 1-based column (in characters) of the exclusive end, on the last line.
    pub column_end: usize,
    /// Whether this is the diagnostic's primary span.
    pub is_primary: bool,
    /// The label drawn under the span.
    pub label: Option<String>,
    /// Suggested replacement text (clippy's `help: try`).
    pub suggested_replacement: Option<String>,
    /// Whether the span comes from a macro expansion (`expansion` is
    /// non-null): such spans point at the macro, which is the backend's.
    pub has_expansion: bool,
    /// The text of the first source line, as rustc saw it.
    pub line_text: Option<String>,
}

/// A `note:` / `help:` attached to a diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawChild {
    /// `"note"`, `"help"`, ...
    pub level: String,
    /// Message text.
    pub message: String,
    /// Spans of the child (a suggestion carries one).
    pub spans: Vec<RawSpan>,
}

/// A `compiler-message` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerMessage {
    /// The `Cargo.toml` of the package the message belongs to; file
    /// names are resolved against it.
    pub manifest_path: PathBuf,
    /// `"error"`, `"warning"`, `"failure-note"`, ...
    pub level: String,
    /// Error code or lint name (`E0308`, `clippy::len_zero`).
    pub code: Option<String>,
    /// The message text.
    pub message: String,
    /// All spans.
    pub spans: Vec<RawSpan>,
    /// `note:` / `help:` children.
    pub children: Vec<RawChild>,
    /// What rustc rendered for a terminal (ANSI when asked for).
    pub rendered: Option<String>,
}

/// What one stdout line of `cargo` is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CargoLine {
    /// A compiler diagnostic.
    Message(Box<CompilerMessage>),
    /// Another record cargo writes while it builds (`compiler-artifact`,
    /// `build-script-executed`): not meant for the user.
    Other,
    /// `build-finished`, the last record cargo writes. Whatever follows
    /// on standard output is the program's own (`cargo test`), however
    /// much it looks like cargo's records.
    BuildFinished,
}

/// The `reason`s cargo itself uses with `--message-format=json*`.
const MESSAGE_REASON: &str = "compiler-message";
const BUILD_FINISHED_REASON: &str = "build-finished";
const OTHER_REASONS: &[&str] = &["compiler-artifact", "build-script-executed"];

/// Classifies one stdout line. `None` means "not a cargo JSON record":
/// a line of test output, which the caller passes through untouched. Only
/// a line that starts with `{` and parses as a JSON object whose string
/// `reason` is one cargo uses is a record; any other `reason` belongs to
/// the program (a test printing its own JSON).
pub fn classify_line(line: &str) -> Option<CargoLine> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('{') {
        return None;
    }
    let value: Value = serde_json::from_str(trimmed).ok()?;
    let reason = value.get("reason")?.as_str()?;
    if reason == BUILD_FINISHED_REASON {
        return Some(CargoLine::BuildFinished);
    }
    if OTHER_REASONS.contains(&reason) {
        return Some(CargoLine::Other);
    }
    if reason != MESSAGE_REASON {
        return None;
    }
    match parse_message(&value) {
        Some(message) => Some(CargoLine::Message(Box::new(message))),
        None => Some(CargoLine::Other),
    }
}

fn parse_message(record: &Value) -> Option<CompilerMessage> {
    let message = record.get("message")?;
    Some(CompilerMessage {
        manifest_path: PathBuf::from(record.get("manifest_path")?.as_str()?),
        level: message.get("level")?.as_str()?.to_string(),
        code: message
            .get("code")
            .and_then(|code| code.get("code"))
            .and_then(Value::as_str)
            .map(str::to_string),
        message: message.get("message")?.as_str()?.to_string(),
        spans: parse_spans(message.get("spans")),
        children: message
            .get("children")
            .and_then(Value::as_array)
            .map(|children| children.iter().filter_map(parse_child).collect())
            .unwrap_or_default(),
        rendered: message
            .get("rendered")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn parse_child(child: &Value) -> Option<RawChild> {
    Some(RawChild {
        level: child.get("level")?.as_str()?.to_string(),
        message: child.get("message")?.as_str()?.to_string(),
        spans: parse_spans(child.get("spans")),
    })
}

fn parse_spans(spans: Option<&Value>) -> Vec<RawSpan> {
    spans
        .and_then(Value::as_array)
        .map(|spans| spans.iter().filter_map(parse_span).collect())
        .unwrap_or_default()
}

fn parse_span(span: &Value) -> Option<RawSpan> {
    let number = |key: &str| span.get(key).and_then(Value::as_u64);
    Some(RawSpan {
        file_name: span.get("file_name")?.as_str()?.to_string(),
        byte_start: u32::try_from(number("byte_start")?).ok()?,
        byte_end: u32::try_from(number("byte_end")?).ok()?,
        line_start: usize::try_from(number("line_start")?).ok()?,
        line_end: usize::try_from(number("line_end")?).ok()?,
        column_start: usize::try_from(number("column_start")?).ok()?,
        column_end: usize::try_from(number("column_end")?).ok()?,
        is_primary: span.get("is_primary").and_then(Value::as_bool) == Some(true),
        label: span
            .get("label")
            .and_then(Value::as_str)
            .map(str::to_string),
        suggested_replacement: span
            .get("suggested_replacement")
            .and_then(Value::as_str)
            .map(str::to_string),
        has_expansion: span.get("expansion").is_some_and(|e| !e.is_null()),
        line_text: span
            .get("text")
            .and_then(Value::as_array)
            .and_then(|text| text.first())
            .and_then(|line| line.get("text"))
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_of_test_output_is_not_a_cargo_record() {
        assert_eq!(classify_line("running 1 test"), None);
        assert_eq!(classify_line("{ not json"), None);
        assert_eq!(classify_line(r#"{"no":"reason"}"#), None);
    }

    #[test]
    fn a_non_message_record_is_other() {
        for reason in ["compiler-artifact", "build-script-executed"] {
            assert_eq!(
                classify_line(&format!(r#"{{"reason":"{reason}"}}"#)),
                Some(CargoLine::Other)
            );
        }
        assert_eq!(
            classify_line(r#"{"reason":"build-finished","success":true}"#),
            Some(CargoLine::BuildFinished)
        );
    }

    #[test]
    fn a_reason_cargo_does_not_use_is_the_programs_own_output() {
        assert_eq!(classify_line(r#"{"reason":"my-own-event"}"#), None);
    }

    #[test]
    fn a_compiler_message_keeps_spans_children_and_code() {
        let line = r#"{"reason":"compiler-message","manifest_path":"/w/app/Cargo.toml","message":{
            "message":"mismatched types","level":"error","code":{"code":"E0308","explanation":null},
            "rendered":"error: x","children":[{"level":"help","message":"try","spans":[{
              "file_name":"a.rs","byte_start":1,"byte_end":2,"line_start":1,"line_end":1,
              "column_start":2,"column_end":3,"is_primary":true,"label":null,
              "suggested_replacement":"y","expansion":null,"text":[]}]}],
            "spans":[{"file_name":"a.rs","byte_start":5,"byte_end":9,"line_start":2,"line_end":2,
              "column_start":3,"column_end":7,"is_primary":true,"label":"here",
              "suggested_replacement":null,"expansion":null,
              "text":[{"text":"let x;","highlight_start":3,"highlight_end":7}]}]}}"#;
        let Some(CargoLine::Message(message)) = classify_line(&line.replace('\n', " ")) else {
            panic!("expected a message");
        };
        assert_eq!(message.code.as_deref(), Some("E0308"));
        assert_eq!(message.manifest_path, PathBuf::from("/w/app/Cargo.toml"));
        assert_eq!(message.spans[0].byte_start, 5);
        assert_eq!(message.spans[0].line_text.as_deref(), Some("let x;"));
        assert_eq!(
            message.children[0].spans[0]
                .suggested_replacement
                .as_deref(),
            Some("y")
        );
    }
}
