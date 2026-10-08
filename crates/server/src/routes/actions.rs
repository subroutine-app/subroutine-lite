use axum::{
    Json, Router,
    extract::Path,
    http::StatusCode,
    routing::{get, post, put},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use uuid::Uuid;

use subroutine_core::{Action, ActionTemplate, BatchPlacement, ChangeEvent, CompleteResult};

use crate::{
    auth::Tenant,
    db,
    error::Result,
    ops::{self, Delete},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/actions", get(list).post(create))
        .route("/actions/batch", post(batch))
        .route("/actions/{id}", get(one).put(update).delete(trash))
        .route("/actions/{id}/queue", post(queue))
        .route("/actions/{id}/backlog", post(backlog))
        .route("/actions/{id}/pin", post(pin))
        .route("/actions/{id}/save", post(save))
        .route("/actions/{id}/complete", post(complete))
        .route("/actions/{id}/clear_duration", post(clear_duration))
        .route(
            "/actions/templates",
            get(list_templates).post(create_template),
        )
        .route("/actions/templates/order", put(reorder_templates))
        .route(
            "/actions/templates/{id}",
            get(one_template)
                .put(update_template)
                .delete(trash_template),
        )
}

async fn list(Tenant(state): Tenant) -> Result<Json<Vec<Action>>> {
    Ok(Json(db::fetch_all::<Action>(state.scope()).await?))
}

async fn create(
    Tenant(state): Tenant,
    Json(action): Json<Action>,
) -> Result<(StatusCode, Json<Action>)> {
    let action = state.apply(ops::actions::create(action)).await?;
    Ok((StatusCode::CREATED, Json(action)))
}

async fn one(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<Json<Action>> {
    Ok(Json(state.require::<Action>("action", id).await?))
}

async fn update(
    Tenant(state): Tenant,
    Path(id): Path<Uuid>,
    Json(action): Json<Action>,
) -> Result<Json<Action>> {
    ops::validate_update("action", id, &action)?;
    Ok(Json(
        state
            .apply_optional::<Action, _, _>(id, |previous| {
                Ok(ops::actions::update(previous.as_ref(), action))
            })
            .await?,
    ))
}

async fn trash(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<StatusCode> {
    state.trash(Delete::Action(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn queue(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<Json<Vec<Action>>> {
    Ok(Json(
        state
            .apply_from_snapshot(|snapshot| Ok(ops::actions::queue(snapshot, id)?))
            .await?,
    ))
}

#[derive(Debug, Deserialize)]
struct BatchRequest {
    action: Action,
    #[serde(default)]
    cursor: Option<DateTime<Utc>>,
}

async fn batch(
    Tenant(state): Tenant,
    Json(body): Json<BatchRequest>,
) -> Result<Json<BatchPlacement>> {
    Ok(Json(
        state
            .apply_from_snapshot(|snapshot| {
                Ok(ops::actions::batch(snapshot, body.action, body.cursor))
            })
            .await?,
    ))
}

async fn backlog(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<Json<Action>> {
    Ok(Json(
        state
            .apply_required::<Action, _, _>("action", id, |action| {
                Ok(ops::actions::backlog(action))
            })
            .await?,
    ))
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PinRequest {
    pinned: bool,
}

async fn pin(
    Tenant(state): Tenant,
    Path(id): Path<Uuid>,
    Json(body): Json<PinRequest>,
) -> Result<Json<Action>> {
    Ok(Json(
        state
            .apply_required::<Action, _, _>("action", id, |action| {
                Ok(ops::actions::set_pinned(action, body.pinned)?)
            })
            .await?,
    ))
}

async fn save(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<Json<ActionTemplate>> {
    Ok(Json(
        state
            .apply_required::<Action, _, _>("action", id, |action| {
                Ok(ops::save_as_template(&action))
            })
            .await?,
    ))
}

async fn complete(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<Json<CompleteResult>> {
    Ok(Json(
        state
            .apply_required::<Action, _, _>("action", id, |action| {
                Ok(ops::actions::complete(action, Utc::now()))
            })
            .await?,
    ))
}

async fn clear_duration(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<Json<Action>> {
    Ok(Json(
        state
            .apply_required::<Action, _, _>("action", id, |action| {
                Ok(ops::actions::clear_duration(action))
            })
            .await?,
    ))
}

async fn list_templates(Tenant(state): Tenant) -> Result<Json<Vec<ActionTemplate>>> {
    Ok(Json(db::fetch_all::<ActionTemplate>(state.scope()).await?))
}

async fn create_template(
    Tenant(state): Tenant,
    Json(template): Json<ActionTemplate>,
) -> Result<(StatusCode, Json<ActionTemplate>)> {
    let template = state.apply(ops::create(template)).await?;
    Ok((StatusCode::CREATED, Json(template)))
}

async fn one_template(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<Json<ActionTemplate>> {
    Ok(Json(
        state
            .require::<ActionTemplate>("action template", id)
            .await?,
    ))
}

async fn reorder_templates(
    Tenant(state): Tenant,
    Json(ids): Json<Vec<Uuid>>,
) -> Result<StatusCode> {
    let Some(seq) = db::actions::reorder_templates(state.scope(), &ids).await? else {
        return Err(crate::error::AppError::bad_request(
            "action template order must contain every active template exactly once".into(),
        ));
    };
    state.announce(seq, ChangeEvent::ActionTemplatesChanged);
    Ok(StatusCode::NO_CONTENT)
}

async fn update_template(
    Tenant(state): Tenant,
    Path(id): Path<Uuid>,
    Json(template): Json<ActionTemplate>,
) -> Result<Json<ActionTemplate>> {
    ops::validate_update("action template", id, &template)?;
    let Some(db::Sequenced {
        value: template,
        seq,
    }) = db::actions::update_template_preserving_order(state.scope(), id, template).await?
    else {
        return Err(crate::error::AppError::not_found(format!(
            "action template {id} not found"
        )));
    };
    state.announce(seq, ChangeEvent::ActionTemplatesChanged);
    Ok(Json(template))
}

async fn trash_template(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<StatusCode> {
    state.trash(Delete::ActionTemplate(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}
