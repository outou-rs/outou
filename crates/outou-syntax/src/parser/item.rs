//! Top-level and module-level item parsing.
//!
//! This module's structural job is only to find `fn` and `mod` items (with
//! their leading attributes) and hand function bodies to [`super::region`];
//! everything else at item level — `struct`, `impl`, `trait`, `const`,
//! `static`, `use`, method bodies inside `impl`/`trait`, and any other
//! nested item — is kept as one [`ast::Item::Rust`] run per contiguous
//! stretch (decision D1). Unlike Phase 0's original design, that run is
//! *not* an opaque text slice: every byte of it is JSX-scanned at any
//! brace depth via [`super::region::Parser::try_jsx_at`], exactly like a
//! function body, so `const VIEW: Element = <A/>;`, an `impl` method body,
//! and every other expression-bearing context still surfaces its JSX as
//! an [`ast::Expr::Jsx`] node instead of disappearing into raw text (H3).
//!
//! A `fn` is only ever treated as a function *item* when the next
//! significant token after it is an identifier: this is what tells
//! `fn view() -> Element { … }` (a function item) apart from `fn()` /
//! `Fn() -> T` in type position (a fn-pointer or `Fn`-trait type, which
//! never has a name before its parameter list). No structural item
//! grammar beyond this one check is needed in Phase 0.

use outou_sourcemap::Span;

use crate::ast;
use crate::lexer::opaque;
use crate::lexer::rust_token::{self, RtKind, RtTok};

use super::diag;
use super::region::operand_sentinel;
use super::Parser;

/// Where an item-scanning pass stops.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bound {
    /// Run to end of input (the whole file).
    EndOfFile,
    /// Run until the matching `}` of an already-consumed opening brace
    /// (an inline `mod name { ... }`).
    ClosingBrace,
}

/// Where a `fn` item's signature scan stopped (LOW-11, issue #4 fix list
/// item 8): a normal body (`{`), a body-less declaration's own `;` (as
/// inside an `extern` block), or end of input with neither found.
enum SignatureEnd {
    /// Position of the opening `{` of a real body.
    Body(usize),
    /// Position right after a depth-0 `;` that ended a body-less
    /// signature.
    Semicolon(usize),
    /// End of input reached before either.
    Eof,
}

impl<'s> Parser<'s> {
    /// Parses the whole file.
    pub(super) fn parse_file(&mut self) -> ast::File {
        let (items, _end) = self.parse_items(0, Bound::EndOfFile);
        ast::File { items }
    }

