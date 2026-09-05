//! The resolver: walks `mod` declarations starting at a crate root and
//! builds an explicit [`ModuleGraph`].
//!
//! See [`crate::scope`] for the directory-ownership model that decides
//! where an implicit `mod name;` searches and where an explicit
//! `#[path]` resolves, and [`crate::probe`]/[`crate::generated`] for the
//! smaller helpers this module composes: candidate-file probing,
//! attribute classification, out-of-crate rejection, and generated-name
//! disambiguation.
//!
//! Two other hazards are guarded against here, since Outou's own
//! cross-file resolution recursion — unlike rustc's, which is bounded by
//! ordinary filesystem nesting — can be driven arbitrarily deep by
//! `#[path]` alone:
//!
//! - **Cycles** ([`ModuleError::Circular`]): a `mod` reachable through
//!   `#[path]` that re-opens a file already open higher up the same
//!   chain used to overflow the stack (a SIGABRT, not a diagnosable
//!   error) instead of being reported. Detected via canonical file
//!   identity (falling back to the lexical path when canonicalization
//!   fails), tracked on [`ResolveCtx::active`], pushed before parsing a
//!   newly opened file and popped after.
//! - **Excess depth** ([`ModuleError::TooDeep`], [`MAX_MODULE_DEPTH`]):
//!   canonical identity does not bound *acyclic* depth — a long, acyclic
//!   `#[path]` chain can still overflow the stack, and an abort is
//!   unacceptable in `outou-lsp`.
//!
//! Both counters advance only when a *new file* is opened for a `mod
//! name;` (or `#[path]`-pinned) declaration, not when descending into an
//! inline `mod m { ... }`: an inline module shares its parent's physical
//! file, and the parser (`outou-syntax`) already caps inline nesting
//! within one file at parse time.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use outou_syntax::ast;
use outou_syntax::Span;

use crate::generated::GeneratedNames;
use crate::probe::{
    conditional_path_attribute, crate_relative, existing_file, is_cfg_attribute,
    resolve_by_candidates,
};
use crate::scope::{unraw, DirScope};
use crate::{relative_to, ModuleError, ModuleGraph, ModuleNode, SourceKind};

/// Module nesting depth cap (decision 1): independent of cycle detection,
/// since canonical identity does not bound acyclic depth.
pub const MAX_MODULE_DEPTH: usize = 128;

/// One entry on the active-file stack used for cycle detection.
struct ActiveFile {
    /// Canonicalized identity used for comparison (falls back to
    /// `lexical` when canonicalization fails, e.g. on a filesystem that
    /// does not support it).
    canonical: PathBuf,
    /// The path as written, used for rendering the reported chain.
    lexical: PathBuf,
}

/// State threaded through the whole resolution, independent of any one
/// `mod` declaration: the crate root (for relativizing paths), the
/// open-file stack for cycle detection, and the current file-open nesting
/// depth. Bundled into one value passed by `&mut` so that
/// [`resolve_items`] does not need one argument per field.
struct ResolveCtx<'a> {
    crate_dir: &'a Path,
    active: Vec<ActiveFile>,
    depth: usize,
    /// In-memory text that overrides whatever is on disk for a given
    /// (canonicalized) path — an editor's unsaved buffer, keyed by
    /// [`canonicalize_or_lexical`] so it lines up regardless of how the
    /// path was spelled on the way in. Empty for the ordinary [`resolve`]
    /// entry point; populated by [`resolve_with_overlay`] so a language
    /// server can plan a crate the way its buffers currently read, not
    /// the way they last were saved (issue #9 Gate 3 review, M2/HIGH-2:
    /// planning always re-read every file from disk, so a `mod` typed
    /// into an unsaved buffer never entered the graph at all).
    overlay: &'a HashMap<PathBuf, String>,
}

