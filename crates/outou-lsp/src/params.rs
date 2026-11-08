//! The parameters of a `#[component]` function, read from its signature
//! text (`ast::Function::signature`, verbatim from `fn` to the opening
//! `{`): each one's Outou name (raw-identifier prefix dropped, grammar §5.1
//! and `docs/backend-leakage.md` row 17), its type as written, its doc
//! comment and where its name sits in the `.rsx` file.
//!
//! Prop-name completion, and hover and definition at a prop name
//! (`crate::attribute_info`), all read this, so a prop is described in the
//! user's own terms and never through the backend's props builder.
//!
//! The parser understands exactly what a parameter list can hold: nested
//! brackets and generics in the types, `->` inside an `Fn(..) -> T` bound,
//! `///` and `/** */` doc comments (a comma or a parenthesis inside one is
//! prose), plain comments and `#[..]` attributes. A parameter that is not a
//! plain name (`(a, b): (u8, u8)`, `&self`) is skipped: it cannot be a prop.

use outou_sourcemap::Span;

/// One parameter of a component, i.e. one prop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParamInfo {
    /// The prop's name as `.rsx` spells it on an element: `type` for the
    /// parameter `r#type`.
    pub name: String,
    /// The type as written, comments dropped and whitespace collapsed.
    pub ty: String,
    /// The doc comment, if any: `///` markers removed, lines joined by `\n`.
    pub doc: Option<String>,
    /// The parameter's name as it is written in the signature (`r#type`
    /// included), in `.rsx` coordinates: where definition jumps to.
    pub span: Span,
}

/// The parameters of the function whose signature text is `signature`,
/// which starts at byte `base` of the `.rsx` file.
pub(crate) fn parse_params(signature: &str, base: u32) -> Vec<ParamInfo> {
    let bytes = signature.as_bytes();
    let Some(open) = param_list_open(bytes) else {
        return Vec::new();
    };
    let Some(close) = matching_close(bytes, open) else {
        return Vec::new();
    };
    split_top_level(bytes, open + 1, close)
        .into_iter()
        .filter_map(|(start, end)| parse_param(signature, start, end, base))
        .collect()
}

