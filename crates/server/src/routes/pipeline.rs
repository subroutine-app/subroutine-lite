use axum::{Json, Router, routing::post};

use subroutine_core::Action;

use crate::{auth::Tenant, error::Result, ops, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/pipeline/refresh", post(refresh))
        .route("/pipeline/expedite", post(expedite))
}

async fn refresh(Tenant(state): Tenant) -> Result<Json<Vec<Action>>> {
    Ok(Json(
        state
            .apply_from_snapshot(|snapshot| Ok(ops::pipeline::refresh(snapshot)?))
            .await?,
    ))
}

async fn expedite(Tenant(state): Tenant) -> Result<Json<Vec<Action>>> {
    Ok(Json(
        state
            .apply_from_snapshot(|snapshot| Ok(ops::pipeline::expedite(snapshot)?))
            .await?,
    ))
}
