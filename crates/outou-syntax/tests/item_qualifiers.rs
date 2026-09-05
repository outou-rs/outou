//! A visibility/qualifier keyword (`pub`, `pub(crate)`, `const`, `async`,
//! `unsafe`, `default`, `extern "abi"`) between an item's attributes and
//! its `fn`/`mod` keyword must not break attribute association (HIGH-1,
//! issue #4 fix list item 2). The repo's own example component,
//! `examples/phase0-app/src/components.rsx`, is `#[component]\npub fn
//! UserCard(...)`, so this is a real Gate 1 regression, not a corner case.

use std::fs;
use std::path::{Path, PathBuf};

use outou_syntax::ast;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/outou-syntax has a parent")
        .parent()
        .expect("crates/ has a parent")
        .to_path_buf()
}

fn find_function<'a>(file: &'a ast::File, name: &str) -> Option<&'a ast::Function> {
    file.items.iter().find_map(|item| match item {
        ast::Item::Function(f) if f.name.name == name => Some(f),
        _ => None,
    })
}

fn find_module<'a>(file: &'a ast::File, name: &str) -> Option<&'a ast::Module> {
    file.items.iter().find_map(|item| match item {
        ast::Item::Module(m) if m.name.name == name => Some(m),
        _ => None,
    })
}

#[test]
fn pub_fn_with_component_attribute_is_recognized() {
    let source = "#[component]\npub fn UserCard(user: User) -> Element {\n    <div/>\n}";
    let parsed = outou_syntax::parse(source);
    let function = find_function(&parsed.file, "UserCard")
        .unwrap_or_else(|| panic!("no `UserCard` function item found: {:#?}", parsed.file));
    assert!(function.is_component, "{:#?}", function);
    assert_eq!(function.attributes.len(), 1, "{:?}", function.attributes);
    assert_eq!(function.attributes[0].text, "#[component]");
    // The whole item span, including the attribute, starts at `#`.
    assert_eq!(function.span.start, 0);
    assert_eq!(
        &source[function.span.start as usize..function.span.start as usize + 1],
        "#"
    );
}

#[test]
fn pub_paren_crate_mod_with_path_attribute_is_recognized() {
    let source = "#[path = \"x.rs\"]\npub(crate) mod m;";
    let parsed = outou_syntax::parse(source);
    let module = find_module(&parsed.file, "m")
        .unwrap_or_else(|| panic!("no `m` module item found: {:#?}", parsed.file));
    assert_eq!(module.path.as_deref(), Some("x.rs"));
    assert_eq!(module.attributes.len(), 1, "{:?}", module.attributes);
}

#[test]
fn pub_mod_with_cfg_attribute_preserves_the_attribute() {
    let source = "#[cfg(test)]\npub mod m {\n    fn f() {}\n}";
    let parsed = outou_syntax::parse(source);
    let module = find_module(&parsed.file, "m")
        .unwrap_or_else(|| panic!("no `m` module item found: {:#?}", parsed.file));
    assert_eq!(module.attributes.len(), 1, "{:?}", module.attributes);
    assert_eq!(module.attributes[0].text, "#[cfg(test)]");
    assert!(module.items.is_some());
}

#[test]
fn async_unsafe_const_default_extern_qualifiers_are_skipped() {
    for source in [
        "#[component] pub const fn f() -> Element { <a/> }",
        "#[component] pub async fn f() -> Element { <a/> }",
        "#[component] pub unsafe fn f() -> Element { <a/> }",
        "#[component] default fn f() -> Element { <a/> }",
        "#[component] pub extern \"C\" fn f() -> Element { <a/> }",
    ] {
        let parsed = outou_syntax::parse(source);
        let function = find_function(&parsed.file, "f").unwrap_or_else(|| {
            panic!(
                "no `f` function item found for {source:?}: {:#?}",
                parsed.file
            )
        });
        assert!(function.is_component, "source: {source}: {:#?}", function);
    }
}

#[test]
fn extern_block_is_not_mistaken_for_a_function_item() {
    let source = "extern \"C\" { fn foo(); }";
    let parsed = outou_syntax::parse(source);
    assert!(
        find_function(&parsed.file, "foo").is_none(),
        "extern block's `fn foo()` must not become its own Item::Function: {:#?}",
        parsed.file
    );
    let has_rust_item = parsed
        .file
        .items
        .iter()
        .any(|item| matches!(item, ast::Item::Rust(_)));
    assert!(has_rust_item, "{:#?}", parsed.file);
}

#[test]
fn impl_trait_for_fn_type_is_not_mistaken_for_a_function_item() {
    let source = "impl Trait for fn() {}";
    let parsed = outou_syntax::parse(source);
    assert!(
        parsed
            .file
            .items
            .iter()
            .all(|item| !matches!(item, ast::Item::Function(_))),
        "{:#?}",
        parsed.file
    );
}

#[test]
fn doc_comment_directly_above_an_attribute_is_recorded() {
    // LOW-9, issue #4 fix list item 8: `try_attribute` is trivia-tolerant
    // (it looks ahead past comments to find `#`), which used to make
    // `scan_leading_attributes` skip straight from the doc comment to the
    // `#[component]` behind it, silently dropping the doc comment from
    // `Function.attributes`.
    let source = "/// A user card.\n#[component]\nfn UserCard() -> Element { <a/> }";
    let parsed = outou_syntax::parse(source);
    let function = find_function(&parsed.file, "UserCard")
        .unwrap_or_else(|| panic!("no `UserCard` function item found: {:#?}", parsed.file));
    assert_eq!(function.attributes.len(), 2, "{:?}", function.attributes);
    assert_eq!(function.attributes[0].text, "/// A user card.");
    assert_eq!(function.attributes[1].text, "#[component]");
    assert!(function.is_component);
}

#[test]
fn examples_components_fixture_keeps_its_component_attribute() {
    let path = repo_root()
        .join("examples")
        .join("phase0-app")
        .join("src")
        .join("components.rsx");
    let source =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let parsed = outou_syntax::parse(&source);
    let function = find_function(&parsed.file, "UserCard")
        .unwrap_or_else(|| panic!("no `UserCard` function item found: {:#?}", parsed.file));
    assert!(function.is_component, "{:#?}", function);
    assert_eq!(function.attributes.len(), 1, "{:?}", function.attributes);
    assert_eq!(function.attributes[0].text, "#[component]");
    // The whole item span (including the attribute) must reach all the
    // way to `#`: nothing before it (e.g. `use outou::prelude::*;`) was
    // wrongly folded into this function's own leading prefix.
    let span_start = function.span.start as usize;
    let signature_start = function.signature.span.start as usize;
    assert_eq!(&source[span_start..signature_start].trim_start()[..1], "#");
}
