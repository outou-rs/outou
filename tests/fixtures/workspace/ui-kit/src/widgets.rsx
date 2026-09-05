use outou::prelude::*;

mod button;

pub use button::Button;

/// A toolbar containing one `Button`, from the nested `button` module.
#[component]
pub fn Toolbar() -> Element {
    <div class="toolbar">
        <Button label="Toolbar" />
    </div>
}
