//! Performance regressions that must not come back (issue #4 fix list,
//! should-fix items). These assert that `parse` *returns* within a
//! generous, CI-safe bound rather than pinning an exact duration, so the
//! test stays robust on slow CI runners while still catching an
//! accidental return to quadratic (or worse) behavior.

use std::time::{Duration, Instant};

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
