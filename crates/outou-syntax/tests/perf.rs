//! Performance regressions that must not come back (issue #4 fix list,
//! should-fix items). These assert that `parse` *returns* within a
//! generous, CI-safe bound rather than pinning an exact duration, so the
//! test stays robust on slow CI runners while still catching an
//! accidental return to quadratic (or worse) behavior.

use std::time::{Duration, Instant};

/// Times `parse` on `source`, discarding the result but keeping the parser
/// from being optimized away by asserting on something cheap about it.
fn time_parse(source: &str) -> Duration {
    let start = Instant::now();
    let parsed = outou_syntax::parse(source);
    let elapsed = start.elapsed();
    assert!(!parsed.file.items.is_empty());
    elapsed
}

/// Asserts that doubling `source`'s size does not roughly quadruple parse
/// time (HIGH-3, issue #4 fix list item 3): `try_skip_opaque_region` and
/// `scan_leading_attributes` both call `rust_token::next_significant`,
/// which walks past an entire run of comments/attributes to find the next
/// real token. Attempting either at *every* position inside such a run,
/// rather than once for the whole run, made parsing quadratic in the run's
/// length. A ratio below 3 (not the ~4x a quadratic algorithm would show)
/// leaves generous slack for a slow or noisy CI runner while still failing
/// hard on a return to quadratic behavior.
fn assert_roughly_linear(label: &str, build: impl Fn(usize) -> String, n: usize) {
    let small = build(n);
    let large = build(n * 2);
    // Warm up (page faults, allocator warm-up) before the timed runs, and
    // take the best of a few timed runs on each side to reduce scheduler
    // noise — this is a coarse wall-clock check, not a benchmark.
    let _ = time_parse(&small);
    let small_elapsed = (0..3).map(|_| time_parse(&small)).min().unwrap();
    let large_elapsed = (0..3).map(|_| time_parse(&large)).min().unwrap();

    eprintln!(
        "{label}: {n} lines = {small_elapsed:?}, {} lines = {large_elapsed:?}",
        n * 2
    );

    // Guard against a small-side measurement so close to zero that the
    // ratio is dominated by timer noise rather than the algorithm.
    if small_elapsed < Duration::from_micros(200) {
        return;
    }
    let ratio = large_elapsed.as_secs_f64() / small_elapsed.as_secs_f64();
    assert!(
        ratio < 3.0,
        "{label}: doubling from {n} to {} lines took {small_elapsed:?} -> {large_elapsed:?} \
         (ratio {ratio:.2}); expected roughly linear time",
        n * 2
    );
}

/// M12: `try_macro_invocation` used to re-walk the remaining path from
/// every segment start looking for a trailing `!`, making a long plain
/// path (no macro at all) quadratic in its segment count. Item 17 makes
/// that attempt O(1) per segment (skipped whenever the previous
/// significant token is already part of a path: an identifier, a raw
/// identifier, or `::`), so the whole scan is linear.
#[test]
fn long_path_is_linear() {
    let mut source = String::from("fn f() { let _x = a");
    for _ in 0..100_000 {
        source.push_str("::a");
    }
    source.push_str("; }");

    let start = Instant::now();
    let parsed = outou_syntax::parse(&source);
    let elapsed = start.elapsed();

    assert!(!parsed.file.items.is_empty());
    assert!(
        elapsed < Duration::from_secs(5),
        "parsing a 100_000-segment path took {elapsed:?}; expected roughly linear time"
    );
}

/// HIGH-3's main repro: a long run of `///` doc-comment lines at item
/// level, not immediately followed by a `fn`/`mod` item — the shape a
/// half-typed doc header above a `struct`/`const`/anything else produces
/// while an IDE feature (e.g. a language server) reparses on every
/// keystroke.
#[test]
fn doc_comment_run_not_preceding_fn_or_mod_is_linear() {
    fn build(lines: usize) -> String {
        let mut source = String::from("fn f() {}\n");
        for _ in 0..lines {
            source.push_str("/// a doc comment line\n");
        }
        source.push_str("struct S;\n");
        source
    }
    assert_roughly_linear("doc_comment_run", build, 2000);
}

/// The same shape, but with an ordinary `#[cfg(test)]`-style attribute
/// repeated instead of a doc comment.
#[test]
fn attribute_run_not_preceding_fn_or_mod_is_linear() {
    fn build(lines: usize) -> String {
        let mut source = String::new();
        for _ in 0..lines {
            source.push_str("#[cfg(test)]\n");
        }
        source.push_str("struct S;\n");
        source
    }
    assert_roughly_linear("attribute_run", build, 2000);
}

/// HIGH-3's other repro: a long run of block comments *inside a function
/// body*, exercised through `parser::region::Parser::scan_region` rather
/// than item-level scanning.
#[test]
fn block_comment_run_inside_a_function_body_is_linear() {
    fn build(lines: usize) -> String {
        let mut source = String::from("fn f() {\n");
        for _ in 0..lines {
            source.push_str("/* a block comment */\n");
        }
        source.push_str("}\n");
        source
    }
    assert_roughly_linear("block_comment_in_fn_body", build, 4000);
}

/// A large, otherwise ordinary doc-comment header directly above the `fn`
/// it documents must also stay fast — this is the common case (not the
/// "doesn't precede an item" repro above), exercising
/// `scan_leading_attributes` collecting the whole run in one call.
#[test]
fn doc_comment_header_directly_above_fn_is_linear() {
    fn build(lines: usize) -> String {
        let mut source = String::new();
        for _ in 0..lines {
            source.push_str("/// a doc comment line\n");
        }
        source.push_str("#[component]\nfn f() -> Element { <a/> }\n");
        source
    }
    assert_roughly_linear("doc_comment_header", build, 2000);
}
