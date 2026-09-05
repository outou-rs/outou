mod a {
    pub mod helper;
}

mod b {
    pub mod helper;
}

/// Same shape as `inline-dup`, but `b::helper` is plain Rust (`.rs`) while
/// `a::helper` is `.rsx` — the exact shape that, under a name-only
/// `module_paths` key, silently rebound `a::helper`'s own `#[path]` at
/// `b::helper`'s file instead (issue #8 fix list step 2, N2).
pub fn labels() -> (&'static str, &'static str) {
    (a::helper::LABEL, b::helper::LABEL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_inline_sibling_binds_to_its_own_file() {
        assert_eq!(labels(), ("a", "b"));
    }
}
