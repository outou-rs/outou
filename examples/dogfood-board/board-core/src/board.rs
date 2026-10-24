//! The immutable board and its filter.

use crate::draft::{DraftError, TaskDraft};
use crate::task::{Status, Task};

/// Which tasks are visible.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Filter {
    /// Only this status; `None` shows all.
    pub status: Option<Status>,
    /// Case-insensitive substring of title or assignee; empty matches all.
    pub query: String,
}

impl Filter {
    /// Whether `task` passes this filter.
    pub fn matches(&self, task: &Task) -> bool {
        if self.status.is_some_and(|s| s != task.status) {
            return false;
        }
        let query = self.query.trim().to_lowercase();
        query.is_empty()
            || task.title.to_lowercase().contains(&query)
            || task.assignee.to_lowercase().contains(&query)
    }
}

/// All tasks. Every operation returns a new board.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Board {
    tasks: Vec<Task>,
    next_id: u32,
}

impl Board {
    /// A board with a few tasks, so the app is not empty on first load.
    pub fn sample() -> Board {
        let mut board = Board::default();
        let seeds = [
            ("Write the quarterly report", "ana", "High", "Numbers are in the shared sheet."),
            ("Renew the office lease", "", "Medium", ""),
            ("Replace the meeting-room display", "ken", "Low", "Waiting on a quote."),
        ];
        for (title, assignee, priority, notes) in seeds {
            let draft = TaskDraft {
                title: title.to_string(),
                assignee: assignee.to_string(),
                priority: crate::Priority::from_label(priority).unwrap_or(crate::Priority::Medium),
                notes: notes.to_string(),
            };
            if let Ok(next) = board.add(&draft) {
                board = next;
            }
        }
        board
    }

    /// Every task, in insertion order.
    pub fn tasks(&self) -> &[Task] {
        &self.tasks
    }

    /// The task with `id`, if any.
    pub fn get(&self, id: u32) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    /// Tasks passing `filter`, in insertion order.
    pub fn visible(&self, filter: &Filter) -> Vec<&Task> {
        self.tasks.iter().filter(|t| filter.matches(t)).collect()
    }

    /// How many tasks have `status`.
    pub fn count(&self, status: Status) -> usize {
        self.tasks.iter().filter(|t| t.status == status).count()
    }

    /// Adds a task from a draft; new tasks start as [`Status::Todo`].
    pub fn add(&self, draft: &TaskDraft) -> Result<Board, Vec<DraftError>> {
        let valid = draft.validate()?;
        let task = Task {
            id: self.next_id + 1,
            title: valid.title,
            assignee: valid.assignee,
            priority: valid.priority,
            status: Status::Todo,
            notes: valid.notes,
        };
        let tasks = self.tasks.iter().cloned().chain(Some(task)).collect();
        Ok(Board {
            tasks,
            next_id: self.next_id + 1,
        })
    }

    /// Replaces the editable fields of task `id`. An unknown id leaves the
    /// board unchanged.
    pub fn update(&self, id: u32, draft: &TaskDraft) -> Result<Board, Vec<DraftError>> {
        let valid = draft.validate()?;
        Ok(self.map_task(id, |task| Task {
            title: valid.title.clone(),
            assignee: valid.assignee.clone(),
            priority: valid.priority,
            notes: valid.notes.clone(),
            ..task.clone()
        }))
    }

    /// Moves task `id` to `status`.
    pub fn set_status(&self, id: u32, status: Status) -> Board {
        self.map_task(id, |task| Task {
            status,
            ..task.clone()
        })
    }

    /// Removes task `id`.
    pub fn remove(&self, id: u32) -> Board {
        Board {
            tasks: self.tasks.iter().filter(|t| t.id != id).cloned().collect(),
            next_id: self.next_id,
        }
    }

    fn map_task(&self, id: u32, change: impl Fn(&Task) -> Task) -> Board {
        Board {
            tasks: self
                .tasks
                .iter()
                .map(|t| if t.id == id { change(t) } else { t.clone() })
                .collect(),
            next_id: self.next_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(title: &str) -> TaskDraft {
        TaskDraft {
            title: title.to_string(),
            ..TaskDraft::default()
        }
    }

    #[test]
    fn add_assigns_increasing_ids_and_never_reuses_after_remove() {
        let board = Board::default().add(&draft("a")).unwrap();
        let board = board.add(&draft("b")).unwrap();
        let board = board.remove(2).add(&draft("c")).unwrap();
        let ids: Vec<u32> = board.tasks().iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![1, 3]);
    }

    #[test]
    fn operations_do_not_mutate_the_original() {
        let before = Board::default().add(&draft("a")).unwrap();
        let after = before.set_status(1, Status::Done);
        assert_eq!(before.get(1).unwrap().status, Status::Todo);
        assert_eq!(after.get(1).unwrap().status, Status::Done);
    }

    #[test]
    fn update_validates_and_keeps_status() {
        let board = Board::default()
            .add(&draft("a"))
            .unwrap()
            .set_status(1, Status::InProgress);
        assert!(board.update(1, &draft("  ")).is_err());
        let board = board.update(1, &draft("renamed")).unwrap();
        let task = board.get(1).unwrap();
        assert_eq!((task.title.as_str(), task.status), ("renamed", Status::InProgress));
    }

    #[test]
    fn filter_combines_status_and_query() {
        let board = Board::sample().set_status(1, Status::Done);
        let all = Filter::default();
        assert_eq!(board.visible(&all).len(), 3);
        let done = Filter {
            status: Some(Status::Done),
            ..Filter::default()
        };
        assert_eq!(board.visible(&done).len(), 1);
        let by_assignee = Filter {
            status: None,
            query: " KEN ".to_string(),
        };
        assert_eq!(board.visible(&by_assignee)[0].id, 3);
    }
}
