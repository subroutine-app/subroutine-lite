use axum::{
    Json, Router,
    extract::Path,
    http::StatusCode,
    routing::{get, post, put},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use uuid::Uuid;

use subroutine_core::{Action, ChangeEvent, Routine, RoutineStep};

use crate::{
    auth::Tenant,
    db,
    error::Result,
    ops::{self, Delete},
    state::{AppState, TenantState},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/routines", get(list).post(create))
        .route("/routines/order", put(reorder))
        .route("/routines/{id}/steps", put(replace_steps))
        .route("/routines/{id}", get(one).put(update).delete(trash))
        .route("/routines/{id}/instantiate", post(instantiate))
}

async fn list(Tenant(state): Tenant) -> Result<Json<Vec<Routine>>> {
    Ok(Json(db::routines::fetch_all(state.scope()).await?))
}

async fn create(
    Tenant(state): Tenant,
    Json(routine): Json<Routine>,
) -> Result<(StatusCode, Json<Routine>)> {
    let routine = state.apply(ops::routines::create(routine)).await?;
    Ok((StatusCode::CREATED, Json(routine)))
}

async fn reorder(Tenant(state): Tenant, Json(ids): Json<Vec<Uuid>>) -> Result<StatusCode> {
    let Some(seq) = db::routines::reorder(state.scope(), &ids).await? else {
        return Err(crate::error::AppError::bad_request(
            "routine order must contain every active routine exactly once".into(),
        ));
    };
    state.announce(seq, ChangeEvent::RoutinesChanged);
    Ok(StatusCode::NO_CONTENT)
}

async fn replace_steps(
    Tenant(state): Tenant,
    Path(id): Path<Uuid>,
    Json(steps): Json<Vec<RoutineStep>>,
) -> Result<StatusCode> {
    let Some(seq) = db::routines::replace_steps(state.scope(), id, &steps).await? else {
        return Err(crate::error::AppError::not_found(format!(
            "routine {id} not found"
        )));
    };
    state.announce(seq, ChangeEvent::RoutinesChanged);
    Ok(StatusCode::NO_CONTENT)
}

async fn one(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<Json<Routine>> {
    Ok(Json(require(&state, id).await?))
}

async fn update(
    Tenant(state): Tenant,
    Path(id): Path<Uuid>,
    Json(routine): Json<Routine>,
) -> Result<Json<Routine>> {
    ops::validate_update("routine", id, &routine)?;
    Ok(Json(state.apply(ops::put(routine)).await?))
}

async fn trash(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<StatusCode> {
    state.trash(Delete::Routine(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct InstantiateRequest {
    start_time: Option<DateTime<Utc>>,
}

async fn instantiate(
    Tenant(state): Tenant,
    Path(id): Path<Uuid>,
    Json(body): Json<InstantiateRequest>,
) -> Result<Json<Vec<Action>>> {
    Ok(Json(
        state
            .apply_from_snapshot(|snapshot| {
                let routine = snapshot
                    .routines
                    .iter()
                    .find(|routine| routine.id == id)
                    .ok_or_else(|| {
                        crate::error::AppError::not_found(format!("routine {id} not found"))
                    })?;
                Ok(ops::routines::instantiate(
                    snapshot,
                    routine,
                    body.start_time,
                ))
            })
            .await?,
    ))
}

async fn require(state: &TenantState, id: Uuid) -> Result<Routine> {
    db::routines::fetch_by_id(state.scope(), id)
        .await?
        .ok_or_else(|| crate::error::AppError::not_found(format!("routine {id} not found")))
}
