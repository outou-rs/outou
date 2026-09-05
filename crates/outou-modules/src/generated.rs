//! Generated-name disambiguation for sibling modules that share a logical
//! name (issue #7 decision 3), e.g. two `cfg`-exclusive `mod imp;`
//! declarations under the same parent (the std-style unix/windows idiom,
//! which rustc accepts — `#[cfg]` is never evaluated, ADR 0006 — so it
//! must not be rejected here either).

use std::collections::HashMap;

/// Tracks how many times each (already unraw'd) sibling name has been
/// used for a generated segment so far, within one parent scope. A fresh
/// instance is created per call to `resolve_items` (one per set of
/// siblings, whether declared inline or as separate files), so
/// disambiguation never crosses into an unrelated part of the tree.
#[derive(Debug, Default)]
pub(crate) struct GeneratedNames {
    counts: HashMap<String, usize>,
}

impl GeneratedNames {
    /// The generated segment for the next sibling named `unraw_name`: the
    /// name itself the first time it is seen in this scope, `{name}-{n}`
    /// (n = 1, 2, …) for every later sibling sharing that name, in source
    /// order.
    pub(crate) fn next(&mut self, unraw_name: &str) -> String {
        let count = self.counts.entry(unraw_name.to_string()).or_insert(0);
        let segment = if *count == 0 {
            unraw_name.to_string()
        } else {
            format!("{unraw_name}-{count}")
        };
        *count += 1;
        segment
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_occurrence_keeps_the_plain_name() {
        let mut names = GeneratedNames::default();
        assert_eq!(names.next("imp"), "imp");
    }

    #[test]
    fn later_occurrences_get_a_numeric_suffix_in_source_order() {
        let mut names = GeneratedNames::default();
        assert_eq!(names.next("imp"), "imp");
        assert_eq!(names.next("imp"), "imp-1");
        assert_eq!(names.next("imp"), "imp-2");
    }

    #[test]
    fn distinct_names_do_not_affect_each_other() {
        let mut names = GeneratedNames::default();
        assert_eq!(names.next("imp"), "imp");
        assert_eq!(names.next("other"), "other");
        assert_eq!(names.next("imp"), "imp-1");
    }
}
