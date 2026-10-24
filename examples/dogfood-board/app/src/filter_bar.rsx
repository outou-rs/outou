use board_core::Status;
use outou::prelude::*;

use crate::state::use_app_state;

#[component]
pub fn FilterBar() -> Element {
    let app = use_app_state();
    let filter = app.filter;
    let current = filter.get();
    let board = app.board.get();

    <div class="filter-bar">
        <input
            type="search"
            placeholder="Search title or assignee"
            value={current.query.clone()}
            oninput={move |event| filter.update(|f| f.query = event.value())}
        />
        <div class="chips">
            <button
                class={
                    if current.status.is_none() {
                        "chip active"
                    } else {
                        "chip"
                    }
                }
                onclick={move |_| filter.update(|f| f.status = None)}
            >
                All ({board.tasks().len()})
            </button>
            {Status::ALL.into_iter().map(|status| <button
                key={status.label()}
                class={if current.status == Some(status) { "chip active" } else { "chip" }}
                onclick={move |_| filter.update(|f| f.status = Some(status))}
            >
                {status.label()} ({board.count(status)})
            </button>)}
        </div>
    </div>
}
