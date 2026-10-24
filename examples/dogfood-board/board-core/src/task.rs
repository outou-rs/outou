//! The task record and its two closed vocabularies.

/// Where a task is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Not started.
    Todo,
    /// Being worked on.
    InProgress,
    /// Finished.
    Done,
}

impl Status {
    /// Every status, in board order.
    pub const ALL: [Status; 3] = [Status::Todo, Status::InProgress, Status::Done];

    /// Human label, also used as the form/select value.
    pub fn label(self) -> &'static str {
        match self {
            Status::Todo => "To do",
            Status::InProgress => "In progress",
            Status::Done => "Done",
        }
    }

    /// Parses the value produced by [`Status::label`].
    pub fn from_label(label: &str) -> Option<Status> {
        Status::ALL.into_iter().find(|s| s.label() == label)
    }
}

/// How urgent a task is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    /// Can wait.
    Low,
    /// The default.
    Medium,
    /// Do first.
    High,
}

impl Priority {
    /// Every priority, lowest first.
    pub const ALL: [Priority; 3] = [Priority::Low, Priority::Medium, Priority::High];

    /// Human label, also used as the form/select value.
    pub fn label(self) -> &'static str {
        match self {
            Priority::Low => "Low",
            Priority::Medium => "Medium",
            Priority::High => "High",
        }
    }

    /// Parses the value produced by [`Priority::label`].
    pub fn from_label(label: &str) -> Option<Priority> {
        Priority::ALL.into_iter().find(|p| p.label() == label)
    }
}

/// One task on the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// Stable identifier assigned by the board.
    pub id: u32,
    /// Short summary. Never empty.
    pub title: String,
    /// Who owns it; empty means unassigned.
    pub assignee: String,
    /// Urgency.
    pub priority: Priority,
    /// Progress.
    pub status: Status,
    /// Free-form details.
    pub notes: String,
}