/// Resolves the module graph starting at `root` (a `main.rs`, `main.rsx`,
/// `lib.rs` or `lib.rsx`).
///
/// Paths recorded on the returned graph (`ModuleNode::file` and the paths
/// inside [`ModuleError`]) are relative to the crate root — the directory
/// containing `root`'s own directory (conventionally `src/`) — not to the
/// process's current directory.
pub fn resolve(root: &Path) -> Result<ModuleGraph, ModuleError> {
    resolve_with_overlay(root, &HashMap::new())
}

/// Like [`resolve`], but any file whose [`canonicalize_or_lexical`]
/// identity matches a key of `overlay` is read from there instead of
/// disk. Every source file this resolution reads — the crate root and
/// every `mod`-reachable descendant, `.rsx` or `.rs` — consults it, so an
/// unsaved edit anywhere in the graph is honored, not only at the crate
/// root.
pub fn resolve_with_overlay(
    root: &Path,
    overlay: &HashMap<PathBuf, String>,
) -> Result<ModuleGraph, ModuleError> {
    let src_dir = root.parent().unwrap_or_else(|| Path::new(""));
    let crate_dir = src_dir.parent().unwrap_or_else(|| Path::new(""));

    let source = read_source(
        root,
        overlay,
        "<crate root>",
        root,
        crate_dir,
        Span::new(0, 0),
    )?;
    let parsed = outou_syntax::parse(&source);
    let kind = source_kind_from_extension(root);

    let mut ctx = ResolveCtx {
        crate_dir,
        active: vec![ActiveFile {
            canonical: canonicalize_or_lexical(root),
            lexical: root.to_path_buf(),
        }],
        depth: 1,
        overlay,
    };
    let scope = DirScope::root(src_dir);
    let children = resolve_items(&parsed.file.items, root, &scope, &[], &[], &mut ctx)?;

    Ok(ModuleGraph {
        root: ModuleNode {
            path: Vec::new(),
            file: relative_to(root, crate_dir),
            kind,
            declared_in: None,
            visibility: None,
            is_inline: false,
            cfg: Vec::new(),
            attributes: Vec::new(),
            span: None,
            generated: Vec::new(),
            children,
        },
    })
}

