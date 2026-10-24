use board_core::{Status, Task};
use outou::prelude::*;

use crate::state::{use_app_state, Panel};

#[component]
pub fn TaskList() -> Element {
    let app = use_app_state();
    let board = app.board.get();
    let filter = app.filter.get();
    let selected = app.panel.get();
    let rows: Vec<Task> = board.visible(&filter).into_iter().cloned().collect();

    <section class="list">
        {
            if rows.is_empty() {
                <p class="hint">No tasks match the current filter.</p>
            } else {
                <ul>
                    {
                        rows.into_iter().map(|task| <TaskRow
                            key={task.id}
                            task={task.clone()}
                            selected={selected == Panel::Detail(task.id) || selected == Panel::Edit(task.id)}
                        />)
                    }
                </ul>
            }
        }
    </section>
}

#[component]
fn TaskRow(task: Task, selected: bool) -> Element {
    let app = use_app_state();
    let board = app.board;
    let panel = app.panel;
    let id = task.id;
    let done = task.status == Status::Done;

    <li
        class={
            if selected {
                "row selected"
            } else {
                "row"
            }
        }
    >
        <input
            type="checkbox"
            checked={done}
            onchange={
                move |event| {
                    let next = if event.checked() {
                        Status::Done
                    } else {
                        Status::Todo
                    };
                    board.update(|b| *b = b.set_status(id, next));
                }
            }
        />
        <button class="title" onclick={move |_| panel.set(Panel::Detail(id))}>
            {task.title.clone()}
        </button>
        <span class="meta">{task.status.label()} / {task.priority.label()}</span>
    </li>
}