    /// Scans items from `start`, stopping per `bound`. Returns the items
    /// and the position just past the stop point (past the matching `}`
    /// for [`Bound::ClosingBrace`], or the file length for
    /// [`Bound::EndOfFile`]).
    ///
    /// Between recognized `fn`/`mod` items, every byte is scanned for JSX
    /// (decision D1) exactly as [`super::region::Parser::scan_region`]
    /// scans a function body: [`Parser::try_jsx_at`](super::region::Parser::try_jsx_at)
    /// is consulted at every `<`, regardless of brace depth, so a
    /// `const`/`static` initializer or an `impl`/`trait` method body can
    /// contain JSX just as a free function's body can. The accumulated
    /// [`ast::Expr`] nodes for the run in progress are kept in `parts`
    /// and turned into one [`ast::Item::Rust`] each time a `fn`/`mod` item
    /// is recognized, the bound is reached, or input runs out.
    fn parse_items(&mut self, start: usize, bound: Bound) -> (Vec<ast::Item>, usize) {
        let len = self.bytes.len();
        let mut items = Vec::new();
        let mut parts: Vec<ast::Expr> = Vec::new();
        let mut run_start = start;
        let mut chunk_start = start;
        let mut pos = start;
        let mut depth: i32 = 0;
        let mut prev: Option<RtTok> = None;

        loop {
            if pos >= len {
                self.flush_rust_run(&mut parts, chunk_start, pos);
                self.finish_rust_run(&mut items, &mut parts, run_start, pos);
                return (items, pos);
            }

            // A comment that cannot possibly become part of a `fn`/`mod`'s
            // attached attributes — any comment once nested past the top
            // level (`depth > 0`, where `scan_leading_attributes` is never
            // even consulted), or a top-level comment that is not itself a
            // doc comment (`///`/`//!`) — is consumed directly as an
            // ordinary token, before ever attempting the
            // `scan_leading_attributes`/head lookahead just below, or the
            // `try_skip_opaque_region` check further down. Both of those
            // call `rust_token::next_significant`, which itself walks past
            // an entire run of comments to find the next real token;
            // attempting either at *every* position inside a long comment
            // run (rather than once, for the whole run) made parsing
            // quadratic in the run's length (HIGH-3, issue #4 fix list item
            // 3).
            let peek = rust_token::next_token(self.bytes, pos);
            let is_doc_comment = peek.kind == RtKind::LineComment && {
                let text = peek.text(self.source);
                text.starts_with("///") || text.starts_with("//!")
            };
            let is_comment = matches!(peek.kind, RtKind::LineComment | RtKind::BlockComment);
            if is_comment && (depth > 0 || !is_doc_comment) {
                prev = Some(peek);
                pos = peek.end;
                continue;
            }

            if depth == 0 {
                let (attrs, after_attrs) = self.scan_leading_attributes(pos);
                let qualified_at = self.skip_item_qualifiers(after_attrs);
                let head = rust_token::next_significant(self.bytes, qualified_at);
                let is_fn_item = head.kind == RtKind::Ident
                    && head.text(self.source) == "fn"
                    && matches!(
                        rust_token::next_significant(self.bytes, head.end).kind,
                        RtKind::Ident | RtKind::RawIdent
                    );
                if is_fn_item {
                    self.flush_rust_run(&mut parts, chunk_start, pos);
                    self.finish_rust_run(&mut items, &mut parts, run_start, pos);
                    let (function, end) = self.parse_function(attrs, pos, head);
                    items.push(ast::Item::Function(function));
                    pos = end;
                    chunk_start = end;
                    run_start = end;
                    prev = None;
                    continue;
                }
                if head.kind == RtKind::Ident && head.text(self.source) == "mod" {
                    self.flush_rust_run(&mut parts, chunk_start, pos);
                    self.finish_rust_run(&mut items, &mut parts, run_start, pos);
                    let qualifiers = self.extract_qualifiers(after_attrs, head.start);
                    let (module, end) = self.parse_module(attrs, qualifiers, pos, head);
                    items.push(ast::Item::Module(module));
                    pos = end;
                    chunk_start = end;
                    run_start = end;
                    prev = None;
                    continue;
                }
                if after_attrs > pos {
                    // Attributes and/or doc comments were collected but did
                    // not precede a `fn`/`mod` item: jump straight past all
                    // of them in one step instead of falling through to the
                    // generic per-token scan below, which would repeat this
                    // same `scan_leading_attributes` call — and its
                    // `next_significant` head lookahead — at every token in
                    // the run (HIGH-3).
                    pos = after_attrs;
                    prev = None;
                    continue;
                }
            }

            if let Some(end) = opaque::try_skip_opaque_region(
                self.bytes,
                pos,
                super::region::is_path_continuation(self.source, prev),
            ) {
                pos = end;
                prev = Some(operand_sentinel(pos));
                continue;
            }

            let tok = peek;
            if tok.start == tok.end {
                self.flush_rust_run(&mut parts, chunk_start, len);
                self.finish_rust_run(&mut items, &mut parts, run_start, len);
                return (items, len);
            }
            match tok.kind {
                RtKind::OpenDelim if self.bytes[tok.start] == b'{' => {
                    depth += 1;
                    prev = Some(tok);
                    pos = tok.end;
                }
                RtKind::CloseDelim if self.bytes[tok.start] == b'}' => {
                    if depth == 0 && bound == Bound::ClosingBrace {
                        self.flush_rust_run(&mut parts, chunk_start, tok.start);
                        self.finish_rust_run(&mut items, &mut parts, run_start, tok.start);
                        return (items, tok.end);
                    }
                    depth -= 1;
                    prev = Some(tok);
                    pos = tok.end;
                }
                RtKind::Punct if self.bytes[tok.start] == b'<' => {
                    match self.try_jsx_at(tok, prev) {
                        None => {
                            prev = Some(tok);
                            pos = tok.end;
                        }
                        Some((expr, new_pos)) => {
                            self.flush_rust_run(&mut parts, chunk_start, tok.start);
                            parts.push(expr);
                            chunk_start = new_pos;
                            pos = new_pos;
                            prev = Some(operand_sentinel(pos));
                        }
                    }
                }
                _ => {
                    prev = Some(tok);
                    pos = tok.end;
                }
            }
        }
    }

