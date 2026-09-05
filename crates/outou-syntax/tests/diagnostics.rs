//! Diagnostic *span* precision (as opposed to `tests/reserved.rs`'s
//! diagnostic *message* precision): M4 — when a closing tag matches an
//! ancestor several levels up, every frame popped above the match must
//! report its own "missing closing tag" at *its own* opening `<`, not at
//! the closing tag that triggered the unwind.

use outou_sourcemap::Span;

#[test]
fn ancestor_close_reports_missing_tags_at_their_openings() {
    let source = "fn f() { <A><B><C></A> }";
    // Byte offsets (verified against `source`):
    //   <A  starts at 9
    //   <B  starts at 12
    //   <C  starts at 15
    //  </A> starts at 18
    assert_eq!(&source[9..10], "<");
    assert_eq!(&source[12..13], "<");
    assert_eq!(&source[15..16], "<");
    assert_eq!(&source[18..19], "<");

    let parsed = outou_syntax::parse(source);
    let diagnostics: Vec<(String, Span)> = parsed
        .diagnostics
        .iter()
        .map(|d| (d.message.clone(), d.span))
        .collect();

    assert_eq!(
        diagnostics,
        vec![
            ("missing closing tag `</C>`".to_string(), Span::new(15, 16),),
            ("missing closing tag `</B>`".to_string(), Span::new(12, 13),),
        ],
        "diagnostics: {diagnostics:?}"
    );
}
