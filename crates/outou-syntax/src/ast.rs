//! Abstract syntax tree.
//!
//! Only JSX is modeled in detail. Rust items and expressions are kept as
//! opaque source slices: Outou hands them to rustc unchanged and lets
//! rust-analyzer understand them. Error and recovery nodes are ordinary
//! nodes so that incomplete files still produce an AST.

use outou_sourcemap::Span;

/// One `.rsx` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// Top-level items, in source order.
    pub items: Vec<Item>,
}

/// A top-level or module-level item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// A function whose body may contain JSX. `#[component]` is recorded as
    /// a backend-neutral marker and lowered by the backend.
    Function(Function),
    /// `mod name;` or `#[path = "…"] mod name;`. Inline modules keep their
    /// items.
    Module(Module),
    /// Any other Rust item (`struct`, `impl`, `trait`, `const`, `static`,
    /// `use`, …), scanned for JSX at any brace depth (decision D1). Method
    /// bodies inside an `impl`/`trait` stay inside `parts` rather than
    /// becoming their own [`Item::Function`] — Phase 0 does not need a
    /// structural Rust item scanner, only to find every JSX expression.
    Rust(RustItem),
    /// Recovered garbage.
    Error(ErrorNode),
}

/// A function item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    /// Whole item span, including attributes.
    pub span: Span,
    /// Attributes and doc comments, verbatim, in order.
    pub attributes: Vec<RustSource>,
    /// Whether `#[component]` is among the attributes.
    pub is_component: bool,
    /// Function name.
    pub name: Ident,
    /// Signature text from `fn` to `{`, verbatim.
    pub signature: RustSource,
    /// Body statements and the optional tail expression.
    pub body: Block,
}

/// A `{ … }` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// Span including the braces.
    pub span: Span,
    /// Statements, verbatim Rust or JSX, **not necessarily in source
    /// order**: trailing trivia between the tail expression and the
    /// block's closing `}` is its own trailing statement (it is never
    /// widened into an adjacent `Jsx` or `Error` node's span), so a
    /// caller that needs source order must sort by span start rather than
    /// assume `statements` then `tail` is byte order (mirrors
    /// [`Island::parts`], whose own doc states the same caveat; unlike
    /// `Island::parts`, `Block::statements` does not exactly partition
    /// `span` on its own — `tail` is part of that partition too).
    pub statements: Vec<Expr>,
    /// Tail expression, if any.
    pub tail: Option<Box<Expr>>,
    /// Span of this block's own closing `}`, when the region scanner
    /// actually found one. `None` when the block ran out at end of input
    /// before finding its own matching `}` (grammar §2.2's Rust-level
    /// recovery), or when there is no real opening brace at all (an
    /// incomplete `fn` signature with no body, `span.start == span.end`).
    /// This must be consulted instead of sniffing whether `source` happens
    /// to end in a `}` byte: a broken construct nested inside the block
    /// (an unterminated string literal, a JSX recovery that ran to end of
    /// input) can itself swallow the source's last `}` without that being
    /// this block's own close (MEDIUM-6, issue #4 fix list item 4).
    pub close: Option<Span>,
}

/// A module declaration or definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// Whole item span.
    pub span: Span,
    /// Attributes such as `#[cfg(…)]` and `#[path = "…"]`, verbatim.
    pub attributes: Vec<RustSource>,
    /// Visibility/qualifier prefix between the attributes and the `mod`
    /// keyword (`pub`, `pub(crate)`, …), verbatim, if any. Captured as the
    /// trimmed source slice between the end of the attributes and the
    /// start of the `mod` keyword; `None` when that slice is empty (a
    /// bare `mod name;`).
    pub qualifiers: Option<RustSource>,
    /// Module name.
    pub name: Ident,
    /// Explicit `#[path]` target, if any.
    pub path: Option<String>,
    /// Items of an inline module; `None` for `mod name;`.
    pub items: Option<Vec<Item>>,
}