    /// Flushes the plain-Rust text `[start, end)` into `parts` as one
    /// [`ast::Expr::Rust`] node, merging into an immediately preceding
    /// Rust node's span when there is one (mirroring
    /// [`super::region::flush`]'s trivia-absorbing behavior, so a run of
    /// whitespace between two JSX nodes does not become its own node).
    fn flush_rust_run(&self, parts: &mut Vec<ast::Expr>, start: usize, end: usize) {
        if start >= end {
            return;
        }
        let text = &self.source[start..end];
        if text.trim().is_empty() {
            if let Some(ast::Expr::Rust(rs)) = parts.last_mut() {
                rs.span = Span::new(rs.span.start, end as u32);
                rs.text = self.source[rs.span.start as usize..end].to_string();
                return;
            }
        }
        parts.push(ast::Expr::Rust(ast::RustSource {
            span: Span::new(start as u32, end as u32),
            text: text.to_string(),
        }));
    }

    /// Turns the accumulated `parts` of one item-level Rust run into an
    /// [`ast::Item::Rust`], if the run is non-empty.
    fn finish_rust_run(
        &self,
        items: &mut Vec<ast::Item>,
        parts: &mut Vec<ast::Expr>,
        start: usize,
        end: usize,
    ) {
        if parts.is_empty() && start >= end {
            return;
        }
        let taken = std::mem::take(parts);
        items.push(ast::Item::Rust(ast::RustItem {
            span: Span::new(start as u32, end as u32),
            parts: taken,
        }));
    }

    /// Collects consecutive leading attributes (`#[...]`, `#![...]`) and
    /// doc comments (`///`, `//!`) starting at `pos`.
    fn scan_leading_attributes(&self, mut pos: usize) -> (Vec<ast::RustSource>, usize) {
        let mut attrs = Vec::new();
        loop {
            while matches!(self.bytes.get(pos), Some(b) if b.is_ascii_whitespace()) {
                pos += 1;
            }
            // A doc comment must be checked for *before* `try_attribute`
            // (LOW-9): `try_attribute` is trivia-tolerant (it looks ahead
            // with `next_significant`, which treats a comment as skippable
            // trivia) so that a caller skipping a *known* opaque region can
            // find `#` past leading whitespace or comments it does not
            // care about. Here, though, every individual doc comment must
            // be recorded as its own attribute, so a `/// doc` immediately
            // followed by `#[component]` must never have its doc comment
            // silently skipped over on the way to the attribute that
            // follows it.
            let tok = rust_token::next_token(self.bytes, pos);
            if tok.kind == RtKind::LineComment {
                let text = tok.text(self.source);
                if text.starts_with("///") || text.starts_with("//!") {
                    attrs.push(ast::RustSource {
                        span: Span::new(tok.start as u32, tok.end as u32),
                        text: text.to_string(),
                    });
                    pos = tok.end;
                    continue;
                }
            }
            if let Some((attr_start, end)) = opaque::try_attribute(self.bytes, pos) {
                attrs.push(ast::RustSource {
                    span: Span::new(attr_start as u32, end as u32),
                    text: self.source[attr_start..end].to_string(),
                });
                pos = end;
                continue;
            }
            break;
        }
        (attrs, pos)
    }

