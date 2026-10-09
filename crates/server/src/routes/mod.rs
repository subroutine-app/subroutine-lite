use axum::Router;

use crate::state::AppState;

mod account;
mod actions;
mod data;
mod events;
mod markers;
mod mutations;
mod pipeline;
mod routines;
mod signals;
mod sse;
pub(crate) mod validation;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(account::router())
        .merge(data::router())
        .merge(actions::router())
        .merge(events::router())
        .merge(markers::router())
        .merge(mutations::router())
        .merge(signals::router())
        .merge(routines::router())
        .merge(pipeline::router())
        .merge(sse::router())
}
