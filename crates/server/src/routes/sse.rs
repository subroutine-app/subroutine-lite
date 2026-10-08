use axum::{
    Router,
    http::HeaderMap,
    response::{Sse, sse::Event},
    routing::get,
};
use futures_util::{StreamExt, stream};
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;

use subroutine_core::ChangeBatch;

use crate::{
    auth::{Tenant, TokenDeadline},
    error::Result,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/changes/stream", get(changes_stream))
}

async fn changes_stream(
    Tenant(state): Tenant,
    deadline: TokenDeadline,
    headers: HeaderMap,
) -> Result<Sse<impl futures_util::Stream<Item = std::result::Result<Event, Infallible>>>> {
    let user_id = state.scope().user_id;
    let rx = state.subscribe_changes();
    let current_seq = state.current_change_seq().await?;
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok());
    let initial = (last_event_id != Some(current_seq)).then(|| ChangeBatch::reset(current_seq));

    let initial = stream::iter(initial.into_iter().filter_map(event_frame).map(Ok));
    let updates = BroadcastStream::new(rx).filter_map(move |result| {
        let state = state.clone();
        async move {
            match result {
                Ok(change) => change.batch_for(user_id).and_then(event_frame).map(Ok),
                Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n)) => {
                    tracing::warn!(%user_id, "SSE client lagged, dropped {n} change events");
                    match state.current_change_seq().await {
                        Ok(seq) => event_frame(ChangeBatch::reset(seq)).map(Ok),
                        Err(error) => {
                            tracing::error!(%user_id, ?error, "failed to read sequence after SSE lag");
                            None
                        }
                    }
                }
            }
        }
    });

    let stream = initial
        .chain(updates)
        .take_until(tokio::time::sleep_until(deadline.0));
    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    ))
}

fn event_frame(batch: ChangeBatch) -> Option<Event> {
    let id = batch.seq.to_string();
    match serde_json::to_string(&batch) {
        Ok(json) => Some(Event::default().id(id).data(json)),
        Err(error) => {
            tracing::error!(?error, "failed to serialize SSE change batch");
            None
        }
    }
}
