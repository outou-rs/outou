//! Shared "emit" infrastructure for backends: append generated text and
//! record byte-accurate source-map mappings as codegen walks the AST, so
//! that any [`crate::Backend`] can reuse it instead of hand-rolling its own
//! bookkeeping.

use outou_sourcemap::{MappingKind, SourceId, SourceMap, SourceMapBuilder, SourceSpan, Span, Uri};

/// Incrementally builds one generated Rust file and its [`SourceMap`].
///
/// A single [`crate::Backend::generate`] call lowers exactly one `.rsx`
/// source into exactly one generated file (see [`crate::GenerateOptions`]),
/// so a `Writer` tracks exactly one registered [`SourceId`] and every
/// mapping it records refers back to that one source.
#[derive(Debug)]
pub struct Writer {
    output: String,
    // `Option` only so `mapped` can take the builder by value (it consumes
    // `self` to append a mapping) and put an updated one back; it is
    // always `Some` between calls, and `expect`-checked as an internal
    // invariant, never on caller-controlled data.
    builder: Option<SourceMapBuilder>,
    source: SourceId,
}

impl Writer {
    /// Starts a writer for one generated file lowered from one `.rsx`
    /// source file.
    pub fn new(generated_uri: Uri, source_uri: Uri) -> Self {
        let (builder, source) = SourceMapBuilder::new(generated_uri).with_source(source_uri);
        Self {
            output: String::new(),
            builder: Some(builder),
            source,
        }
    }

    /// The [`SourceId`] every mapping produced by this writer refers to.
    pub fn source(&self) -> SourceId {
        self.source
    }

    /// How many bytes have been written to the generated output so far —
    /// the offset the next write will start at.
    pub fn offset(&self) -> u32 {
        self.output.len() as u32
    }

    /// Appends `text` with no source mapping: synthesized code such as
    /// `rsx! {`, a placeholder call, or punctuation the backend writes
    /// itself rather than copying from the `.rsx` source.
    pub fn raw(&mut self, text: &str) -> &mut Self {
        self.output.push_str(text);
        self
    }

    /// Appends `text` and records a mapping from the bytes it occupies in
    /// the generated output back to `sources`, classified as `kind`.
    ///
    /// `sources` is empty for synthesized text with no clear origin span
    /// (still worth mapping, so a reverse lookup can tell "generated code,
    /// no source" from "not part of any mapping at all").
    pub fn mapped(
        &mut self,
        text: &str,
        sources: &[Span],
        kind: MappingKind,
        label: Option<String>,
    ) {
        let start = self.offset();
        self.output.push_str(text);
        let end = self.offset();
        let source_spans: Vec<SourceSpan> = sources
            .iter()
            .map(|span| SourceSpan::new(self.source, *span))
            .collect();
        let builder = self
            .builder
            .take()
            .expect("Writer::builder is always Some between calls");
        let builder = builder
            .try_with_mapping(Span::new(start, end), &source_spans, kind, label)
            .expect(
                "codegen only ever builds well-formed mappings: generated spans come from \
                 Writer::offset (never inverted) and source spans come from the parser's own \
                 spans against the one source this writer was created for",
            );
        self.builder = Some(builder);
    }

    /// Appends `source[span]` verbatim, mapped as `kind` back to that same
    /// span. This is the common case: copying Rust the user already wrote.
    pub fn verbatim(&mut self, source: &str, span: Span, kind: MappingKind, label: Option<String>) {
        let slice = &source[span.start as usize..span.end as usize];
        self.mapped(slice, &[span], kind, label);
    }

    /// Finishes the writer into the generated text and its source map.
    pub fn finish(self) -> (String, SourceMap) {
        let builder = self
            .builder
            .expect("Writer::builder is always Some at finish");
        (self.output, builder.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use outou_sourcemap::MappingKind;

    fn writer() -> Writer {
        Writer::new(Uri::new("file:///g.rs"), Uri::new("file:///a.rsx"))
    }

    #[test]
    fn raw_text_is_not_mapped() {
        let mut w = writer();
        w.raw("hello ");
        let (rust, map) = w.finish();
        assert_eq!(rust, "hello ");
        assert!(map.mappings.is_empty());
    }

    #[test]
    fn verbatim_copies_the_exact_slice_and_maps_it_back() {
        let source = "let x = 1;";
        let mut w = writer();
        w.verbatim(source, Span::new(4, 5), MappingKind::Identifier, None);
        let (rust, map) = w.finish();
        assert_eq!(rust, "x");
        assert_eq!(map.mappings.len(), 1);
        assert_eq!(map.mappings[0].generated, Span::new(0, 1));
        assert_eq!(map.mappings[0].sources[0].span, Span::new(4, 5));
    }

    #[test]
    fn mapped_can_record_zero_or_several_sources() {
        let mut w = writer();
        w.mapped(
            "Greeting",
            &[Span::new(1, 9), Span::new(18, 26)],
            MappingKind::Identifier,
            None,
        );
        let (_rust, map) = w.finish();
        assert_eq!(map.mappings[0].sources.len(), 2);
    }

    #[test]
    fn offset_tracks_bytes_written_so_far() {
        let mut w = writer();
        assert_eq!(w.offset(), 0);
        w.raw("abc");
        assert_eq!(w.offset(), 3);
    }
}
