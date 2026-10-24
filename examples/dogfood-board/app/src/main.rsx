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
    let panel = *app.panel.read();
    let mut open_new = app.panel;

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
    use dioxus_core::{AttributeValue, Mutation, VirtualDom};

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

    /// F18: the select must show the task's value on a fresh mount, which
    /// the backend only does through `selected` on the matching option.
    #[test]
    fn edit_form_marks_the_tasks_priority_option_selected() {
        // Priority::ALL is Low, Medium, High; task 1 is High.
        assert_eq!(
            selected_flags(FormForHighPriorityTask),
            [false, false, true]
        );
    }

    #[test]
    fn detail_view_marks_the_tasks_status_option_selected() {
        // Status::ALL is To do, In progress, Done; task 1 is To do.
        assert_eq!(selected_flags(DetailForTodoTask), [true, false, false]);
    }
}
