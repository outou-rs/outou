//! Source-map tests (issue #6, deliverable 3): a component's identifier is
//! written once in the generated Rust but appears at both its opening and
//! closing tag in the `.rsx` source, so the one generated mapping must
//! carry both source spans, and `SourceMap::map_range` on that generated
//! span must return both.

use outou_backend_dioxus::DioxusBackend;
use outou_codegen::{Backend, GenerateOptions, Mode};
use outou_sourcemap::{MappingKind, Uri};

#[test]
fn greeting_identifier_maps_back_to_both_tag_spans() {
    let source = "fn App() -> Element {\n    <Greeting>Hello</Greeting>\n}\n";
    let parsed = outou_syntax::parse(source);
    let opts = GenerateOptions::new(Uri::new("file:///gen.rs"), Uri::new("file:///a.rsx"));
    let generated = DioxusBackend
        .generate(&parsed, source, Mode::Strict, &opts)
        .expect("well-formed source generates");

    let open_start = source.find("Greeting>").unwrap();
    let identifier_mapping = generated
        .source_map
        .mappings
        .iter()
        .find(|mapping| {
            mapping.kind == MappingKind::Identifier
                && mapping.sources.len() == 2
                && mapping
                    .sources
                    .iter()
                    .any(|s| (s.span.start as usize) == open_start)
        })
        .unwrap_or_else(|| {
            panic!(
                "no two-source identifier mapping in {:#?}",
                generated.source_map
            )
        });

    assert_eq!(
        &generated.rust[identifier_mapping.generated.start as usize
            ..identifier_mapping.generated.end as usize],
        "Greeting"
    );

    let mapped = generated.source_map.map_range(identifier_mapping.generated);
    assert!(!mapped.unmapped);
    assert_eq!(mapped.sources.len(), 2, "{mapped:#?}");
}

#[test]
fn text_child_is_mapped_as_text_kind() {
    let source = "fn App() -> Element {\n    <p>Hello</p>\n}\n";
    let parsed = outou_syntax::parse(source);
    let opts = GenerateOptions::new(Uri::new("file:///gen.rs"), Uri::new("file:///a.rsx"));
    let generated = DioxusBackend
        .generate(&parsed, source, Mode::Strict, &opts)
        .expect("well-formed source generates");

    assert!(generated
        .source_map
        .mappings
        .iter()
        .any(|mapping| mapping.kind == MappingKind::Text));
}

#[test]
fn rsx_invocation_site_maps_back_to_the_element_span() {
    // MEDIUM-13, issue #6 fix list item 8: Dioxus reports macro-level
    // errors at the macro invocation site
    // (`::outou::__private::rsx! { `), which was previously written with
    // `Writer::raw` and so entirely unmapped — a reverse lookup from that
    // position had nothing to return. Mapping the prefix (and the
    // closing ` }`) to the element's own span lets a macro-site error
    // land back on the JSX element instead of nowhere.
    let source = "fn App() -> Element {\n    <div>Hello</div>\n}\n";
    let parsed = outou_syntax::parse(source);
    let opts = GenerateOptions::new(Uri::new("file:///gen.rs"), Uri::new("file:///a.rsx"));
    let generated = DioxusBackend
        .generate(&parsed, source, Mode::Strict, &opts)
        .expect("well-formed source generates");

    let element_start = source.find("<div>").unwrap() as u32;
    let prefix = "::outou::__private::rsx! { ";
    let prefix_start = generated
        .rust
        .find(prefix)
        .expect("generated output contains the rsx! invocation prefix");
    let prefix_span =
        outou_sourcemap::Span::new(prefix_start as u32, (prefix_start + prefix.len()) as u32);

    let mapped = generated.source_map.map_range(prefix_span);
    assert!(!mapped.unmapped, "{mapped:#?}");
    assert!(
        mapped.sources.iter().any(|s| s.span.start == element_start),
        "{mapped:#?}"
    );
}

#[test]
fn attribute_name_is_mapped_as_attribute_kind() {
    let source = "fn App() -> Element {\n    <div class=\"x\" />\n}\n";
    let parsed = outou_syntax::parse(source);
    let opts = GenerateOptions::new(Uri::new("file:///gen.rs"), Uri::new("file:///a.rsx"));
    let generated = DioxusBackend
        .generate(&parsed, source, Mode::Strict, &opts)
        .expect("well-formed source generates");

    assert!(generated
        .source_map
        .mappings
        .iter()
        .any(|mapping| mapping.kind == MappingKind::Attribute));
}

fn generate_strict(source: &str) -> outou_codegen::Generated {
    let parsed = outou_syntax::parse(source);
    let opts = GenerateOptions::new(Uri::new("file:///gen.rs"), Uri::new("file:///a.rsx"));
    DioxusBackend
        .generate(&parsed, source, Mode::Strict, &opts)
        .expect("well-formed source generates")
}

#[test]
fn fragment_tags_have_no_mapping_of_their_own_and_children_keep_theirs() {
    // ADR 0015 (ADR 0007: a source span may map to zero generated spans):
    // `<>` and `</>` produce no generated text of their own. The only
    // mapping that covers them is the call site, the whole fragment.
    let source =
        "fn App() -> Element {\n    <>\n        <p>one</p>\n        <q>two</q>\n    </>\n}\n";
    let generated = generate_strict(source);
    let fragment_start = source.find("<>").unwrap() as u32;
    let fragment_end = (source.find("</>").unwrap() + "</>".len()) as u32;
    for mapping in &generated.source_map.mappings {
        for source_span in mapping.sources.iter().map(|s| s.span) {
            let inside = source_span.start >= fragment_start && source_span.end <= fragment_end;
            let is_tag = &source[source_span.start as usize..source_span.end as usize];
            assert!(
                !inside || (is_tag != "<>" && is_tag != "</>"),
                "a fragment tag must not be a mapping source: {mapping:#?}"
            );
        }
    }
    let call_sites: Vec<_> = generated
        .source_map
        .mappings
        .iter()
        .filter(|m| {
            m.sources
                .iter()
                .any(|s| s.span.start == fragment_start && s.span.end == fragment_end)
        })
        .collect();
    assert_eq!(
        call_sites.len(),
        2,
        "the `rsx!` prefix and its closing brace"
    );
    for name in ["p", "q"] {
        let start = source.find(&format!("<{name}>")).unwrap() as u32 + 1;
        assert!(
            generated.source_map.mappings.iter().any(|m| {
                m.kind == MappingKind::Identifier
                    && m.sources.iter().any(|s| s.span.start == start)
                    && &generated.rust[m.generated.start as usize..m.generated.end as usize] == name
            }),
            "<{name}> keeps its identifier mapping"
        );
    }
}

#[test]
fn a_spliced_fragment_child_maps_each_of_its_children() {
    let source = "fn App() -> Element {\n    <ul><><li>a</li><li>b</li></></ul>\n}\n";
    let generated = generate_strict(source);
    assert!(
        generated
            .rust
            .contains("ul { li { \"a\", }, li { \"b\", }, }"),
        "{}",
        generated.rust
    );
    let texts = generated
        .source_map
        .mappings
        .iter()
        .filter(|m| m.kind == MappingKind::Text)
        .count();
    assert_eq!(texts, 2);
}
