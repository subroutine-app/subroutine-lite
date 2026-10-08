use axum::{
    Json, Router,
    extract::Path,
    http::StatusCode,
    routing::{get, post},
};
use uuid::Uuid;

use subroutine_core::{Signal, SignalTemplate};

use crate::{
    auth::Tenant,
    db,
    error::Result,
    ops::{self, Delete},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/signals", get(list).post(create))
        .route("/signals/{id}", get(one).put(update).delete(trash))
        .route("/signals/{id}/save", post(save))
        .route(
            "/signals/templates",
            get(list_templates).post(create_template),
        )
        .route(
            "/signals/templates/{id}",
            get(one_template)
                .put(update_template)
                .delete(trash_template),
        )
}

async fn list(Tenant(state): Tenant) -> Result<Json<Vec<Signal>>> {
    Ok(Json(db::fetch_all::<Signal>(state.scope()).await?))
}

async fn create(
    Tenant(state): Tenant,
    Json(signal): Json<Signal>,
) -> Result<(StatusCode, Json<Signal>)> {
    let signal = state.apply(ops::create(signal)).await?;
    Ok((StatusCode::CREATED, Json(signal)))
}

async fn one(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<Json<Signal>> {
    Ok(Json(state.require::<Signal>("signal", id).await?))
}

async fn update(
    Tenant(state): Tenant,
    Path(id): Path<Uuid>,
    Json(signal): Json<Signal>,
) -> Result<Json<Signal>> {
    ops::validate_update("signal", id, &signal)?;
    Ok(Json(state.apply(ops::put(signal)).await?))
}

async fn trash(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<StatusCode> {
    state.trash(Delete::Signal(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn save(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<Json<SignalTemplate>> {
    Ok(Json(
        state
            .apply_required::<Signal, _, _>("signal", id, |signal| {
                Ok(ops::save_as_template(&signal))
            })
            .await?,
    ))
}

async fn list_templates(Tenant(state): Tenant) -> Result<Json<Vec<SignalTemplate>>> {
    Ok(Json(db::fetch_all::<SignalTemplate>(state.scope()).await?))
}

async fn create_template(
    Tenant(state): Tenant,
    Json(template): Json<SignalTemplate>,
) -> Result<(StatusCode, Json<SignalTemplate>)> {
    let template = state.apply(ops::create(template)).await?;
    Ok((StatusCode::CREATED, Json(template)))
}

async fn one_template(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<Json<SignalTemplate>> {
    Ok(Json(
        state
            .require::<SignalTemplate>("signal template", id)
            .await?,
    ))
}

async fn update_template(
    Tenant(state): Tenant,
    Path(id): Path<Uuid>,
    Json(template): Json<SignalTemplate>,
) -> Result<Json<SignalTemplate>> {
    ops::validate_update("signal template", id, &template)?;
    let outcome = ops::put(template);
    Ok(Json(state.apply(outcome).await?))
}

async fn trash_template(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<StatusCode> {
    state.trash(Delete::SignalTemplate(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}
