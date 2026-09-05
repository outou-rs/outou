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
    /// Any other Rust item, copied verbatim.
    Rust(RustSource),
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
    Expression(Expr),
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
    Expression(Expr),
    /// A nested element.
    Element(JsxElement),
    /// Recovered garbage.
    Error(ErrorNode),
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
