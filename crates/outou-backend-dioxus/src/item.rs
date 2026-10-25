//! Lowering top-level and inline-module [`Item`]s.

use outou_codegen::{GenerateOptions, Mode, Writer};
use outou_sourcemap::{MappingKind, Span};
use outou_syntax::ast::{Function, Item, RustItem};

use crate::block::lower_block;
use crate::expr::lower_parts;
use crate::module::lower_module;
use crate::Cx;
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
    cx: Cx<'_>,
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
        lower_item(writer, source, item, opts, cx);
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

fn lower_item(writer: &mut Writer, source: &str, item: &Item, opts: &GenerateOptions, cx: Cx<'_>) {
    match item {
        Item::Function(function) => lower_function(writer, source, function, opts, cx),
        Item::Module(module) => lower_module(writer, source, module, opts, cx),
        Item::Rust(rust_item) => lower_rust_item(writer, source, rust_item, cx),
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
/// A function with no body (`body.span.start == body.span.end`, no real
/// `{` at all — see [`outou_syntax::ast::Block::close`]'s doc) is one of
/// two shapes. A declaration ending in its own `;` (`fn Decl(x: i32);`,
/// as in an `extern` block) is valid Rust with no diagnostic, and in
/// [`Mode::Strict`] it is emitted verbatim like any other source. A
/// signature that never reached a body or a `;` (it hit end of input, the
/// next item, or an unmatched closing delimiter) is diagnosed by the
/// parser, so [`outou_codegen::reject_syntax_errors_in_strict_mode`]
/// already refused Strict generation and only [`Mode::Recovery`] gets
/// here with it; the placeholder below keeps its symbol.
///
/// Strict mode MUST NOT drop source bytes for the first shape, and the
/// verbatim branch is also the defense in depth if a zero-diagnostic
/// truncated signature ever reaches codegen again (CRITICAL-1, issue #6):
/// splicing `source[function.span]` keeps every byte and lets `rustc`
/// report the real error at the right place instead of silently compiling
/// a file with the rest of it missing.
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
    cx: Cx<'_>,
) {
    let _ = opts;
    if function.body.span.start == function.body.span.end {
        match cx.mode {
            Mode::Strict => writer.verbatim(source, function.span, MappingKind::Expression, None),
            Mode::Recovery => emit_bodyless_function_placeholder(writer, function),
        }
        return;
    }
    // `function.span` starts with the trivia before the first attribute.
    let attributes_start = function
        .attributes
        .first()
        .map_or(function.span.start, |a| a.span.start);
    let allow_non_snake_case = function.is_component && !is_snake_case(&function.name.name);
    if allow_non_snake_case {
        // rust-analyzer's native `non_snake_case` check sees the item before
        // `#[component]` expands, so it warns on every PascalCase component
        // (F17 (b)). Unmapped text on the line of the first attribute, so no
        // line shifts.
        writer.verbatim(
            source,
            Span::new(function.span.start, attributes_start),
            MappingKind::Expression,
            None,
        );
        writer.raw(NON_SNAKE_CASE_ALLOW);
    }
    writer.verbatim(
        source,
        Span::new(
            if allow_non_snake_case {
                attributes_start
            } else {
                function.span.start
            },
            function.signature.span.start,
        ),
        MappingKind::Expression,
        None,
    );
    writer.verbatim(
        source,
        function.signature.span,
        MappingKind::Expression,
        None,
    );
    lower_block(writer, source, &function.body, cx);
}

/// Written before a PascalCase `#[component]` function, scoped to it.
const NON_SNAKE_CASE_ALLOW: &str = "#[allow(non_snake_case)] ";

/// Whether `name` has no uppercase letter.
fn is_snake_case(name: &str) -> bool {
    !name.chars().any(char::is_uppercase)
}

fn lower_rust_item(writer: &mut Writer, source: &str, rust_item: &RustItem, cx: Cx<'_>) {
    lower_parts(writer, source, &rust_item.parts, rust_item.span, cx);
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
