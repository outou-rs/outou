use board_core::{DraftError, Priority, TaskDraft};
use outou::prelude::*;

use crate::state::{use_app_state, Panel};

/// Create (`editing == None`) or edit (`Some(id)`) a task. Local form state
/// starts from the task being edited; it is dropped when the panel changes
/// (the caller keys this component per target).
#[component]
pub fn TaskForm(editing: Option<u32>) -> Element {
    let app = use_app_state();
    let mut board = app.board;
    let mut panel = app.panel;
    let initial = editing
        .and_then(|id| app.board.read().get(id).cloned())
        .map(|task| TaskDraft {
            title: task.title,
            assignee: task.assignee,
            priority: task.priority,
            notes: task.notes,
        })
        .unwrap_or_default();

    let mut draft = use_signal(|| initial);
    let mut errors = use_signal(Vec::<DraftError>::new);
    let current = draft.read().clone();
    let heading = if editing.is_some() {
        "Edit task"
    } else {
        "New task"
    };
    let title_error = errors
        .read()
        .iter()
        .find(|e| matches!(e, DraftError::TitleEmpty | DraftError::TitleTooLong))
        .map(DraftError::message);
    let assignee_error = errors
        .read()
        .iter()
        .find(|e| matches!(e, DraftError::AssigneeTooLong))
        .map(DraftError::message);

    <form
        class="form"
        onsubmit={
            move |event| {
                event.prevent_default();
                let result = match editing {
                    Some(id) => board
                        .read()
                        .update(id, &draft.read())
                        .map(|next| (next, id)),
                    None => board.read().add(&draft.read()).map(|next| {
                        let id = next.tasks().last().map_or(0, |task| task.id);
                        (next, id)
                    }),
                };
                match result {
                    Ok((next, id)) => {
                        board.set(next);
                        errors.set(Vec::new());
                        panel.set(Panel::Detail(id));
                    }
                    Err(found) => errors.set(found),
                }
            }
        }
    >
        <h2>{heading}</h2>
        <label>
            Title
            <input
                value={current.title.clone()}
                oninput={
                    move |event| {
                        let next = TaskDraft {
                            title: event.value(),
                            ..draft.read().clone()
                        };
                        draft.set(next);
                    }
                }
            />
        </label>
        {title_error.map(|message| <p class="error" role="alert">{message}</p>)}
        <label>
            Assignee
            <input
                value={current.assignee.clone()}
                oninput={
                    move |event| {
                        let next = TaskDraft {
                            assignee: event.value(),
                            ..draft.read().clone()
                        };
                        draft.set(next);
                    }
                }
            />
        </label>
        {assignee_error.map(|message| <p class="error" role="alert">{message}</p>)}
        <label>
            Priority
            <select
                value={current.priority.label()}
                onchange={
                    move |event| {
                        if let Some(priority) = Priority::from_label(&event.value()) {
                            let next = TaskDraft {
                                priority,
                                ..draft.read().clone()
                            };
                            draft.set(next);
                        }
                    }
                }
            >
                {
                    Priority::ALL.into_iter().map(|priority| <option key={priority.label()} value={priority.label()}>{priority.label()}</option>)
                }
            </select>
        </label>
        <label>
            Notes
            <textarea
                value={current.notes.clone()}
                oninput={
                    move |event| {
                        let next = TaskDraft {
                            notes: event.value(),
                            ..draft.read().clone()
                        };
                        draft.set(next);
                    }
                }
            >
            </textarea>
        </label>
        <div class="actions">
            <button type="submit" class="primary">Save</button>
            <button
                type="button"
                onclick={
                    move |_| {
                        panel.set(match editing {
                            Some(id) => Panel::Detail(id),
                            None => Panel::Empty,
                        })
                    }
                }
            >
                Cancel
            </button>
        </div>
    </form>
}