    /// Skips a visibility/qualifier prefix that may sit between an item's
    /// attributes and its `fn`/`mod` keyword — `pub`, `pub(...)`, and any
    /// run of `default`, `const`, `async`, `unsafe`, `extern "abi"?` — so
    /// that the head-detection in [`Self::parse_items`] still recognizes
    /// `fn`/`mod` correctly regardless of them (HIGH-1, issue #4 fix list
    /// item 2). Phase 0 does not need to validate that the qualifiers
    /// appear in a legal order or combination; rustc already does that.
    fn skip_item_qualifiers(&self, mut pos: usize) -> usize {
        let vis = rust_token::next_significant(self.bytes, pos);
        if vis.kind == RtKind::Ident && vis.text(self.source) == "pub" {
            pos = vis.end;
            let paren = rust_token::next_significant(self.bytes, pos);
            if paren.kind == RtKind::OpenDelim && self.bytes.get(paren.start) == Some(&b'(') {
                pos = rust_token::skip_balanced_group(self.bytes, paren.start);
            }
        }
        loop {
            let tok = rust_token::next_significant(self.bytes, pos);
            if tok.kind != RtKind::Ident {
                break;
            }
            match tok.text(self.source) {
                "default" | "const" | "async" | "unsafe" => pos = tok.end,
                "extern" => {
                    pos = tok.end;
                    let abi = rust_token::next_significant(self.bytes, pos);
                    if abi.kind == RtKind::Literal {
                        pos = abi.end;
                    }
                }
                _ => break,
            }
        }
        pos
    }

