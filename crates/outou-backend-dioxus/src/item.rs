//! Lowering top-level and inline-module [`Item`]s.

use outou_codegen::{GenerateOptions, Mode, Writer};
use outou_sourcemap::{MappingKind, Span};
use outou_syntax::ast::{Function, Item, RustItem};

use crate::block::lower_block;
use crate::expr::lower_parts;
use crate::module::lower_module;
use crate::PRIVATE_PATH;

/// The span an [`Item`] occupies, for sorting and gap-filling.
pub(crate) fn item_span(item: &Item) -> Span {
    match item {
        Item::Function(f) => f.span,
        Item::Module(m) => m.span,
        Item::Rust(r) => r.span,
        Item::Error(e) => e.span,
    }
}

/// Lowers a source-ordered, span-partitioning run of items (`File::items`,
/// or an inline `Module::items`), filling any gap between them — and
/// before the first / after the last, within `region` — with verbatim
/// Rust.
pub fn lower_items(
    writer: &mut Writer,
    source: &str,
    items: &[Item],
    opts: &GenerateOptions,
    mode: Mode,
    region: Span,
) {
    let mut cursor = region.start;
    for item in items {
        let span = item_span(item);
        if cursor < span.start {
            writer.verbatim(
                source,
                Span::new(cursor, span.start),
                MappingKind::Expression,
                None,
            );
        }
        lower_item(writer, source, item, opts, mode);
        cursor = span.end.max(cursor);
    }
    if cursor < region.end {
        writer.verbatim(
            source,
            Span::new(cursor, region.end),
            MappingKind::Expression,
            None,
        );
    }
}

fn lower_item(writer: &mut Writer, source: &str, item: &Item, opts: &GenerateOptions, mode: Mode) {
    match item {
        Item::Function(function) => lower_function(writer, source, function, opts, mode),
        Item::Module(module) => lower_module(writer, source, module, opts, mode),
        Item::Rust(rust_item) => lower_rust_item(writer, source, rust_item, mode),
        // Reserved-syntax error nodes never reach here in Strict mode
        // (refused earlier by `reject_syntax_errors_in_strict_mode`). In
        // Recovery mode, item-level garbage that could not even be
        // recognized as the start of a function or module is dropped —
        // there is no item shape to reconstruct it as, and dropping a
        // top-level item never breaks the syntax of the items around it.
        Item::Error(_) => {}
    }
}

/// Lowers `#[component] fn Name(...) -> Element { … }` (or any plain
/// function; JSX inside a non-component function is lowered identically —
/// `#[component]` is a backend-neutral marker preserved verbatim, not a
/// codegen switch). Attributes and the signature are copied verbatim; only
/// the body's JSX is lowered.
///
/// A function whose signature never reached a body or a body-less `;`
/// before end of input (`body.span.start == body.span.end`, no real `{` at
/// all — see [`outou_syntax::ast::Block::close`]'s doc) cannot be
/// reconstructed as a valid Rust item at all: its parameter list is
/// truncated mid-token.
///
/// [`Mode::Strict`] MUST NOT drop source bytes here: the parser does not
/// always raise an error diagnostic for this shape (a body-less `fn App(`
/// mid-file swallows the rest of the file into its own signature text
/// with zero diagnostics — CRITICAL-1, issue #6), so
/// [`outou_codegen::reject_syntax_errors_in_strict_mode`] cannot be
/// relied on to have already refused generation. Splicing
/// `source[function.span]` verbatim instead keeps every byte and lets
/// `rustc` report the real error (an unclosed delimiter) at the right
/// place, rather than silently compiling a truncated file with the rest
/// of it missing.
///
/// [`Mode::Recovery`] instead keeps the *symbol* (MEDIUM-14, issue #6 fix
/// list item 7): the function's name is known even though its parameter
/// list is not, and dropping the item removed `App` from the outline,
/// from completion and from every use site in the file — the opposite of
/// what recovery mode exists for. A placeholder
/// (`fn App() -> ::outou::__private::Element { ::outou::__private::recovery() }`,
/// name mapped to `function.name.span`) keeps it resolvable; its original
/// attributes are dropped (`#[component]` needs a coherent signature to
/// mean anything) and its body is never executed — see the same
/// never-run contract [`crate::recovery`] documents.
fn lower_function(
    writer: &mut Writer,
    source: &str,
    function: &Function,
    opts: &GenerateOptions,
    mode: Mode,
) {
    let _ = opts;
    if function.body.span.start == function.body.span.end {
        match mode {
            Mode::Strict => writer.verbatim(source, function.span, MappingKind::Expression, None),
            Mode::Recovery => emit_bodyless_function_placeholder(writer, function),
        }
        return;
    }
    writer.verbatim(
        source,
        Span::new(function.span.start, function.signature.span.start),
        MappingKind::Expression,
        None,
    );
    writer.verbatim(
        source,
        function.signature.span,
        MappingKind::Expression,
        None,
    );
    lower_block(writer, source, &function.body, mode);
}

fn lower_rust_item(writer: &mut Writer, source: &str, rust_item: &RustItem, mode: Mode) {
    lower_parts(writer, source, &rust_item.parts, rust_item.span, mode);
}

/// Emits the [`Mode::Recovery`] placeholder for a body-less function
/// (MEDIUM-14): `fn <Name>() -> ::outou::__private::Element {
/// ::outou::__private::recovery() }`, with the name mapped to
/// `function.name.span` so hover/definition/rename still land on it.
fn emit_bodyless_function_placeholder(writer: &mut Writer, function: &Function) {
    writer.raw("fn ");
    writer.mapped(
        &function.name.name,
        &[function.name.span],
        MappingKind::Identifier,
        None,
    );
    writer.raw("() -> ");
    writer.raw(PRIVATE_PATH);
    writer.raw("::Element { ");
    writer.raw(PRIVATE_PATH);
    writer.raw("::recovery() }");
}
