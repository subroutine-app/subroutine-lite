use axum::{
    Json, Router,
    extract::Query,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;

use crate::{auth::Tenant, db, error::Result, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new().route("/data", get(all))
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DataQuery {
    since: Option<i64>,
}

async fn all(Tenant(state): Tenant, Query(query): Query<DataQuery>) -> Result<Response> {
    if let Some(since) = query.since {
        return Ok(Json(state.data_delta(since).await?).into_response());
    }

    Ok(Json(db::all_data(state.scope()).await?).into_response())
}
