use outou::prelude::*;

mod filter_bar;
mod state;
mod task_detail;
mod task_form;
mod task_list;

use filter_bar::FilterBar;
use state::{provide_state, Panel};
use task_detail::TaskDetail;
use task_form::TaskForm;
use task_list::TaskList;

#[component]
fn App() -> Element {
    let app = provide_state();
    let panel = app.panel.get();
    let open_new = app.panel;

    <main class="board">
        <header>
            <h1>Task board</h1>
            <button class="primary" onclick={move |_| open_new.set(Panel::New)}>New task</button>
        </header>
        <FilterBar />
        <div class="layout">
            <TaskList />
            <section class="side">
                {
                    match panel {
                        Panel::Empty => <p class="hint">Select a task, or create a new one.</p>,
                        Panel::Detail(id) => <TaskDetail id={id} />,
                        Panel::New => <TaskForm key="new" editing={None} />,
                        Panel::Edit(id) => <TaskForm key={format!("edit-{id}")} editing={Some(id)} />,
                    }
                }
            </section>
        </div>
    </main>
}

fn main() {
    launch(App);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use board_core::{Filter, Status};
    use dioxus_core::{AttributeValue, Mutation, VirtualDom};
    use std::cell::RefCell;

    /// Renders a component once with no renderer attached and returns the
    /// `selected` flags of its `<option>` elements, in document order.
    fn selected_flags(root: fn() -> Element) -> Vec<bool> {
        let mut dom = VirtualDom::new(root);
        dom.rebuild_to_vec()
            .edits
            .into_iter()
            .filter_map(|edit| match edit {
                Mutation::SetAttribute {
                    name: "selected",
                    value: AttributeValue::Bool(flag),
                    ..
                } => Some(flag),
                _ => None,
            })
            .collect()
    }

    #[component]
    fn FormForHighPriorityTask() -> Element {
        provide_state();
        <TaskForm editing={Some(1)} />
    }

    #[component]
    fn FormForNewTask() -> Element {
        provide_state();
        <TaskForm editing={None} />
    }

    #[component]
    fn DetailForTodoTask() -> Element {
        provide_state();
        <TaskDetail id={1} />
    }

    /// Renders the whole component tree once with no renderer attached: it
    /// proves context, signals and the initial branch evaluate without
    /// panicking. It cannot click, so the interactive flows are covered only
    /// by the manual walkthrough in README.md.
    #[test]
    fn first_render_succeeds() {
        let mut dom = VirtualDom::new(App);
        dom.rebuild_in_place();
    }

    /// F18: the select must show the task's value on a fresh mount. The
    /// source only writes `<select value={..}>`; the lowering (issue #28)
    /// sets `selected` on the matching option, which is what these tests read.
    #[test]
    fn edit_form_marks_the_tasks_priority_option_selected() {
        // Priority::ALL is Low, Medium, High; task 1 is High.
        assert_eq!(
            selected_flags(FormForHighPriorityTask),
            [false, false, true]
        );
    }

    #[test]
    fn new_task_form_marks_the_default_priority_option_selected() {
        // Priority::ALL is Low, Medium, High; a new draft is Medium.
        assert_eq!(selected_flags(FormForNewTask), [false, true, false]);
    }

    #[test]
    fn detail_view_marks_the_tasks_status_option_selected() {
        // Status::ALL is To do, In progress, Done; task 1 is To do.
        assert_eq!(selected_flags(DetailForTodoTask), [true, false, false]);
    }

    thread_local! {
        static STATE: RefCell<Option<AppState>> = const { RefCell::new(None) };
    }

    #[component]
    fn ListWithState() -> Element {
        let app = provide_state();
        STATE.with(|cell| *cell.borrow_mut() = Some(app));
        <TaskList />
    }

    fn texts(edits: Vec<Mutation>) -> Vec<String> {
        edits
            .into_iter()
            .filter_map(|edit| match edit {
                Mutation::CreateTextNode { value, .. } | Mutation::SetText { value, .. } => {
                    Some(value)
                }
                _ => None,
            })
            .collect()
    }

    /// The state API re-renders the list: writing the filter state, as an
    /// event handler does, changes what `TaskList` shows on the next render.
    #[test]
    fn writing_the_filter_state_re_renders_the_list() {
        let mut dom = VirtualDom::new(ListWithState);
        let first = texts(dom.rebuild_to_vec().edits);
        assert!(first.iter().any(|text| text == "Renew the office lease"));

        let app = STATE.with(|cell| cell.borrow().expect("the component ran"));
        dom.in_runtime(|| app.filter.update(|f| f.query = "zzz".to_string()));
        // No task matches: the list is replaced by the static hint template.
        let hidden = dom.render_immediate_to_vec().edits;
        assert!(
            hidden
                .iter()
                .any(|edit| matches!(edit, Mutation::LoadTemplate { .. })),
            "{hidden:?}"
        );

        dom.in_runtime(|| app.filter.set(Filter::default()));
        dom.in_runtime(|| app.board.update(|b| *b = b.set_status(1, Status::Done)));
        dom.in_runtime(|| app.filter.update(|f| f.status = Some(Status::Done)));
        let done = texts(dom.render_immediate_to_vec().edits);
        assert!(
            done.iter().any(|text| text == "Write the quarterly report"),
            "{done:?}"
        );
        assert!(
            !done.iter().any(|text| text == "Renew the office lease"),
            "{done:?}"
        );
    }
}