    /// Captures the trimmed source slice between the end of an item's
    /// attributes (`after_attrs`) and the start of its `fn`/`mod` keyword
    /// (`head_start`) as an [`ast::RustSource`], or `None` when that slice
    /// is empty or all whitespace. For a `mod` item this is its
    /// visibility/qualifier prefix (`pub`, `pub(crate)`, …).
    fn extract_qualifiers(&self, after_attrs: usize, head_start: usize) -> Option<ast::RustSource> {
        let text = &self.source[after_attrs..head_start];
        let leading_ws = text.len() - text.trim_start().len();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }
        let start = after_attrs + leading_ws;
        let end = start + trimmed.len();
        Some(ast::RustSource {
            span: Span::new(start as u32, end as u32),
            text: trimmed.to_string(),
        })
    }

    /// Parses a `fn` item: `head` is the already-located `fn` token.
    fn parse_function(
        &mut self,
        attrs: Vec<ast::RustSource>,
        item_start: usize,
        head: rust_token::RtTok,
    ) -> (ast::Function, usize) {
        let name_tok = rust_token::next_significant(self.bytes, head.end);
        let name = if matches!(name_tok.kind, RtKind::Ident | RtKind::RawIdent) {
            self.ident_at(name_tok.start, name_tok.end)
        } else {
            self.ident_at(head.end, head.end)
        };

        let mut pos = name_tok.end.max(head.end);
        let mut depth: i32 = 0;
        let signature_end = loop {
            // A function *signature* (parameters, generics, return type)
            // is bounded and short in practice, unlike a statement-level
            // path (M12); `prev_is_path_continuation: false` here always
            // attempts macro-invocation detection, matching this loop's
            // pre-existing behavior exactly.
            if let Some(end) = opaque::try_skip_opaque_region(self.bytes, pos, false) {
                pos = end;
                continue;
            }
            let tok = rust_token::next_token(self.bytes, pos);
            if tok.start == tok.end {
                break SignatureEnd::Eof;
            }
            match tok.kind {
                RtKind::OpenDelim if self.bytes[tok.start] == b'{' && depth == 0 => {
                    break SignatureEnd::Body(tok.start)
                }
                RtKind::OpenDelim => depth += 1,
                RtKind::CloseDelim => depth = (depth - 1).max(0),
                // A body-less declaration (`fn foo();`, as inside an
                // `extern` block) ends its signature at its own `;`
                // instead of continuing to hunt for the next `{` —
                // which used to belong to a completely unrelated
                // following item (LOW-11, issue #4 fix list item 8).
                RtKind::Punct if self.bytes[tok.start] == b';' && depth == 0 => {
                    break SignatureEnd::Semicolon(tok.end)
                }
                _ => {}
            }
            pos = tok.end;
        };

        let is_component = attrs
            .iter()
            .any(|a| attribute_meta_path(&a.text) == Some("component"));

        match signature_end {
            SignatureEnd::Body(brace_pos) => {
                let signature = ast::RustSource {
                    span: Span::new(head.start as u32, brace_pos as u32),
                    text: self.source[head.start..brace_pos].to_string(),
                };
                let body = self.parse_block(brace_pos);
                let end = body.span.end as usize;
                (
                    ast::Function {
                        span: Span::new(item_start as u32, end as u32),
                        attributes: attrs,
                        is_component,
                        name,
                        signature,
                        body,
                    },
                    end,
                )
            }
            SignatureEnd::Semicolon(end) => {
                let signature = ast::RustSource {
                    span: Span::new(head.start as u32, end as u32),
                    text: self.source[head.start..end].to_string(),
                };
                (
                    ast::Function {
                        span: Span::new(item_start as u32, end as u32),
                        attributes: attrs,
                        is_component,
                        name,
                        signature,
                        body: ast::Block {
                            span: Span::new(end as u32, end as u32),
                            statements: Vec::new(),
                            tail: None,
                            close: None,
                        },
                    },
                    end,
                )
            }
            SignatureEnd::Eof => {
                // No body and no terminating `;` found before end of
                // input: an incomplete signature. Recovery: an empty body
                // at end of input, no panic. Not one of the required
                // fixtures.
                let end = self.bytes.len();
                let signature = ast::RustSource {
                    span: Span::new(head.start as u32, end as u32),
                    text: self.source[head.start..end].to_string(),
                };
                (
                    ast::Function {
                        span: Span::new(item_start as u32, end as u32),
                        attributes: attrs,
                        is_component,
                        name,
                        signature,
                        body: ast::Block {
                            span: Span::new(end as u32, end as u32),
                            statements: Vec::new(),
                            tail: None,
                            close: None,
                        },
                    },
                    end,
                )
            }
        }
    }

    /// Parses a `mod name;` or `mod name { ... }` item: `head` is the
    /// already-located `mod` token. `qualifiers` is the item's already
    /// extracted visibility/qualifier prefix, if any.
    fn parse_module(
        &mut self,
        attrs: Vec<ast::RustSource>,
        qualifiers: Option<ast::RustSource>,
        item_start: usize,
        head: rust_token::RtTok,
    ) -> (ast::Module, usize) {
        let name_tok = rust_token::next_significant(self.bytes, head.end);
        let name = if matches!(name_tok.kind, RtKind::Ident | RtKind::RawIdent) {
            self.ident_at(name_tok.start, name_tok.end)
        } else {
            self.ident_at(head.end, head.end)
        };
        let path = extract_path_attribute(&attrs);
        let head_parts = ModuleHeadParts {
            attributes: attrs,
            qualifiers,
            name,
            path,
        };
        let next = rust_token::next_significant(self.bytes, name_tok.end.max(head.end));

        if next.kind == RtKind::OpenDelim && self.bytes[next.start] == b'{' {
            if self.mod_depth >= diag::MAX_NESTING {
                return self
                    .recover_too_deeply_nested_module(item_start, head, head_parts, next.start);
            }
            self.mod_depth += 1;
            let (items, end) = self.parse_items(next.end, Bound::ClosingBrace);
            self.mod_depth -= 1;
            return (head_parts.into_module(item_start, end, Some(items)), end);
        }

        // `mod name;`, or a malformed module head: recover by ending the
        // item at the `;` if present, otherwise right after the name.
        let end = if next.kind == RtKind::Punct && self.bytes.get(next.start) == Some(&b';') {
            next.end
        } else {
            name_tok.end.max(head.end)
        };
        (head_parts.into_module(item_start, end, None), end)
    }

    /// Grammar §9's nesting-cap recovery for inline modules (decision D4,
    /// H7): unlike a JSX element's closing tag, an inline module's close is
    /// a plain, unnamed `}`, so the whole body can be skipped in one flat,
    /// non-recursive pass with
    /// [`rust_token::skip_balanced_group`] instead of falling back to end
    /// of input — the rest of the file after this module still parses.
    /// The module's own items are not recorded (`items: None`) since
    /// recording them would require the very recursion this cap avoids.
    fn recover_too_deeply_nested_module(
        &mut self,
        item_start: usize,
        head: rust_token::RtTok,
        head_parts: ModuleHeadParts,
        open_brace_pos: usize,
    ) -> (ast::Module, usize) {
        self.push_diag(
            Span::new(head.start as u32, head.start as u32 + 1),
            diag::modules_nested_too_deeply(),
        );
        let end = rust_token::skip_balanced_group(self.bytes, open_brace_pos);
        (head_parts.into_module(item_start, end, None), end)
    }
}

