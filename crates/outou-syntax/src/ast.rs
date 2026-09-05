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
    /// Statements, verbatim Rust or JSX.
    pub statements: Vec<Expr>,
    /// Tail expression, if any.
    pub tail: Option<Box<Expr>>,
}

/// A module declaration or definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// Whole item span.
    pub span: Span,
    /// Attributes such as `#[cfg(…)]` and `#[path = "…"]`, verbatim.
    pub attributes: Vec<RustSource>,
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