/// The index of the `(` that opens the parameter list: the first one after
/// the function name and any generics (`pub(crate) fn` has an earlier one).
fn param_list_open(bytes: &[u8]) -> Option<usize> {
    let mut i = 0;
    let mut after_fn = false;
    let mut angle = 0i32;
    while i < bytes.len() {
        let skipped = skip_opaque(bytes, i);
        if skipped != i {
            i = skipped;
            continue;
        }
        match bytes[i] {
            b'<' if after_fn => angle += 1,
            b'>' if after_fn && !is_arrow(bytes, i) => angle -= 1,
            b'(' if after_fn && angle == 0 => return Some(i),
            b if is_ident_byte(b) => {
                let start = i;
                while i < bytes.len() && is_ident_byte(bytes[i]) {
                    i += 1;
                }
                if &bytes[start..i] == b"fn" {
                    after_fn = true;
                }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The index of the `)` that closes the `(` at `open`.
fn matching_close(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = open;
    while i < bytes.len() {
        let skipped = skip_opaque(bytes, i);
        if skipped != i {
            i = skipped;
            continue;
        }
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The byte ranges of the comma-separated parameters between `from` and
/// `to`, commas inside brackets, generics and comments not counted.
fn split_top_level(bytes: &[u8], from: usize, to: usize) -> Vec<(usize, usize)> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = from;
    let mut i = from;
    while i < to {
        let skipped = skip_opaque(bytes, i);
        if skipped != i {
            i = skipped;
            continue;
        }
        match bytes[i] {
            b'(' | b'[' | b'{' | b'<' => depth += 1,
            b'>' if is_arrow(bytes, i) => {}
            b')' | b']' | b'}' | b'>' => depth -= 1,
            b',' if depth == 0 => {
                parts.push((start, i));
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < to {
        parts.push((start, to));
    }
    parts
}

fn parse_param(signature: &str, start: usize, end: usize, base: u32) -> Option<ParamInfo> {
    let bytes = signature.as_bytes();
    let (docs, pattern_start) = leading_docs(signature, start, end);
    let colon = pattern_colon(bytes, pattern_start, end)?;
    let (name_start, name_end) = binding_name(bytes, pattern_start, colon)?;
    let name = &signature[name_start..name_end];
    if name == "self" || name == "_" || !is_plain_name(name) {
        return None;
    }
    let ty = join_type(&strip_comments(&signature[colon + 1..end]));
    Some(ParamInfo {
        name: name.strip_prefix("r#").unwrap_or(name).to_string(),
        ty,
        doc: (!docs.is_empty()).then(|| docs.join("\n")),
        span: Span::new(base + name_start as u32, base + name_end as u32),
    })
}

/// The doc-comment lines that lead a parameter, and the index where its
/// pattern starts (after comments and `#[..]` attributes).
fn leading_docs(signature: &str, start: usize, end: usize) -> (Vec<String>, usize) {
    let bytes = signature.as_bytes();
    let mut docs = Vec::new();
    let mut i = start;
    loop {
        while i < end && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let rest = &signature[i..end];
        if rest.starts_with("///") && !rest.starts_with("////") {
            let line_end = rest.find('\n').map_or(end, |n| i + n);
            let text = &signature[i + 3..line_end];
            docs.push(
                text.strip_prefix(' ')
                    .unwrap_or(text)
                    .trim_end()
                    .to_string(),
            );
            i = line_end;
        } else if rest.starts_with("/**") && !rest.starts_with("/***") && !rest.starts_with("/**/")
        {
            let after = skip_comment(bytes, i);
            let inner = signature
                .get(i + 3..after.saturating_sub(2).max(i + 3))
                .unwrap_or_default();
            docs.extend(block_doc_lines(inner));
            i = after;
        } else if rest.starts_with("//") || rest.starts_with("/*") {
            i = skip_comment(bytes, i);
        } else if rest.starts_with("#[") {
            i = skip_attribute(bytes, i, end);
        } else {
            return (docs, i);
        }
    }
}

/// The lines of a `/** .. */` comment body, leading `*` markers removed.
fn block_doc_lines(inner: &str) -> Vec<String> {
    let lines: Vec<String> = inner
        .lines()
        .map(|line| {
            let line = line.trim();
            let line = line.strip_prefix('*').unwrap_or(line);
            line.strip_prefix(' ')
                .unwrap_or(line)
                .trim_end()
                .to_string()
        })
        .collect();
    let first = lines.iter().position(|l| !l.is_empty()).unwrap_or(0);
    let last = lines
        .iter()
        .rposition(|l| !l.is_empty())
        .map_or(0, |n| n + 1);
    lines[first..last.max(first)].to_vec()
}

/// The index just past the `#[..]` attribute that starts at `i`; a bracket
/// inside a string or a comment is not counted.
fn skip_attribute(bytes: &[u8], i: usize, end: usize) -> usize {
    let mut depth = 0i32;
    let mut j = i + 1;
    while j < end {
        let skipped = skip_opaque(bytes, j);
        if skipped != j {
            j = skipped;
            continue;
        }
        match bytes[j] {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return j + 1;
                }
            }
            _ => {}
        }
        j += 1;
    }
    end
}

/// The index of the `:` between a parameter's pattern and its type: not
/// part of a `::`, not inside brackets, not inside a comment.
fn pattern_colon(bytes: &[u8], from: usize, to: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = from;
    while i < to {
        let skipped = skip_opaque(bytes, i);
        if skipped != i {
            i = skipped;
            continue;
        }
        match bytes[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b':' if depth == 0 => {
                if bytes.get(i + 1) == Some(&b':') {
                    i += 2;
                    continue;
                }
                return Some(i);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// `text` with every comment removed.
fn strip_comments(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        let skipped = skip_comment(bytes, i);
        if skipped != i {
            out.push(' ');
            i = skipped;
            continue;
        }
        let ch_len = text[i..].chars().next().map_or(1, char::len_utf8);
        out.push_str(&text[i..i + ch_len]);
        i += ch_len;
    }
    out
}

/// The span of the bound name in a parameter pattern `[from, to)`: trivia,
/// an optional `mut` token, trivia, then one identifier (`r#` included) and
/// nothing else. A pattern that is anything more (`(a, b)`, `&x`, `ref x`)
/// has no name.
fn binding_name(bytes: &[u8], from: usize, to: usize) -> Option<(usize, usize)> {
    let mut i = skip_trivia(bytes, from, to);
    let (mut start, mut end) = ident_token(bytes, i, to)?;
    if &bytes[start..end] == b"mut" {
        i = skip_trivia(bytes, end, to);
        (start, end) = ident_token(bytes, i, to)?;
    }
    (skip_trivia(bytes, end, to) == to).then_some((start, end))
}

/// The identifier token at `i`, `r#` prefix included.
fn ident_token(bytes: &[u8], i: usize, to: usize) -> Option<(usize, usize)> {
    let mut j = i;
    if bytes[i..to].starts_with(b"r#") {
        j += 2;
    }
    let body = j;
    while j < to && is_ident_byte(bytes[j]) {
        j += 1;
    }
    (j > body).then_some((i, j))
}

/// `i` moved past whitespace and comments, but not past `to`.
fn skip_trivia(bytes: &[u8], mut i: usize, to: usize) -> usize {
    while i < to {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let skipped = skip_comment(bytes, i);
        if skipped == i {
            break;
        }
        i = skipped.min(to);
    }
    i
}

/// A type's text with its whitespace collapsed to what a human would write:
/// no space inside brackets or before a comma, and no trailing comma in a
/// generic list (`Vec<\n String,\n >` is `Vec<String>`).
fn join_type(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            pending_space = true;
            continue;
        }
        if ch == '>' && out.ends_with(',') {
            out.pop();
        }
        let prev = out.chars().last();
        let tight = prev.is_none()
            || matches!(prev, Some('<' | '(' | '['))
            || matches!(ch, '>' | ')' | ']' | ',');
        if pending_space && !tight {
            out.push(' ');
        }
        pending_space = false;
        out.push(ch);
    }
    out
}

/// The index after the comment, string or character literal that starts at
/// `i`, or `i` itself when none does. A lifetime (`'a`) is not a literal.
fn skip_opaque(bytes: &[u8], i: usize) -> usize {
    let after_comment = skip_comment(bytes, i);
    if after_comment != i {
        return after_comment;
    }
    match bytes.get(i) {
        Some(b'"') => skip_string(bytes, i + 1),
        Some(b'r') if i == 0 || !is_ident_byte(bytes[i - 1]) => skip_raw_string(bytes, i),
        Some(b'\'') => skip_char(bytes, i),
        _ => i,
    }
}

/// The index after the closing quote of a string whose body starts at `i`.
fn skip_string(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

/// The index after the raw string `r"..."` / `r#"..."#` at `i`, or `i`
/// when `r` starts something else (`r#type`, `return`).
fn skip_raw_string(bytes: &[u8], i: usize) -> usize {
    let hashes = bytes[i + 1..].iter().take_while(|&&b| b == b'#').count();
    let quote = i + 1 + hashes;
    if bytes.get(quote) != Some(&b'"') {
        return i;
    }
    let closing: Vec<u8> = std::iter::once(b'"')
        .chain(std::iter::repeat_n(b'#', hashes))
        .collect();
    bytes[quote + 1..]
        .windows(closing.len())
        .position(|w| w == closing.as_slice())
        .map_or(bytes.len(), |n| quote + 1 + n + closing.len())
}

/// The index after the character literal at `i`, or `i` when the `'` starts
/// a lifetime.
fn skip_char(bytes: &[u8], i: usize) -> usize {
    match bytes.get(i + 1) {
        Some(b'\\') => bytes[i + 3.min(bytes.len() - i)..]
            .iter()
            .position(|&b| b == b'\'')
            .map_or(bytes.len(), |n| i + 3 + n + 1),
        Some(&lead) => {
            let width = match lead {
                0..=0x7f => 1,
                0xc0..=0xdf => 2,
                0xe0..=0xef => 3,
                _ => 4,
            };
            if bytes.get(i + 1 + width) == Some(&b'\'') {
                i + 2 + width
            } else {
                i
            }
        }
        None => i,
    }
}

/// The index after the comment that starts at `i`, or `i` itself when none
/// does. A block comment may nest, as in Rust.
fn skip_comment(bytes: &[u8], i: usize) -> usize {
    match (bytes.get(i), bytes.get(i + 1)) {
        (Some(b'/'), Some(b'/')) => bytes[i..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(bytes.len(), |n| i + n),
        (Some(b'/'), Some(b'*')) => {
            let mut depth = 0i32;
            let mut j = i;
            while j < bytes.len() {
                if bytes[j..].starts_with(b"/*") {
                    depth += 1;
                    j += 2;
                } else if bytes[j..].starts_with(b"*/") {
                    depth -= 1;
                    j += 2;
                    if depth == 0 {
                        return j;
                    }
                } else {
                    j += 1;
                }
            }
            bytes.len()
        }
        _ => i,
    }
}

fn is_arrow(bytes: &[u8], i: usize) -> bool {
    i > 0 && bytes[i - 1] == b'-'
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// An identifier, optionally raw: `label`, `r#type`.
fn is_plain_name(name: &str) -> bool {
    let bare = name.strip_prefix("r#").unwrap_or(name);
    let mut chars = bare.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(signature: &str) -> Vec<String> {
        parse_params(signature, 0)
            .into_iter()
            .map(|p| p.name)
            .collect()
    }

    #[test]
    fn extracts_plain_parameter_names_and_types() {
        let params = parse_params("fn UserCard(user: User) -> Element", 0);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "user");
        assert_eq!(params[0].ty, "User");
        assert_eq!(params[0].doc, None);
    }

    #[test]
    fn strips_the_raw_identifier_prefix_from_the_name_but_not_the_span() {
        let signature = "fn Field(label: String, r#type: String) -> Element";
        let params = parse_params(signature, 100);
        assert_eq!(names(signature), ["label", "type"]);
        let span = params[1].span;
        let start = signature.find("r#type").unwrap() as u32 + 100;
        assert_eq!((span.start, span.end), (start, start + 6));
    }

    #[test]
    fn keeps_generic_types_whole() {
        let params = parse_params(
            "fn TagList(tags: Vec<String>, map: HashMap<String, Vec<u8>>) -> Element",
            0,
        );
        assert_eq!(params[0].ty, "Vec<String>");
        assert_eq!(params[1].name, "map");
        assert_eq!(params[1].ty, "HashMap<String, Vec<u8>>");
    }

    #[test]
    fn a_fn_bound_arrow_does_not_close_a_generic() {
        let signature = "fn Each<F: Fn(u8) -> u8>(run: F, size: u32) -> Element";
        assert_eq!(names(signature), ["run", "size"]);
    }

    #[test]
    fn a_visibility_with_parentheses_is_not_the_parameter_list() {
        assert_eq!(
            names("pub(crate) fn Card(title: String) -> Element"),
            ["title"]
        );
    }

    #[test]
    fn doc_comments_attach_to_the_following_parameter() {
        let signature = "fn Field(\n    /// The label, shown above the input.\n    /// Second line (with parens).\n    label: String,\n    r#type: String,\n) -> Element";
        let params = parse_params(signature, 0);
        assert_eq!(
            params[0].doc.as_deref(),
            Some("The label, shown above the input.\nSecond line (with parens).")
        );
        assert_eq!(params[1].doc, None);
        assert_eq!(params[1].name, "type");
    }

    #[test]
    fn a_block_doc_comment_and_plain_comments_are_handled() {
        let signature = "fn F(\n    /** Size in px. */\n    size: u32,\n    // just a note, with a comma\n    label: String, /* trailing */\n) -> Element";
        let params = parse_params(signature, 0);
        assert_eq!(params[0].doc.as_deref(), Some("Size in px."));
        assert_eq!(params[1].name, "label");
        assert_eq!(params[1].doc, None);
        assert_eq!(params[1].ty, "String");
    }

    #[test]
    fn an_attribute_on_a_parameter_is_skipped() {
        let params = parse_params("fn F(#[props(default)] size: u32) -> Element", 0);
        assert_eq!(params[0].name, "size");
        assert_eq!(params[0].ty, "u32");
    }

    #[test]
    fn a_mutable_binding_keeps_its_name() {
        assert_eq!(names("fn F(mut count: u32) -> Element"), ["count"]);
    }

    #[test]
    fn parameters_that_cannot_be_props_are_skipped() {
        assert_eq!(names("fn f(&self, (a, b): (u8, u8), size: u32)"), ["size"]);
        assert!(names("fn F() -> Element").is_empty());
        assert!(names("not a signature").is_empty());
    }

    #[test]
    fn a_type_spread_over_lines_is_collapsed() {
        let params = parse_params("fn F(items: Vec<\n        String,\n    >) -> Element", 0);
        assert_eq!(params[0].ty, "Vec<String>");
    }

    #[test]
    fn a_unicode_doc_comment_does_not_break_offsets() {
        let signature = "fn F(\n    /// 名前、ラベル\n    label: String,\n) -> Element";
        let params = parse_params(signature, 0);
        assert_eq!(params[0].doc.as_deref(), Some("名前、ラベル"));
        let span = params[0].span;
        assert_eq!(&signature[span.start as usize..span.end as usize], "label");
    }

    #[test]
    fn a_name_inside_another_token_is_located_exactly() {
        let signature = "fn F(mut t: u8, mut mu: u8)";
        let params = parse_params(signature, 0);
        let texts: Vec<&str> = params
            .iter()
            .map(|p| &signature[p.span.start as usize..p.span.end as usize])
            .collect();
        assert_eq!(texts, ["t", "mu"]);
        assert_eq!(
            signature[params[1].span.start as usize..].find("mu"),
            Some(0)
        );
    }

    #[test]
    fn mut_may_be_followed_by_any_whitespace_or_a_comment() {
        let signature = "fn F(mut\tt: u8, mut\nn: u8, mut /* c */ c: u8, muted: u8)";
        let params = parse_params(signature, 0);
        let texts: Vec<&str> = params
            .iter()
            .map(|p| &signature[p.span.start as usize..p.span.end as usize])
            .collect();
        assert_eq!(texts, ["t", "n", "c", "muted"]);
    }

    #[test]
    fn a_pattern_that_is_more_than_a_name_is_skipped() {
        assert_eq!(names("fn F(ref x: u8, &y: &u8, z: u8)"), ["z"]);
        assert_eq!(names("fn F(mut mut x: u8, w: u8)"), ["w"]);
    }

    #[test]
    fn a_bracket_inside_a_string_in_an_attribute_does_not_empty_the_list() {
        assert_eq!(
            names("fn F(#[props(default = \")\")] z: u8, w: u8)"),
            ["z", "w"]
        );
        assert_eq!(names("fn F(#[doc = \"]\"] z: u8, w: u8)"), ["z", "w"]);
        assert_eq!(
            names("fn F(#[props(default = r#\")\"#)] z: u8, w: u8)"),
            ["z", "w"]
        );
    }

    #[test]
    fn a_raw_string_may_contain_quotes_and_brackets() {
        assert_eq!(
            names("fn F(#[props(default = r#\"a\"b)\"#)] z: u8, w: u8)"),
            ["z", "w"]
        );
        assert_eq!(
            names("fn F(#[props(default = r\"a\\)\")] z: u8, w: u8)"),
            ["z", "w"]
        );
    }

    #[test]
    fn character_literals_are_skipped_and_lifetimes_are_not() {
        assert_eq!(
            names("fn F(#[props(default = ')')] z: u8, w: u8)"),
            ["z", "w"]
        );
        assert_eq!(
            names("fn F(#[props(default = '\\'')] z: u8, w: u8)"),
            ["z", "w"]
        );
        let params = parse_params("fn F<'a>(s: &'a str, t: &'a [u8]) -> Element", 0);
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].ty, "&'a str");
        assert_eq!(
            names("fn F(#[props(default = 'é')] z: u8, w: u8)"),
            ["z", "w"]
        );
    }

    #[test]
    fn an_escaped_quote_does_not_end_a_string() {
        assert_eq!(
            names("fn F(#[props(default = \"a\\\")\")] z: u8, w: u8)"),
            ["z", "w"]
        );
    }

    #[test]
    fn a_raw_identifier_is_not_a_raw_string() {
        assert_eq!(
            names("fn F(r#type: String, r: u8, w: u8)"),
            ["type", "r", "w"]
        );
    }

    #[test]
    fn an_underscore_is_not_a_prop() {
        assert_eq!(names("fn F(_: u8, _x: u8, w: u8)"), ["_x", "w"]);
    }

    #[test]
    fn a_tuple_type_keeps_its_trailing_comma() {
        let params = parse_params("fn F(pair: (u8,), f: Box<dyn Fn(u8) -> u8>) -> Element", 0);
        assert_eq!(params[0].ty, "(u8,)");
        assert_eq!(params[1].ty, "Box<dyn Fn(u8) -> u8>");
    }
}
