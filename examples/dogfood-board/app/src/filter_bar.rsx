use board_core::{Filter, Status};
use outou::prelude::*;

use crate::state::use_app_state;

#[component]
pub fn FilterBar() -> Element {
    let app = use_app_state();
    let mut filter = app.filter;
    let current = filter.read().clone();
    let board = app.board.read().clone();

    <div class="filter-bar">
        <input
            type="search"
            placeholder="Search title or assignee"
            value={current.query.clone()}
            oninput={
                move |event| {
                    let next = Filter {
                        query: event.value(),
                        ..filter.read().clone()
                    };
                    filter.set(next);
                }
            }
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
                onclick={
                    move |_| {
                        let next = Filter {
                            status: None,
                            ..filter.read().clone()
                        };
                        filter.set(next);
                    }
                }
            >
                All ({board.tasks().len()})
            </button>
            {Status::ALL.into_iter().map(|status| <button
                key={status.label()}
                class={if current.status == Some(status) { "chip active" } else { "chip" }}
                onclick={move |_| {
                    let next = Filter { status: Some(status), ..filter.read().clone() };
                    filter.set(next);
                }}
            >
                {status.label()} ({board.count(status)})
            </button>)}
        </div>
    </div>
}
