use axum::{
    Json, Router,
    extract::Path,
    http::StatusCode,
    routing::{get, post},
};
use uuid::Uuid;

use subroutine_core::{Marker, MarkerTemplate};

use crate::{
    auth::Tenant,
    db,
    error::Result,
    ops::{self, Delete},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/markers", get(list).post(create))
        .route("/markers/{id}", get(one).put(update).delete(trash))
        .route("/markers/{id}/save", post(save))
        .route(
            "/markers/templates",
            get(list_templates).post(create_template),
        )
        .route(
            "/markers/templates/{id}",
            get(one_template)
                .put(update_template)
                .delete(trash_template),
        )
}

async fn list(Tenant(state): Tenant) -> Result<Json<Vec<Marker>>> {
    Ok(Json(db::fetch_all::<Marker>(state.scope()).await?))
}

async fn create(
    Tenant(state): Tenant,
    Json(marker): Json<Marker>,
) -> Result<(StatusCode, Json<Marker>)> {
    let marker = state.apply(ops::create(marker)).await?;
    Ok((StatusCode::CREATED, Json(marker)))
}

async fn one(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<Json<Marker>> {
    Ok(Json(state.require::<Marker>("marker", id).await?))
}

async fn update(
    Tenant(state): Tenant,
    Path(id): Path<Uuid>,
    Json(marker): Json<Marker>,
) -> Result<Json<Marker>> {
    ops::validate_update("marker", id, &marker)?;
    Ok(Json(state.apply(ops::put(marker)).await?))
}

async fn trash(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<StatusCode> {
    state.trash(Delete::Marker(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn save(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<Json<MarkerTemplate>> {
    Ok(Json(
        state
            .apply_required::<Marker, _, _>("marker", id, |marker| {
                Ok(ops::save_as_template(&marker))
            })
            .await?,
    ))
}

async fn list_templates(Tenant(state): Tenant) -> Result<Json<Vec<MarkerTemplate>>> {
    Ok(Json(db::fetch_all::<MarkerTemplate>(state.scope()).await?))
}

async fn create_template(
    Tenant(state): Tenant,
    Json(template): Json<MarkerTemplate>,
) -> Result<(StatusCode, Json<MarkerTemplate>)> {
    let template = state.apply(ops::create(template)).await?;
    Ok((StatusCode::CREATED, Json(template)))
}

async fn one_template(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<Json<MarkerTemplate>> {
    Ok(Json(
        state
            .require::<MarkerTemplate>("marker template", id)
            .await?,
    ))
}

async fn update_template(
    Tenant(state): Tenant,
    Path(id): Path<Uuid>,
    Json(template): Json<MarkerTemplate>,
) -> Result<Json<MarkerTemplate>> {
    ops::validate_update("marker template", id, &template)?;
    let outcome = ops::put(template);
    Ok(Json(state.apply(outcome).await?))
}

async fn trash_template(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<StatusCode> {
    state.trash(Delete::MarkerTemplate(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}
