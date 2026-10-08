use axum::http::StatusCode;
use chrono::NaiveDate;
use subroutine_core::{
    Action, ApiErrorCode, ChangeEvent, Event, EventTemplate, Marker, MutationEffect,
    MutationOperation, MutationResult, ResourceKey, ResourceValue,
};
use uuid::Uuid;

use super::mutation::MutationContext;
use crate::{
    db,
    error::MutationError,
    ops::{Changes, Delete, Outcome},
};

pub(super) struct MutationOutcome {
    pub(super) mutation: db::TenantMutation,
    pub(super) result: MutationResult,
    pub(super) effect: MutationEffect,
    pub(super) commit_seq: i64,
    pub(super) events: Vec<ChangeEvent>,
}

pub(super) async fn apply(
    mut mutation: db::TenantMutation,
    operation: MutationOperation,
    context: &MutationContext,
) -> Result<MutationOutcome, MutationError> {
    let user_id = mutation.user_id();
    let outcome = match operation {
        MutationOperation::UpsertAction { action } => {
            let previous = db::fetch_by_id_in::<Action>(mutation.connection(), user_id, action.id)
                .await
                .map_err(MutationError::transient)?;
            let outcome = crate::ops::actions::update(previous.as_ref(), *action);
            Outcome::new(
                MutationResult::ActionUpserted {
                    action: Box::new(outcome.value),
                },
                outcome.changes,
            )
        }
        MutationOperation::UpsertActions { actions } => {
            let mut changes = Changes::default();
            changes.put_all(actions.clone());
            Outcome::new(MutationResult::ActionsUpserted { actions }, changes)
        }
        MutationOperation::UpsertResources { resources } => {
            upsert_resources(&mut mutation, resources).await?
        }
        MutationOperation::SetEventBusyOverride {
            event_id,
            busy_override,
        } => {
            let event = db::fetch_by_id_in::<Event>(mutation.connection(), user_id, event_id)
                .await
                .map_err(MutationError::transient)?;
            let Some(mut event) = event else {
                mutation
                    .rollback()
                    .await
                    .map_err(MutationError::transient)?;
                return Err(context.error(
                    StatusCode::NOT_FOUND,
                    ApiErrorCode::ResourceNotFound,
                    format!("event {event_id} not found"),
                    Some(ResourceKey::Event { id: event_id }),
                ));
            };
            event.busy_override = busy_override;
            let mut changes = Changes::default();
            changes.put(event.clone());
            Outcome::new(
                MutationResult::ResourcesUpserted {
                    resources: vec![ResourceValue::Event(event)],
                },
                changes,
            )
        }

        MutationOperation::DeleteResources { resources } => {
            let mut changes = Changes::default();
            for resource in resources.iter().copied() {
                changes.delete(delete_for_resource(resource));
            }
            Outcome::new(MutationResult::ResourcesDeleted { resources }, changes)
        }
        MutationOperation::ReorderRoutines { routine_ids } => {
            return reorder_routines(mutation, routine_ids, context).await;
        }
        MutationOperation::CompleteAction {
            action_id,
            completed_at,
        } => {
            let action = db::fetch_by_id_in::<Action>(mutation.connection(), user_id, action_id)
                .await
                .map_err(MutationError::transient)?;
            let Some(action) = action else {
                mutation
                    .rollback()
                    .await
                    .map_err(MutationError::transient)?;
                return Err(context.error(
                    StatusCode::NOT_FOUND,
                    ApiErrorCode::ResourceNotFound,
                    format!("action {action_id} not found"),
                    Some(ResourceKey::Action { id: action_id }),
                ));
            };
            let outcome = crate::ops::actions::complete(action, completed_at);
            Outcome::new(
                MutationResult::ActionCompleted {
                    completed: Box::new(outcome.value.completed),
                    next: outcome.value.next.map(Box::new),
                },
                outcome.changes,
            )
        }
        MutationOperation::DeleteAction { action_id } => {
            let exists = db::fetch_by_id_in::<Action>(mutation.connection(), user_id, action_id)
                .await
                .map_err(MutationError::transient)?
                .is_some();
            if !exists {
                mutation
                    .rollback()
                    .await
                    .map_err(MutationError::transient)?;
                return Err(context.error(
                    StatusCode::NOT_FOUND,
                    ApiErrorCode::ResourceNotFound,
                    format!("action {action_id} not found"),
                    Some(ResourceKey::Action { id: action_id }),
                ));
            }
            let mut changes = Changes::default();
            changes.delete(Delete::Action(action_id));
            Outcome::new(MutationResult::ActionDeleted { action_id }, changes)
        }
        MutationOperation::ConvertEventToMarker {
            event_id,
            marker_id,
            date,
            local_end_date,
        } => {
            return convert_event_to_marker(
                mutation,
                event_id,
                marker_id,
                date,
                local_end_date,
                context,
            )
            .await;
        }
    };
    apply_changes(mutation, outcome, context).await
}

