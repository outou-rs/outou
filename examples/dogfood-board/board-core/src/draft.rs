//! Form input and its validation.

use crate::task::Priority;

/// Longest accepted title, in characters.
pub const MAX_TITLE_LEN: usize = 80;
/// Longest accepted assignee name, in characters.
pub const MAX_ASSIGNEE_LEN: usize = 40;

/// A reason a draft cannot become a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DraftError {
    /// The title is empty or whitespace.
    TitleEmpty,
    /// The title exceeds [`MAX_TITLE_LEN`].
    TitleTooLong,
    /// The assignee exceeds [`MAX_ASSIGNEE_LEN`].
    AssigneeTooLong,
}

impl DraftError {
    /// Message shown next to the offending field.
    pub fn message(&self) -> String {
        match self {
            DraftError::TitleEmpty => "Title is required.".to_string(),
            DraftError::TitleTooLong => format!("Title must be at most {MAX_TITLE_LEN} characters."),
            DraftError::AssigneeTooLong => {
                format!("Assignee must be at most {MAX_ASSIGNEE_LEN} characters.")
            }
        }
    }
}

/// Raw, unvalidated form input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDraft {
    /// Title as typed.
    pub title: String,
    /// Assignee as typed.
    pub assignee: String,
    /// Chosen priority.
    pub priority: Priority,
    /// Notes as typed.
    pub notes: String,
}

impl Default for TaskDraft {
    fn default() -> Self {
        TaskDraft {
            title: String::new(),
            assignee: String::new(),
            priority: Priority::Medium,
            notes: String::new(),
        }
    }
}

impl TaskDraft {
    /// Returns the trimmed draft, or every problem found (never just the
    /// first, so a form can show all of them at once).
    pub fn validate(&self) -> Result<TaskDraft, Vec<DraftError>> {
        let title = self.title.trim();
        let assignee = self.assignee.trim();
        let mut errors = Vec::new();
        if title.is_empty() {
            errors.push(DraftError::TitleEmpty);
        } else if title.chars().count() > MAX_TITLE_LEN {
            errors.push(DraftError::TitleTooLong);
        }
        if assignee.chars().count() > MAX_ASSIGNEE_LEN {
            errors.push(DraftError::AssigneeTooLong);
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(TaskDraft {
            title: title.to_string(),
            assignee: assignee.to_string(),
            priority: self.priority,
            notes: self.notes.trim().to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(title: &str, assignee: &str) -> TaskDraft {
        TaskDraft {
            title: title.to_string(),
            assignee: assignee.to_string(),
            ..TaskDraft::default()
        }
    }

    #[test]
    fn trims_valid_input() {
        let ok = draft("  Ship it ", " ana ").validate().unwrap();
        assert_eq!(ok.title, "Ship it");
        assert_eq!(ok.assignee, "ana");
    }

    #[test]
    fn rejects_blank_title() {
        assert_eq!(
            draft("   ", "").validate(),
            Err(vec![DraftError::TitleEmpty])
        );
    }

    #[test]
    fn reports_every_problem_at_once() {
        let long = "x".repeat(MAX_ASSIGNEE_LEN + 1);
        assert_eq!(
            draft("", &long).validate(),
            Err(vec![DraftError::TitleEmpty, DraftError::AssigneeTooLong])
        );
    }

    #[test]
    fn counts_characters_not_bytes() {
        let title = "あ".repeat(MAX_TITLE_LEN);
        assert!(draft(&title, "").validate().is_ok());
        let too_long = "あ".repeat(MAX_TITLE_LEN + 1);
        assert_eq!(
            draft(&too_long, "").validate(),
            Err(vec![DraftError::TitleTooLong])
        );
    }
}
