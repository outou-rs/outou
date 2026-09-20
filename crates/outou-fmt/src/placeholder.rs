//! Placeholder identifiers substituted for JSX regions before handing
//! text to `rustfmt` (see the crate doc comment and `docs/adr/0011-...`).
//!
//! A placeholder must be a valid Rust identifier (so the surrounding text
//! stays syntactically valid Rust) and must not collide with anything
//! already in the source: a collision could make `rustfmt` reindent the
//! wrong token, or make the later placeholder search find a spurious
//! match. Collision is made impossible, not just unlikely, by checking
//! the whole source text before use and bumping a salt until no
//! collision is found.

/// A source of fresh, source-unique placeholder identifiers for one
/// [`crate::format_source`] call. Every identifier ever handed out by one
/// [`PlaceholderSource`] is guaranteed distinct from the others and from
/// anything in `source`.
pub(crate) struct PlaceholderSource<'a> {
    source: &'a str,
    salt: u64,
    next_index: u64,
}

impl<'a> PlaceholderSource<'a> {
    pub(crate) fn new(source: &'a str) -> Self {
        // The salt only needs to change when the *base* prefix collides
        // with something already in `source` (checked once, not per
        // placeholder): each placeholder is additionally suffixed with
        // its own index, so distinct placeholders from the same source
        // never collide with each other regardless of the salt.
        let mut salt = 0u64;
        while source.contains(&base_prefix(salt)) {
            salt += 1;
        }
        Self {
            source,
            salt,
            next_index: 0,
        }
    }

    /// Returns a fresh placeholder identifier, guaranteed not to appear
    /// anywhere in the original source text.
    pub(crate) fn fresh(&mut self) -> String {
        loop {
            let candidate = format!("{}_{}", base_prefix(self.salt), self.next_index);
            self.next_index += 1;
            if !self.source.contains(&candidate) {
                return candidate;
            }
            // Exceedingly unlikely (the index is unique per call already),
            // but keep the invariant airtight rather than assume it.
            self.salt += 1;
        }
    }
}

fn base_prefix(salt: u64) -> String {
    if salt == 0 {
        "__outou_fmt_jsx".to_string()
    } else {
        format!("__outou_fmt_jsx_s{salt}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_placeholders_are_distinct() {
        let mut source = PlaceholderSource::new("fn f() {}");
        let a = source.fresh();
        let b = source.fresh();
        assert_ne!(a, b);
    }

    #[test]
    fn placeholder_never_collides_with_existing_source_text() {
        // A source that already happens to contain the default prefix
        // forces the salt to bump so every placeholder stays unique.
        let source = "let __outou_fmt_jsx_0 = 1;";
        let mut placeholders = PlaceholderSource::new(source);
        let candidate = placeholders.fresh();
        assert!(!source.contains(&candidate));
    }

    #[test]
    fn placeholder_is_a_valid_rust_identifier_shape() {
        let mut placeholders = PlaceholderSource::new("");
        let candidate = placeholders.fresh();
        assert!(candidate.starts_with("__"));
        assert!(candidate
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_'));
    }
}