async fn upsert_resources(
    mutation: &mut db::TenantMutation,
    mut resources: Vec<ResourceValue>,
) -> Result<Outcome<MutationResult>, MutationError> {
    let user_id = mutation.user_id();
    let mut changes = Changes::default();
    for resource in &mut resources {
        match resource {
            ResourceValue::Event(event) => {
                let previous =
                    db::fetch_by_id_in::<Event>(mutation.connection(), user_id, event.id)
                        .await
                        .map_err(MutationError::transient)?;
                event.preserve_missing_availability(previous.as_ref());
            }
            ResourceValue::EventTemplate(template) => {
                let previous = db::fetch_by_id_in::<EventTemplate>(
                    mutation.connection(),
                    user_id,
                    template.id,
                )
                .await
                .map_err(MutationError::transient)?;
                template.preserve_missing_availability(previous.as_ref());
            }
            _ => {}
        }
        put_resource(&mut changes, resource.clone());
    }
    Ok(Outcome::new(
        MutationResult::ResourcesUpserted { resources },
        changes,
    ))
}

async fn convert_event_to_marker(
    mut mutation: db::TenantMutation,
    event_id: Uuid,
    marker_id: Uuid,
    date: NaiveDate,
    local_end_date: Option<NaiveDate>,
    context: &MutationContext,
) -> Result<MutationOutcome, MutationError> {
    let user_id = mutation.user_id();
    let event = db::fetch_by_id_in::<Event>(mutation.connection(), user_id, event_id)
        .await
        .map_err(MutationError::transient)?;
    let Some(event) = event else {
        mutation
            .rollback()
            .await
            .map_err(MutationError::transient)?;
        return Err(context.error(
            StatusCode::NOT_FOUND,
            ApiErrorCode::ResourceNotFound,
            format!("event {event_id} not found"),
            Some(ResourceKey::Event { id: event_id }),
        ));
    };
    let marker_exists = db::identity_exists_in::<Marker>(mutation.connection(), user_id, marker_id)
        .await
        .map_err(MutationError::transient)?;
    if marker_exists {
        mutation
            .rollback()
            .await
            .map_err(MutationError::transient)?;
        return Err(context.error(
            StatusCode::CONFLICT,
            ApiErrorCode::DomainConflict,
            format!("marker {marker_id} already exists"),
            Some(ResourceKey::Marker { id: marker_id }),
        ));
    }
    let outcome =
        crate::ops::convert_event_to_marker_with_id(&event, date, local_end_date, marker_id);
    let result = MutationResult::EventConvertedToMarker {
        event_id,
        marker: Box::new(outcome.value),
    };
    apply_changes(mutation, Outcome::new(result, outcome.changes), context).await
}

