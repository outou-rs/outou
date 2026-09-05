//! A plain Rust sibling module, declared from the `.rsx` crate root
//! (`mod util;` in `lib.rsx`) — the direction Phase 0 always supports,
//! unlike a `.rs` file declaring a `.rsx` child (`crates/outou-cli/README.md`).

pub fn double(x: i32) -> i32 {
    x * 2
}

#[cfg(test)]
mod tests {
    #[test]
    fn doubles() {
        assert_eq!(super::double(3), 6);
    }
}
