//! Lowering an [`ast::Module`] declaration or definition.
//!
//! See `crates/outou-modules/README.md` for the generated-path convention
//! this follows: a file-based module (`mod name;`) whose
//! [`module_paths_key`] appears in [`GenerateOptions::module_paths`] gets
//! its `#[path]` rewritten to point at the generated file; codegen does
//! not resolve the module graph itself, it only applies a mapping the
//! caller already computed.

use outou_codegen::{module_paths_key, GenerateOptions, Mode, Writer};
use outou_sourcemap::{MappingKind, Span};
use outou_syntax::ast as syntax_ast;

use crate::escape::plain_rust_string_literal;
use crate::item::{item_span, lower_items};

/// Lowers one module item.
///
/// - An inline module (`mod name { … }`) has no file of its own, so
///   `module_paths` does not apply to it: its header and closing brace are
///   copied verbatim and its items are lowered exactly like top-level
///   items (JSX inside a nested module's items is lowered the same way as
///   anywhere else — grammar §1, decision D1).
/// - A file-based module (`mod name;`) whose name is a key of
///   `opts.module_paths` gets `#[path = "<value>"]` emitted in place of
///   its original `#[path]` (if it had one), with its other attributes and
///   qualifiers preserved verbatim.
/// - A file-based module absent from `opts.module_paths` (the module
///   graph has not been resolved into paths yet, or this module is not
///   one of its own generated units) is copied verbatim, unchanged.
pub fn lower_module(
    writer: &mut Writer,
    source: &str,
    module: &syntax_ast::Module,
    opts: &GenerateOptions,
    mode: Mode,
) {
    if let Some(items) = &module.items {
        lower_inline_module(writer, source, module, items, opts, mode);
        return;
    }

    let own_path_attribute = module
        .attributes
        .iter()
        .find(|attribute| is_path_attribute(&attribute.text))
        .map(|attribute| attribute.text.as_str());
    let key = module_paths_key(&module.name.name, own_path_attribute);
    match opts.module_paths.get(&key) {
        None => writer.verbatim(source, module.span, MappingKind::Expression, None),
        Some(path) => lower_file_module_with_new_path(writer, source, module, path),
    }
}

fn lower_inline_module(
    writer: &mut Writer,
    source: &str,
    module: &syntax_ast::Module,
    items: &[syntax_ast::Item],
    opts: &GenerateOptions,
    mode: Mode,
) {
    if items.is_empty() {
        writer.verbatim(source, module.span, MappingKind::Expression, None);
        return;
    }
    let items_start = item_span(&items[0]).start;
    let items_end = item_span(&items[items.len() - 1]).end;
    writer.verbatim(
        source,
        Span::new(module.span.start, items_start),
        MappingKind::Expression,
        None,
    );
    lower_items(
        writer,
        source,
        items,
        opts,
        mode,
        Span::new(items_start, items_end),
    );
    writer.verbatim(
        source,
        Span::new(items_end, module.span.end),
        MappingKind::Expression,
        None,
    );
}

fn lower_file_module_with_new_path(
    writer: &mut Writer,
    source: &str,
    module: &syntax_ast::Module,
    path: &str,
) {
    // Leading trivia (blank lines, comments) belongs to `module.span`
    // itself — the same convention `Function::span` uses — up to wherever
    // the first attribute (or, absent one, the qualifiers, or absent
    // those too, the `mod` keyword) begins. Reproduced verbatim so
    // rewriting `#[path]` does not also silently eat the formatting
    // around the declaration.
    let head_end = module
        .attributes
        .first()
        .map(|attribute| attribute.span.start)
        .or_else(|| {
            module
                .qualifiers
                .as_ref()
                .map(|qualifiers| qualifiers.span.start)
        })
        .unwrap_or_else(|| mod_keyword_start(source, module));
    if module.span.start < head_end {
        writer.verbatim(
            source,
            Span::new(module.span.start, head_end),
            MappingKind::Expression,
            None,
        );
    }

    for attribute in &module.attributes {
        if is_path_attribute(&attribute.text) {
            // Replaced, never reproduced: two `#[path]` attributes on one
            // declaration would leave rustc using whichever is written
            // first (`crates/outou-modules/README.md`).
            continue;
        }
        writer.verbatim(source, attribute.span, MappingKind::Expression, None);
        writer.raw("\n");
    }
    writer.raw("#[path = ");
    writer.raw(&plain_rust_string_literal(path));
    writer.raw("]\n");
    if let Some(qualifiers) = &module.qualifiers {
        writer.verbatim(source, qualifiers.span, MappingKind::Expression, None);
        writer.raw(" ");
    }
    writer.raw("mod ");
    writer.mapped(
        &module.name.name,
        &[module.name.span],
        MappingKind::Identifier,
        None,
    );
    writer.raw(";");
}

/// Where the `mod` keyword itself starts, for a module with neither
/// attributes nor qualifiers (so nothing else marks the boundary between
/// leading trivia and the declaration). Nothing but whitespace/comments
/// can occupy the bytes between `module.span.start` and the name in that
/// case, so the last `"mod"` found before the name is the keyword.
fn mod_keyword_start(source: &str, module: &syntax_ast::Module) -> u32 {
    let prefix = &source[module.span.start as usize..module.name.span.start as usize];
    match prefix.rfind("mod") {
        Some(offset) => module.span.start + offset as u32,
        None => module.span.start,
    }
}

/// Whether an attribute's verbatim text (`#[…]`) is a `#[path = "…"]`
/// attribute, by its meta path — the only attribute codegen ever
/// replaces (see `attributes_without_path()` in `outou-modules`, which
/// this mirrors for the one case this crate needs without depending on
/// that crate).
fn is_path_attribute(text: &str) -> bool {
    let inner = text
        .strip_prefix("#[")
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(text)
        .trim_start();
    // A prefix match alone also matches `#[pathological]`, silently
    // dropping an unrelated attribute (MEDIUM-10, issue #6 fix list item
    // 10): the byte right after `path` must be `=`, `(`, whitespace, or
    // the attribute's own end for this to actually be `#[path]`.
    match inner.strip_prefix("path") {
        None => false,
        Some(rest) => match rest.chars().next() {
            None => true,
            Some(c) => c == '=' || c == '(' || c.is_whitespace(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_path_attribute_variants() {
        assert!(is_path_attribute("#[path = \"x.rs\"]"));
        assert!(is_path_attribute("#[path=\"x.rs\"]"));
        assert!(!is_path_attribute("#[cfg(test)]"));
        assert!(!is_path_attribute("#[component]"));
    }

    #[test]
    fn does_not_match_an_attribute_that_merely_starts_with_path() {
        // MEDIUM-10, issue #6 fix list item 10: a prefix match silently
        // dropped `#[pathological]` entirely, mistaking it for `#[path]`.
        assert!(!is_path_attribute("#[pathological]"));
        assert!(!is_path_attribute("#[path_ish]"));
        assert!(!is_path_attribute("#[pathfinder = \"x\"]"));
    }
}