async fn reorder_routines(
    mut mutation: db::TenantMutation,
    routine_ids: Vec<Uuid>,
    context: &MutationContext,
) -> Result<MutationOutcome, MutationError> {
    let user_id = mutation.user_id();
    let seq = mutation
        .next_change_seq()
        .await
        .map_err(MutationError::transient)?;
    let reordered = db::routines::reorder_in(mutation.connection(), user_id, seq, &routine_ids)
        .await
        .map_err(MutationError::transient)?;
    if !reordered {
        mutation
            .rollback()
            .await
            .map_err(MutationError::transient)?;
        return Err(context.error(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::ValidationFailed,
            "routine order must contain every active routine exactly once",
            None,
        ));
    }
    Ok(MutationOutcome {
        mutation,
        result: MutationResult::RoutinesReordered { routine_ids },
        effect: MutationEffect::Applied,
        commit_seq: seq,
        events: vec![ChangeEvent::RoutinesChanged],
    })
}

async fn apply_changes(
    mut mutation: db::TenantMutation,
    outcome: Outcome<MutationResult>,
    context: &MutationContext,
) -> Result<MutationOutcome, MutationError> {
    let user_id = mutation.user_id();
    let Outcome {
        value: mut result,
        changes,
    } = outcome;
    let events = changes.change_events();
    let (effect, commit_seq) = if changes.is_empty() {
        if let MutationResult::ActionCompleted { next, .. } = &mut result {
            *next = None;
        }
        (MutationEffect::NoOp, context.current_seq)
    } else {
        let seq = mutation
            .next_change_seq()
            .await
            .map_err(MutationError::transient)?;
        match db::apply_changes_in(mutation.connection(), user_id, seq, &changes)
            .await
            .map_err(MutationError::transient)?
        {
            db::ApplyResult::Applied => {}
            db::ApplyResult::Missing(target) => {
                mutation
                    .rollback()
                    .await
                    .map_err(MutationError::transient)?;
                return Err(context.error(
                    StatusCode::NOT_FOUND,
                    ApiErrorCode::ResourceNotFound,
                    format!("{} {} not found", target.label(), target.id()),
                    Some(resource_for_delete(target)),
                ));
            }
        }
        (MutationEffect::Applied, seq)
    };
    Ok(MutationOutcome {
        mutation,
        result,
        effect,
        commit_seq,
        events,
    })
}

fn put_resource(changes: &mut Changes, resource: ResourceValue) {
    match resource {
        ResourceValue::Action(value) => changes.put(value),
        ResourceValue::Event(value) => changes.put(value),
        ResourceValue::Routine(value) => changes.put(value),
        ResourceValue::Marker(value) => changes.put(value),
        ResourceValue::Signal(value) => changes.put(value),
        ResourceValue::ActionTemplate(value) => changes.put(value),
        ResourceValue::EventTemplate(value) => changes.put(value),
        ResourceValue::MarkerTemplate(value) => changes.put(value),
        ResourceValue::SignalTemplate(value) => changes.put(value),
    };
}

fn delete_for_resource(resource: ResourceKey) -> Delete {
    match resource {
        ResourceKey::Action { id } => Delete::Action(id),
        ResourceKey::Event { id } => Delete::Event(id),
        ResourceKey::Routine { id } => Delete::Routine(id),
        ResourceKey::Marker { id } => Delete::Marker(id),
        ResourceKey::Signal { id } => Delete::Signal(id),
        ResourceKey::ActionTemplate { id } => Delete::ActionTemplate(id),
        ResourceKey::EventTemplate { id } => Delete::EventTemplate(id),
        ResourceKey::MarkerTemplate { id } => Delete::MarkerTemplate(id),
        ResourceKey::SignalTemplate { id } => Delete::SignalTemplate(id),
    }
}

fn resource_for_delete(target: Delete) -> ResourceKey {
    match target {
        Delete::Action(id) => ResourceKey::Action { id },
        Delete::Event(id) => ResourceKey::Event { id },
        Delete::Routine(id) => ResourceKey::Routine { id },
        Delete::Marker(id) => ResourceKey::Marker { id },
        Delete::Signal(id) => ResourceKey::Signal { id },
        Delete::ActionTemplate(id) => ResourceKey::ActionTemplate { id },
        Delete::EventTemplate(id) => ResourceKey::EventTemplate { id },
        Delete::MarkerTemplate(id) => ResourceKey::MarkerTemplate { id },
        Delete::SignalTemplate(id) => ResourceKey::SignalTemplate { id },
    }
}
