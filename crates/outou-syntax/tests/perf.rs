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

/// Growth factor between the small and the large input.
const SIZE_FACTOR: usize = 4;
/// Timed runs per side; the minimum is kept, because load can only ever make
/// a run slower, never faster than the true cost.
const TIMED_RUNS: usize = 7;
/// Linear growth shows a ratio near `SIZE_FACTOR` (4); quadratic growth shows
/// near `SIZE_FACTOR^2` (16). 10 sits well clear of both, so scheduler noise
/// on the best-of-k does not trip it and a quadratic regression still does.
const MAX_RATIO: f64 = 10.0;

/// Asserts that making `source` four times larger does not make parse time
/// grow by anywhere near 16x (HIGH-3, issue #4 fix list item 3):
/// `try_skip_opaque_region` and `scan_leading_attributes` both call
/// `rust_token::next_significant`, which walks past an entire run of
/// comments/attributes to find the next real token. Attempting either at
/// *every* position inside such a run, rather than once for the whole run,
/// made parsing quadratic in the run's length. This is a coarse wall-clock
/// check, not a benchmark.
fn assert_roughly_linear(label: &str, build: impl Fn(usize) -> String, n: usize) {
    let small = build(n);
    let large = build(n * SIZE_FACTOR);
    // Warm up (page faults, allocator warm-up) before the timed runs.
    let _ = time_parse(&small);
    let small_elapsed = best_of(TIMED_RUNS, || time_parse(&small));
    let large_elapsed = best_of(TIMED_RUNS, || time_parse(&large));
    eprintln!(
        "{label}: {n} lines = {small_elapsed:?}, {} lines = {large_elapsed:?}",
        n * SIZE_FACTOR
    );
    assert_ratio_is_linear(label, n, small_elapsed, large_elapsed);
}

/// The minimum of `runs` measurements.
fn best_of(runs: usize, mut measure: impl FnMut() -> Duration) -> Duration {
    (0..runs).map(|_| measure()).min().unwrap()
}

fn assert_ratio_is_linear(label: &str, n: usize, small: Duration, large: Duration) {
    // Guard against a small-side measurement so close to zero that the
    // ratio is dominated by timer noise rather than the algorithm.
    if small < Duration::from_micros(200) {
        eprintln!("{label}: small side {small:?} below the noise floor, ratio not checked");
        return;
    }
    let ratio = large.as_secs_f64() / small.as_secs_f64();
    assert!(
        ratio < MAX_RATIO,
        "{label}: growing from {n} to {} lines took {small:?} -> {large:?} \
         (ratio {ratio:.2}); expected roughly linear time (about {SIZE_FACTOR}x)",
        n * SIZE_FACTOR
    );
}

/// The rule must accept noisy linear growth and reject quadratic growth.
#[test]
fn the_ratio_rule_accepts_noisy_linear_growth() {
    let small = Duration::from_millis(1);
    assert_ratio_is_linear("linear", 1, small, small * 4);
    assert_ratio_is_linear("noisy linear", 1, small, small * 7);
}

#[test]
#[should_panic(expected = "expected roughly linear time")]
fn the_ratio_rule_rejects_quadratic_growth() {
    // A 4x input costing 16x is what a quadratic regression looks like.
    let small = Duration::from_millis(1);
    assert_ratio_is_linear("quadratic", 1, small, small * 16);
}

/// The limit is a ratio of 10: just under passes, just over fails.
#[test]
fn the_ratio_rule_accepts_a_ratio_just_under_the_limit() {
    let small = Duration::from_millis(1);
    assert_ratio_is_linear("just under", 1, small, Duration::from_micros(9_900));
}

#[test]
#[should_panic(expected = "expected roughly linear time")]
fn the_ratio_rule_rejects_a_ratio_just_over_the_limit() {
    let small = Duration::from_millis(1);
    assert_ratio_is_linear("just over", 1, small, Duration::from_micros(10_100));
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
    assert_roughly_linear("doc_comment_run", build, 4000);
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
    assert_roughly_linear("attribute_run", build, 4000);
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
    assert_roughly_linear("doc_comment_header", build, 4000);
}