/// Resolves every [`ast::Item::Module`] in `items`, declared in
/// `current_file`, under directory scope `scope`, with module paths and
/// generated-name prefixes so far given by `path_prefix`/`generated_prefix`.
/// Paths stored on the result are relative to `ctx.crate_dir`.
fn resolve_items(
    items: &[ast::Item],
    current_file: &Path,
    scope: &DirScope,
    path_prefix: &[String],
    generated_prefix: &[String],
    ctx: &mut ResolveCtx<'_>,
) -> Result<Vec<ModuleNode>, ModuleError> {
    let mut nodes = Vec::new();
    let mut generated_names = GeneratedNames::default();
    let declared_in = relative_to(current_file, ctx.crate_dir);

    for item in items {
        let ast::Item::Module(module) = item else {
            continue;
        };
        let name = module.name.name.clone();
        let unraw_name = unraw(&name).to_string();
        let span = module.span;
        let mut path = path_prefix.to_vec();
        path.push(name.clone());
        let attributes: Vec<String> = module.attributes.iter().map(|a| a.text.clone()).collect();
        let cfg: Vec<String> = attributes
            .iter()
            .filter(|text| is_cfg_attribute(text))
            .cloned()
            .collect();
        let visibility = module.qualifiers.as_ref().map(|q| q.text.clone());

        if let Some(attribute) = conditional_path_attribute(&attributes) {
            return Err(ModuleError::ConditionalPath {
                name,
                declared_in: declared_in.clone(),
                span,
                attribute,
            });
        }

        let mut generated = generated_prefix.to_vec();
        generated.push(generated_names.next(&unraw_name));

        if let Some(inline_items) = &module.items {
            // Inline module: same physical file, current file's kind.
            let kind = source_kind_from_extension(current_file);
            let child_scope = scope.child_of_inline(&unraw_name, module.path.as_deref());
            let children = resolve_items(
                inline_items,
                current_file,
                &child_scope,
                &path,
                &generated,
                ctx,
            )?;
            nodes.push(ModuleNode {
                path,
                file: relative_to(current_file, ctx.crate_dir),
                kind,
                declared_in: Some(declared_in.clone()),
                visibility,
                is_inline: true,
                cfg,
                attributes,
                span: Some(span),
                generated,
                children,
            });
            continue;
        }

        // File module: `mod name;`, either found by candidate search or
        // pinned by `#[path = "…"]`.
        let (target, is_mod_rs_style) = match &module.path {
            Some(explicit) => {
                let target = scope.dir.join(explicit);
                crate_relative(&target, ctx.crate_dir, &name, current_file, span)?;
                if !existing_file(&target, &name, current_file, span, ctx.crate_dir)? {
                    return Err(ModuleError::NotFound {
                        name,
                        declared_in: declared_in.clone(),
                        span,
                        candidates: vec![relative_to(&target, ctx.crate_dir)],
                    });
                }
                // A file loaded via `#[path]` is mod-rs-like: its own
                // children look up directly in its containing directory.
                (target, true)
            }
            None => resolve_by_candidates(
                &scope.lookup_dir(),
                &unraw_name,
                ctx.crate_dir,
                current_file,
                span,
            )?,
        };

        if ctx.depth >= MAX_MODULE_DEPTH {
            return Err(ModuleError::TooDeep {
                name,
                declared_in: declared_in.clone(),
                span,
                limit: MAX_MODULE_DEPTH,
            });
        }

        let canonical = canonicalize_or_lexical(&target);
        if let Some(reentry) = ctx
            .active
            .iter()
            .position(|open| open.canonical == canonical)
        {
            let mut chain: Vec<PathBuf> = ctx.active[reentry..]
                .iter()
                .map(|open| relative_to(&open.lexical, ctx.crate_dir))
                .collect();
            chain.push(relative_to(&target, ctx.crate_dir));
            return Err(ModuleError::Circular {
                name,
                declared_in: declared_in.clone(),
                span,
                chain,
            });
        }

        let kind = source_kind_from_extension(&target);
        let contents = read_source(
            &target,
            ctx.overlay,
            &name,
            current_file,
            ctx.crate_dir,
            span,
        )?;
        let parsed = outou_syntax::parse(&contents);
        let child_scope = DirScope::child_of_file(&target, &unraw_name, is_mod_rs_style);

        ctx.active.push(ActiveFile {
            canonical,
            lexical: target.clone(),
        });
        ctx.depth += 1;
        let children_result = resolve_items(
            &parsed.file.items,
            &target,
            &child_scope,
            &path,
            &generated,
            ctx,
        );
        ctx.depth -= 1;
        ctx.active.pop();
        let children = children_result?;

        nodes.push(ModuleNode {
            path,
            file: relative_to(&target, ctx.crate_dir),
            kind,
            declared_in: Some(declared_in.clone()),
            visibility,
            is_inline: false,
            cfg,
            attributes,
            span: Some(span),
            generated,
            children,
        });
    }

    Ok(nodes)
}

/// Reads `path`'s contents, preferring `overlay`'s entry for it (matched
/// by [`canonicalize_or_lexical`] identity, falling back to a raw lexical
/// match for a path `canonicalize` cannot resolve) over the filesystem.
/// See [`resolve_with_overlay`].
fn read_source(
    path: &Path,
    overlay: &HashMap<PathBuf, String>,
    module_name: &str,
    declared_in: &Path,
    crate_dir: &Path,
    span: Span,
) -> Result<String, ModuleError> {
    let canonical = canonicalize_or_lexical(path);
    if let Some(text) = overlay.get(&canonical).or_else(|| overlay.get(path)) {
        return Ok(text.clone());
    }
    read_to_string(path, module_name, declared_in, crate_dir, span)
}

