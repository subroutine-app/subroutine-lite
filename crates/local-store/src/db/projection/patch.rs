use std::collections::HashSet;
use subroutine_core::{
    AllData, Event, EventTemplate, MutationOperation, OptimisticPatch, ResourceKey, ResourceValue,
    Routine,
};
use uuid::Uuid;

use crate::{LocalStoreError, Result};

pub(crate) fn apply_remote_patch(
    data: &mut AllData,
    mut patch: OptimisticPatch,
    operation: &MutationOperation,
) -> Result<()> {
    if let MutationOperation::SetEventBusyOverride {
        event_id,
        busy_override,
    } = operation
    {
        if let Some(event) = data.events.iter_mut().find(|event| event.id == *event_id) {
            event.busy_override = *busy_override;
        }
        return Ok(());
    }
    patch
        .writes
        .retain(|value| operation_projects_write(operation, value.key()));
    patch
        .deletes
        .retain(|key| operation_projects_delete(operation, *key));
    if !matches!(operation, MutationOperation::ReorderRoutines { .. }) {
        patch.routine_order = None;
    }
    if matches!(operation, MutationOperation::UpsertResources { .. }) {
        preserve_missing_availability(&mut patch, &data.events, &data.event_templates);
    }
    apply_patch(data, patch)
}

pub(in crate::db) fn preserve_missing_availability(
    patch: &mut OptimisticPatch,
    events: &[Event],
    templates: &[EventTemplate],
) {
    for value in &mut patch.writes {
        match value {
            ResourceValue::Event(event) => {
                event.preserve_missing_availability(events.iter().find(|old| old.id == event.id))
            }
            ResourceValue::EventTemplate(template) => template
                .preserve_missing_availability(templates.iter().find(|old| old.id == template.id)),
            _ => {}
        }
    }
}

fn operation_projects_write(operation: &MutationOperation, key: ResourceKey) -> bool {
    match operation {
        MutationOperation::UpsertAction { action } => {
            key == (ResourceKey::Action { id: action.id })
        }
        MutationOperation::UpsertActions { actions } => actions
            .iter()
            .any(|action| key == (ResourceKey::Action { id: action.id })),
        MutationOperation::UpsertResources { resources } => {
            resources.iter().any(|value| value.key() == key)
        }
        MutationOperation::SetEventBusyOverride { event_id, .. } => {
            key == (ResourceKey::Event { id: *event_id })
        }

        MutationOperation::CompleteAction { .. } => true,
        MutationOperation::ConvertEventToMarker { marker_id, .. } => {
            key == (ResourceKey::Marker { id: *marker_id })
        }
        MutationOperation::DeleteResources { .. }
        | MutationOperation::ReorderRoutines { .. }
        | MutationOperation::DeleteAction { .. } => false,
    }
}

fn operation_projects_delete(operation: &MutationOperation, key: ResourceKey) -> bool {
    match operation {
        MutationOperation::DeleteResources { resources } => resources.contains(&key),
        MutationOperation::DeleteAction { action_id } => {
            key == (ResourceKey::Action { id: *action_id })
        }
        MutationOperation::ConvertEventToMarker { event_id, .. } => {
            key == (ResourceKey::Event { id: *event_id })
        }
        MutationOperation::UpsertAction { .. }
        | MutationOperation::UpsertActions { .. }
        | MutationOperation::UpsertResources { .. }
        | MutationOperation::SetEventBusyOverride { .. }
        | MutationOperation::ReorderRoutines { .. }
        | MutationOperation::CompleteAction { .. } => false,
    }
}

fn apply_patch(data: &mut AllData, patch: OptimisticPatch) -> Result<()> {
    let OptimisticPatch {
        writes,
        deletes,
        routine_order,
    } = patch;
    for value in writes {
        match value {
            ResourceValue::Action(action) => upsert(&mut data.actions, action, |row| row.id),
            ResourceValue::Event(event) => upsert(&mut data.events, event, |row| row.id),
            ResourceValue::Routine(routine) => upsert(&mut data.routines, routine, |row| row.id),
            ResourceValue::Marker(marker) => upsert(&mut data.markers, marker, |row| row.id),
            ResourceValue::Signal(signal) => upsert(&mut data.signals, signal, |row| row.id),
            ResourceValue::ActionTemplate(template) => {
                upsert(&mut data.action_templates, template, |row| row.id)
            }
            ResourceValue::EventTemplate(template) => {
                upsert(&mut data.event_templates, template, |row| row.id)
            }
            ResourceValue::MarkerTemplate(template) => {
                upsert(&mut data.marker_templates, template, |row| row.id)
            }
            ResourceValue::SignalTemplate(template) => {
                upsert(&mut data.signal_templates, template, |row| row.id)
            }
        }
    }
    for key in deletes {
        match key {
            ResourceKey::Action { id } => data.actions.retain(|action| action.id != id),
            ResourceKey::Event { id } => data.events.retain(|event| event.id != id),
            ResourceKey::Routine { id } => data.routines.retain(|routine| routine.id != id),
            ResourceKey::Marker { id } => data.markers.retain(|marker| marker.id != id),
            ResourceKey::Signal { id } => data.signals.retain(|signal| signal.id != id),
            ResourceKey::ActionTemplate { id } => {
                data.action_templates.retain(|template| template.id != id)
            }
            ResourceKey::EventTemplate { id } => {
                data.event_templates.retain(|template| template.id != id)
            }
            ResourceKey::MarkerTemplate { id } => {
                data.marker_templates.retain(|template| template.id != id)
            }
            ResourceKey::SignalTemplate { id } => {
                data.signal_templates.retain(|template| template.id != id)
            }
        }
    }
    if let Some(routine_order) = routine_order {
        reorder_routines(&mut data.routines, &routine_order)?;
    }
    data.action_templates
        .sort_by_key(|template| template.sort_order);
    data.event_templates
        .sort_by_key(|template| template.sort_order);
    Ok(())
}

fn upsert<T>(rows: &mut Vec<T>, value: T, id: impl Fn(&T) -> Uuid) {
    if let Some(existing) = rows.iter_mut().find(|row| id(row) == id(&value)) {
        *existing = value;
    } else {
        rows.push(value);
    }
}

fn reorder_routines(routines: &mut Vec<Routine>, routine_order: &[Uuid]) -> Result<()> {
    validate_routine_order(routines.iter().map(|routine| routine.id), routine_order)?;
    let mut unordered = std::mem::take(routines);
    let mut reordered = Vec::with_capacity(unordered.len());
    for id in routine_order {
        let position = unordered
            .iter()
            .position(|routine| routine.id == *id)
            .ok_or_else(|| {
                LocalStoreError::InvalidMutation(
                    "routine order references a routine that does not exist".into(),
                )
            })?;
        reordered.push(unordered.remove(position));
    }
    *routines = reordered;
    Ok(())
}

pub(in crate::db) fn validate_routine_order(
    routine_ids: impl IntoIterator<Item = Uuid>,
    routine_order: &[Uuid],
) -> Result<()> {
    let routine_ids = routine_ids.into_iter().collect::<HashSet<_>>();
    let ordered_ids = routine_order.iter().copied().collect::<HashSet<_>>();
    if routine_ids.len() != routine_order.len()
        || ordered_ids.len() != routine_order.len()
        || routine_ids != ordered_ids
    {
        return Err(LocalStoreError::InvalidMutation(
            "routine order must contain every resulting routine exactly once".into(),
        ));
    }
    Ok(())
}
