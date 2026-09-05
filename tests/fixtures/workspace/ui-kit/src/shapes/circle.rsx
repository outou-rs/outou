use outou::prelude::*;

/// Reached from the crate root through `#[path = "shapes/circle.rsx"]
/// mod circle;`: the module's name (`circle`) does not match its source
/// file's directory (`shapes/`), and its *generated* path still follows
/// the module path convention (`src/.generated/circle.rs`), not the
/// source layout (`crates/outou-modules/README.md`).
#[component]
pub fn Circle(radius: f64) -> Element {
    <div class="circle">{radius.to_string()}</div>
}
