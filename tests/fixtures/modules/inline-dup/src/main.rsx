mod a {
    pub mod helper;
}

mod b {
    pub mod helper;
}

/// Returns each inline module's own `helper`'s label, proving `a::helper`
/// and `b::helper` are bound to their own distinct files rather than one
/// clobbering the other's `#[path]` (issue #8 fix list step 2, HIGH-2:
/// a name-only `module_paths` key cannot distinguish two sibling inline
/// modules that happen to declare a same-named child).
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