/// The parts of a `mod` item's head (attributes, qualifiers, name, explicit
/// `#[path]`) collected before it is known whether the module is inline or
/// a bare `mod name;`. Bundled into one value so that
/// [`Parser::recover_too_deeply_nested_module`] does not need one argument
/// per field.
struct ModuleHeadParts {
    attributes: Vec<ast::RustSource>,
    qualifiers: Option<ast::RustSource>,
    name: ast::Ident,
    path: Option<String>,
}

impl ModuleHeadParts {
    /// Combines this head with a span and items into the finished
    /// [`ast::Module`].
    fn into_module(self, start: usize, end: usize, items: Option<Vec<ast::Item>>) -> ast::Module {
        ast::Module {
            span: Span::new(start as u32, end as u32),
            attributes: self.attributes,
            qualifiers: self.qualifiers,
            name: self.name,
            path: self.path,
            items,
        }
    }
}

/// Best-effort extraction of `#[path = "..."]`'s string value from an
/// item's collected attributes.
fn extract_path_attribute(attrs: &[ast::RustSource]) -> Option<String> {
    for attr in attrs {
        if attribute_meta_path(&attr.text) != Some("path") {
            continue;
        }
        if let Some(path_kw) = attr.text.find("path") {
            let after = &attr.text[path_kw + "path".len()..];
            if let Some(quote_start) = after.find('"') {
                let rest = &after[quote_start + 1..];
                if let Some(quote_end) = rest.find('"') {
                    return Some(rest[..quote_end].to_string());
                }
            }
        }
    }
    None
}

/// The identifier right after `#[` or `#![` in an attribute's verbatim
/// text — its meta path's first (and, for `component`/`path`, only)
/// segment. Used to tell a real `#[component]`/`#[path = "…"]` apart from
/// `#[not_component]`/`#[not_path = "…"]` by exact match instead of a
/// substring search over the whole attribute text (L2).
pub fn attribute_meta_path(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let after_hash_bracket = if text.starts_with("#![") {
        3
    } else if text.starts_with("#[") {
        2
    } else {
        return None;
    };
    let tok = rust_token::next_significant(bytes, after_hash_bracket);
    matches!(tok.kind, RtKind::Ident | RtKind::RawIdent).then(|| tok.text(text))
}
