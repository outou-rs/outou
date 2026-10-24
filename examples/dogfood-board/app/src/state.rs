//! Shared application state, provided once by `App` and read by every
//! component through context. Plain Rust: no JSX in this file.

use board_core::{Board, Filter};
use outou::prelude::*;

/// What the side panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    /// Nothing selected.
    Empty,
    /// Read-only view of one task.
    Detail(u32),
    /// The "new task" form.
    New,
    /// The "edit task" form.
    Edit(u32),
}

/// The three pieces of state the components share.
#[derive(Clone, Copy)]
pub struct AppState {
    /// All tasks.
    pub board: State<Board>,
    /// Current list filter.
    pub filter: State<Filter>,
    /// Current side panel.
    pub panel: State<Panel>,
}

/// Creates the state and makes it available to descendants. Call once, in
/// the root component.
pub fn provide_state() -> AppState {
    let state = AppState {
        board: use_state(Board::sample),
        filter: use_state(Filter::default),
        panel: use_state(|| Panel::Empty),
    };
    provide_context(|| state)
}

/// The state provided by [`provide_state`].
pub fn use_app_state() -> AppState {
    use_context::<AppState>()
}