/// Reads `path`'s contents, translating an I/O failure into a
/// [`ModuleError::Io`] naming `module_name`, `declared_in` and `span`, and
/// the offending path, all relative to `crate_dir` where applicable.
fn read_to_string(
    path: &Path,
    module_name: &str,
    declared_in: &Path,
    crate_dir: &Path,
    span: Span,
) -> Result<String, ModuleError> {
    fs::read_to_string(path).map_err(|source| ModuleError::Io {
        name: module_name.to_string(),
        path: relative_to(path, crate_dir),
        declared_in: relative_to(declared_in, crate_dir),
        span,
        source,
    })
}

/// Canonicalizes `path` for cycle-detection identity, falling back to the
/// lexical path unchanged when canonicalization fails (e.g. a filesystem
/// that does not support it, or — in a unit test — a target constructed
/// without touching disk).
fn canonicalize_or_lexical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// The extension decides [`SourceKind`]: `.rsx` goes through the Outou
/// front end, anything else (in practice, `.rs`) is plain Rust.
fn source_kind_from_extension(path: &Path) -> SourceKind {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("rsx") => SourceKind::Rsx,
        _ => SourceKind::Rust,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn relative_to_strips_the_base_when_present() {
        assert_eq!(
            relative_to(Path::new("/a/b/src/c.rs"), Path::new("/a/b")),
            PathBuf::from("src/c.rs")
        );
        assert_eq!(
            relative_to(Path::new("/x/y.rs"), Path::new("/a/b")),
            PathBuf::from("/x/y.rs")
        );
    }

    /// M2 (issue #9 Gate 3 review, HIGH-2): a `mod` declaration typed into
    /// an unsaved buffer must enter the graph even though the file on
    /// disk does not have it yet.
    #[test]
    fn resolve_with_overlay_sees_a_mod_declaration_only_present_in_the_overlay() {
        let tmp = TempDir::new("resolve-overlay");
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).expect("creating src dir");
        fs::write(src.join("main.rs"), "fn main() {}\n").expect("writing crate root");
        fs::write(src.join("newmod.rs"), "pub fn f() {}\n").expect("writing the new module file");

        let root = src.join("main.rs");
        let overlay_text = "mod newmod;\nfn main() {}\n";
        let mut overlay = std::collections::HashMap::new();
        overlay.insert(root.clone(), overlay_text.to_string());

        // Without the overlay, the disk text (no `mod newmod;`) is what
        // gets resolved.
        let without_overlay = resolve(&root).expect("resolves");
        assert!(without_overlay.root.children.is_empty());

        let with_overlay = resolve_with_overlay(&root, &overlay).expect("resolves with overlay");
        assert_eq!(with_overlay.root.children.len(), 1);
        assert_eq!(
            with_overlay.root.children[0].path,
            vec!["newmod".to_string()]
        );
    }

    #[test]
    fn nesting_deeper_than_the_limit_is_an_error() {
        let tmp = TempDir::new("too-deep");
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).expect("creating src dir");

        // main.rs -> m0.rs -> m1.rs -> ... each via an explicit `#[path]`
        // so there is never any ambiguity to resolve, only depth.
        let total = MAX_MODULE_DEPTH + 4;
        for i in 0..total {
            let content = format!("#[path = \"m{}.rs\"]\nmod m{};\n", i + 1, i + 1);
            fs::write(src.join(format!("m{i}.rs")), content).expect("writing chain file");
        }
        fs::write(src.join(format!("m{total}.rs")), "").expect("writing terminal file");
        fs::write(src.join("main.rs"), "#[path = \"m0.rs\"]\nmod m0;\n")
            .expect("writing crate root");

        let err = resolve(&src.join("main.rs")).expect_err("chain exceeds MAX_MODULE_DEPTH");
        assert!(
            matches!(err, ModuleError::TooDeep { limit, .. } if limit == MAX_MODULE_DEPTH),
            "{err:?}"
        );
    }
}
