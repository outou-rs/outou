use outou::prelude::*;

mod widgets;
#[cfg(feature = "extra")]
mod extra;
#[path = "shapes/circle.rsx"]
mod circle;
mod util;

pub use circle::Circle;
pub use widgets::Button;

/// Adds one to `x`.
///
/// A plain Rust doc test (no JSX): `.rsx` files preserve doc comments and
/// their fenced code blocks verbatim through codegen, so this runs under
/// `cargo test --doc -p ui-kit` exactly as if it had been written in a
/// `.rs` file.
///
/// ```
/// assert_eq!(ui_kit::add_one(1), 2);
/// ```
pub fn add_one(x: i32) -> i32 {
    x + 1
}

/// A card built from two other components, one of them (`Button`) from a
/// nested `.rsx` module and one (`Circle`) reached through an explicit
/// `#[path]` into a differently named source file.
#[component]
pub fn Card(title: String) -> Element {
    <div class="card">
        <h2>{title}</h2>
        <Circle radius={util::double(1) as f64} />
        <Button label="OK" />
    </div>
}

#[cfg(test)]
mod tests {
    #[test]
    fn plain_rust_tests_still_run_in_a_library_crate() {
        assert_eq!(super::add_one(1), 2);
    }
}