/// An expression: either opaque Rust or a JSX element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// Opaque Rust.
    Rust(RustSource),
    /// A JSX element in expression position.
    Jsx(JsxElement),
    /// Recovered garbage.
    Error(ErrorNode),
}

/// A JSX element such as `<UserCard user={user} />` or `<h1>Hi</h1>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsxElement {
    /// Whole element span.
    pub span: Span,
    /// Opening tag, or what could be recovered of it.
    pub open: JsxTag,
    /// Attributes, in source order.
    pub attributes: Vec<JsxAttribute>,
    /// Children, after whitespace normalization.
    pub children: Vec<JsxChild>,
    /// Closing tag. `None` for self-closing elements, `Some(IncompleteTag)`
    /// when it is missing or mismatched.
    pub close: Option<JsxTag>,
    /// Stray, uncatalogued bytes skipped while scanning this element's
    /// opening tag (grammar §4.1's `<div!>` row, M8), each paired with
    /// the [`crate::Diagnostic`] already reported for it. Attributes and
    /// children have no slot for a byte that belongs to neither, so this
    /// is where it is recorded losslessly instead of only ever appearing
    /// in the diagnostics list.
    pub errors: Vec<ErrorNode>,
}

/// One tag of an element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsxTag {
    /// A complete tag with a name.
    Named {
        /// Span of the whole tag.
        span: Span,
        /// Element or component name.
        name: Ident,
    },
    /// A tag that ended before its name or `>` was seen.
    Incomplete(IncompleteTag),
}

/// A JSX attribute: `name`, `name="text"` or `name={expr}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsxAttribute {
    /// Whole attribute span.
    pub span: Span,
    /// Attribute name.
    pub name: Ident,
    /// Value, or `None` for a bare attribute or a missing value.
    pub value: Option<JsxAttributeValue>,
}

/// The value of a [`JsxAttribute`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsxAttributeValue {
    /// `"text"`.
    Text(JsxText),
    /// `{expr}`.
    Expression(Island),
    /// `{` with no closing brace, or otherwise unrecoverable.
    Error(ErrorNode),
}

/// A child of a [`JsxElement`]. Text and expressions are kept as separate
/// nodes; they are never merged into one format string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsxChild {
    /// Normalized text.
    Text(JsxText),
    /// A `{ … }` Rust expression island.
    Expression(Island),
    /// A nested element.
    Element(JsxElement),
    /// Recovered garbage.
    Error(ErrorNode),
}

/// A Rust expression island: the content of a JSX child or attribute-value
/// `{ … }` (grammar §6), kept as an ordered sequence of nodes rather than
/// collapsed into one opaque slice or a single expression.
///
/// `parts` exactly partitions `span` (the content between the braces, not
/// including them): no gaps, no overlaps. This is what lets every JSX
/// element nested anywhere inside an island — as a statement, as the tail,
/// at any depth of `if`/`match`/closures — surface as its own
/// [`Expr::Jsx`] node instead of being swallowed into one
/// [`Expr::Rust`] text slice (grammar §2.1, §9; see the Phase 0 round-trip
/// contract in `docs/grammar.md` §9 and this crate's `README.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Island {
    /// Span of the island's content, between (not including) the braces.
    pub span: Span,
    /// The island's nodes, in source order, partitioning `span`.
    pub parts: Vec<Expr>,
}

/// Text content after JSX whitespace normalization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsxText {
    /// Span of the raw text in the source.
    pub span: Span,
    /// Normalized value.
    pub value: String,
}

/// An identifier or tag name with its span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    /// Span.
    pub span: Span,
    /// Name.
    pub name: String,
}

/// A slice of Rust source, copied verbatim into the generated file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustSource {
    /// Span in the `.rsx` file.
    pub span: Span,
    /// The text.
    pub text: String,
}

