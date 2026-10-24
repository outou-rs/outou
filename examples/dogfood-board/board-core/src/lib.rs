//! Domain model for the dogfood task board: tasks, validated drafts, an
//! immutable board and a filter. Plain Rust with no UI dependency, so the
//! app crate (written in `.rsx`) is the only place that knows about
//! rendering.

mod board;
mod draft;
mod task;

pub use board::{Board, Filter};
pub use draft::{DraftError, TaskDraft, MAX_ASSIGNEE_LEN, MAX_TITLE_LEN};
pub use task::{Priority, Status, Task};
