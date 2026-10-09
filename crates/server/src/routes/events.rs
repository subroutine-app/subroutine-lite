use axum::{
    Json, Router,
    extract::Path,
    http::StatusCode,
    routing::{get, post, put},
};
use uuid::Uuid;

use subroutine_core::{ChangeEvent, ConvertEventToMarker, Event, EventTemplate, Marker};

use crate::{
    auth::Tenant,
    db,
    error::{AppError, Result},
    ops::{self, Delete},
    state::AppState,
};

use super::validation;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/events", get(list).post(create))
        .route("/events/{id}", get(one).put(update).delete(trash))
        .route("/events/{id}/save", post(save))
        .route("/events/{id}/convert-to-marker", post(convert_to_marker))
        .route(
            "/events/templates",
            get(list_templates).post(create_template),
        )
        .route("/events/templates/order", put(reorder_templates))
        .route(
            "/events/templates/{id}",
            get(one_template)
                .put(update_template)
                .delete(trash_template),
        )
}

async fn list(Tenant(state): Tenant) -> Result<Json<Vec<Event>>> {
    Ok(Json(db::fetch_all::<Event>(state.scope()).await?))
}

async fn create(
    Tenant(state): Tenant,
    Json(mut event): Json<Event>,
) -> Result<(StatusCode, Json<Event>)> {
    validation::event(&event).map_err(AppError::bad_request)?;
    ops::Identify::ensure_id(&mut event);
    let event = state
        .apply_optional::<Event, _, _>(event.id, |previous| {
            event.preserve_missing_availability(previous.as_ref());
            Ok(ops::put(event))
        })
        .await?;
    Ok((StatusCode::CREATED, Json(event)))
}

async fn one(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<Json<Event>> {
    Ok(Json(state.require::<Event>("event", id).await?))
}

async fn update(
    Tenant(state): Tenant,
    Path(id): Path<Uuid>,
    Json(mut event): Json<Event>,
) -> Result<Json<Event>> {
    ops::validate_update("event", id, &event)?;
    validation::event(&event).map_err(AppError::bad_request)?;
    Ok(Json(
        state
            .apply_optional::<Event, _, _>(id, |previous| {
                event.preserve_missing_availability(previous.as_ref());
                Ok(ops::put(event))
            })
            .await?,
    ))
}

async fn trash(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<StatusCode> {
    state.trash(Delete::Event(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn save(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<Json<EventTemplate>> {
    Ok(Json(
        state
            .apply_required::<Event, _, _>("event", id, |event| Ok(ops::save_as_template(&event)))
            .await?,
    ))
}

async fn convert_to_marker(
    Tenant(state): Tenant,
    Path(id): Path<Uuid>,
    Json(request): Json<ConvertEventToMarker>,
) -> Result<Json<Marker>> {
    Ok(Json(
        state
            .apply_required::<Event, _, _>("event", id, |event| {
                Ok(ops::convert_event_to_marker(
                    &event,
                    request.date,
                    request.local_end_date,
                )?)
            })
            .await?,
    ))
}

async fn list_templates(Tenant(state): Tenant) -> Result<Json<Vec<EventTemplate>>> {
    Ok(Json(db::fetch_all::<EventTemplate>(state.scope()).await?))
}

async fn create_template(
    Tenant(state): Tenant,
    Json(mut template): Json<EventTemplate>,
) -> Result<(StatusCode, Json<EventTemplate>)> {
    validation::duration(subroutine_core::SchedulePoint::now(), template.duration)
        .map_err(AppError::bad_request)?;
    ops::Identify::ensure_id(&mut template);
    let template = state
        .apply_optional::<EventTemplate, _, _>(template.id, |previous| {
            template.preserve_missing_availability(previous.as_ref());
            Ok(ops::put(template))
        })
        .await?;
    Ok((StatusCode::CREATED, Json(template)))
}

async fn one_template(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<Json<EventTemplate>> {
    Ok(Json(
        state.require::<EventTemplate>("event template", id).await?,
    ))
}

async fn reorder_templates(
    Tenant(state): Tenant,
    Json(ids): Json<Vec<Uuid>>,
) -> Result<StatusCode> {
    let Some(seq) = db::events::reorder_templates(state.scope(), &ids).await? else {
        return Err(crate::error::AppError::bad_request(
            "event template order must contain every active template exactly once".into(),
        ));
    };
    state.announce(seq, ChangeEvent::EventTemplatesChanged);
    Ok(StatusCode::NO_CONTENT)
}

async fn update_template(
    Tenant(state): Tenant,
    Path(id): Path<Uuid>,
    Json(template): Json<EventTemplate>,
) -> Result<Json<EventTemplate>> {
    ops::validate_update("event template", id, &template)?;
    validation::duration(subroutine_core::SchedulePoint::now(), template.duration)
        .map_err(AppError::bad_request)?;
    let Some(db::Sequenced {
        value: template,
        seq,
    }) = db::events::update_template_preserving_order(state.scope(), id, template).await?
    else {
        return Err(crate::error::AppError::not_found(format!(
            "event template {id} not found"
        )));
    };
    state.announce(seq, ChangeEvent::EventTemplatesChanged);
    Ok(Json(template))
}

async fn trash_template(Tenant(state): Tenant, Path(id): Path<Uuid>) -> Result<StatusCode> {
    state.trash(Delete::EventTemplate(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}
