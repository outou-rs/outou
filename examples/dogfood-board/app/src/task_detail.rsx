use board_core::Status;
use outou::prelude::*;

use crate::state::{use_app_state, Panel};

#[component]
pub fn TaskDetail(id: u32) -> Element {
    let app = use_app_state();
    let board = app.board;
    let panel = app.panel;
    let found = app.board.with(|b| b.get(id).cloned());

    {
        match found {
            None => <p class="hint">This task no longer exists.</p>,
            Some(task) => <article class="detail">
                <h2>{task.title.clone()}</h2>
                <dl>
                    <dt>Assignee</dt>
                    <dd>
                        {
                            if task.assignee.is_empty() {
                                "Unassigned".to_string()
                            } else {
                                task.assignee.clone()
                            }
                        }
                    </dd>
                    <dt>Priority</dt>
                    <dd>{task.priority.label()}</dd>
                    <dt>Notes</dt>
                    <dd>
                        {
                            if task.notes.is_empty() {
                                "No notes".to_string()
                            } else {
                                task.notes.clone()
                            }
                        }
                    </dd>
                </dl>
                <label>
                    Status
                    <select
                        value={task.status.label()}
                        onchange={
                            move |event| {
                                if let Some(status) = Status::from_label(&event.value()) {
                                    board.update(|b| *b = b.set_status(id, status));
                                }
                            }
                        }
                    >
                        {
                            Status::ALL.into_iter().map(|status| <option key={status.label()} value={status.label()}>{status.label()}</option>)
                        }
                    </select>
                </label>
                <div class="actions">
                    <button onclick={move |_| panel.set(Panel::Edit(id))}>Edit</button>
                    <button
                        class="danger"
                        onclick={
                            move |_| {
                                board.update(|b| *b = b.remove(id));
                                panel.set(Panel::Empty);
                            }
                        }
                    >
                        Delete
                    </button>
                </div>
            </article>,
        }
    }
}