/// A contiguous run of item-level Rust between recognized `fn`/`mod`
/// items (grammar §1; decision D1), scanned for JSX at any brace depth so
/// that a `const`/`static` initializer, an `impl`/`trait` method body, or
/// any other nested item can contain JSX just as a free function's body
/// can.
///
/// `parts` exactly partitions `span`: no gaps, no overlaps (the Phase 0
/// round-trip contract, `docs/grammar.md` §9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustItem {
    /// Span of the whole run.
    pub span: Span,
    /// The run's nodes, in source order, partitioning `span`.
    pub parts: Vec<Expr>,
}

/// A region the parser could not understand. Codegen in recovery mode
/// replaces it with a placeholder so rust-analyzer can keep going.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorNode {
    /// Span of the skipped region.
    pub span: Span,
    /// The token that was expected, if a single one can be named.
    pub expected: Option<MissingToken>,
}

/// A token the parser expected but did not find.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingToken {
    /// Position where it should have been.
    pub at: u32,
    /// Human-readable description, e.g. "`>`" or "closing tag `</div>`".
    pub description: String,
}

/// A tag cut short by the end of input or by an unrelated token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncompleteTag {
    /// Span of what was seen.
    pub span: Span,
    /// The name, if the parser got that far.
    pub name: Option<Ident>,
    /// What was missing.
    pub missing: MissingToken,
}

impl File {
    /// Every [`JsxElement`] anywhere in this file, in a depth-first,
    /// source-order-ish traversal (top-level items in order, then each
    /// element's attributes before its children).
    ///
    /// This one traversal used to be copied verbatim in two places outside
    /// this crate — `xtask/src/corpus/splice.rs` and
    /// `crates/outou-syntax/tests/corpus_smoke.rs` (F19, issue #12 review)
    /// — both walking the same `Item`/`Expr`/`JsxElement` shapes to answer
    /// the same question ("does this file contain any JSX at all, and
    /// where"). Kept here once so a new `Item`/`Expr` variant only needs
    /// updating in one place.
    pub fn jsx_elements(&self) -> Vec<&JsxElement> {
        let mut out = Vec::new();
        for item in &self.items {
            collect_in_item(item, &mut out);
        }
        out
    }
}

fn collect_in_item<'a>(item: &'a Item, out: &mut Vec<&'a JsxElement>) {
    match item {
        Item::Function(f) => {
            for expr in f.body.statements.iter().chain(f.body.tail.as_deref()) {
                collect_in_expr(expr, out);
            }
        }
        Item::Module(m) => {
            for inner in m.items.iter().flatten() {
                collect_in_item(inner, out);
            }
        }
        Item::Rust(r) => {
            for part in &r.parts {
                collect_in_expr(part, out);
            }
        }
        Item::Error(_) => {}
    }
}

fn collect_in_expr<'a>(expr: &'a Expr, out: &mut Vec<&'a JsxElement>) {
    if let Expr::Jsx(element) = expr {
        collect_in_element(element, out);
    }
}

fn collect_in_element<'a>(element: &'a JsxElement, out: &mut Vec<&'a JsxElement>) {
    out.push(element);
    for attr in &element.attributes {
        if let Some(JsxAttributeValue::Expression(island)) = &attr.value {
            for part in &island.parts {
                collect_in_expr(part, out);
            }
        }
    }
    for child in &element.children {
        match child {
            JsxChild::Expression(island) => {
                for part in &island.parts {
                    collect_in_expr(part, out);
                }
            }
            JsxChild::Element(nested) => collect_in_element(nested, out),
            JsxChild::Text(_) | JsxChild::Error(_) => {}
        }
    }
}

#[cfg(test)]
mod jsx_elements_tests {
    #[test]
    fn finds_a_top_level_const_initializer_element_and_a_nested_child() {
        let source = "const VIEW: Element = <div><span>{1}</span></div>;";
        let parsed = crate::parse(source);
        let elements = parsed.file.jsx_elements();
        assert_eq!(elements.len(), 2, "{elements:#?}");
    }

    #[test]
    fn finds_nothing_in_plain_rust() {
        let source = "fn f(a: i32, b: i32) -> bool { a < b }";
        let parsed = crate::parse(source);
        assert!(parsed.file.jsx_elements().is_empty());
    }
}
