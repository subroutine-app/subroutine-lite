use std::collections::HashSet;

use subroutine_core::{DataDelta, OptimisticPatch, ResourceKey, ResourceValue};

pub(super) fn first_patch_resource(patch: &OptimisticPatch) -> Option<ResourceKey> {
    patch_resources(patch).next()
}

pub(super) fn patches_overlap(left: &OptimisticPatch, right: &OptimisticPatch) -> bool {
    let left_keys = patch_resources(left).collect::<HashSet<_>>();
    let left_order = left.routine_order.is_some();
    let right_order = right.routine_order.is_some();
    (left_order && right_order)
        || (right_order
            && left_keys
                .iter()
                .any(|key| matches!(key, ResourceKey::Routine { .. })))
        || patch_resources(right).any(|key| {
            left_keys.contains(&key) || (left_order && matches!(key, ResourceKey::Routine { .. }))
        })
}

fn patch_resources(patch: &OptimisticPatch) -> impl Iterator<Item = ResourceKey> + '_ {
    patch
        .writes
        .iter()
        .map(ResourceValue::key)
        .chain(patch.deletes.iter().copied())
        .chain(
            patch
                .routine_order
                .iter()
                .flatten()
                .copied()
                .map(|id| ResourceKey::Routine { id }),
        )
}

pub(crate) fn first_patch_delta_conflict(
    patch: &OptimisticPatch,
    delta: &DataDelta,
) -> Option<ResourceKey> {
    routine_order_delta_conflict(patch, delta).or_else(|| {
        patch
            .writes
            .iter()
            .map(ResourceValue::key)
            .chain(patch.deletes.iter().copied())
            .find(|key| delta_touches(delta, *key))
    })
}

fn routine_order_delta_conflict(patch: &OptimisticPatch, delta: &DataDelta) -> Option<ResourceKey> {
    let patch_order = patch.routine_order.as_ref()?;
    if delta.routines.is_empty()
        && delta.tombstones.routines.is_empty()
        && delta.routine_order.is_empty()
    {
        return None;
    }

    delta
        .routines
        .first()
        .map(|routine| routine.id)
        .or_else(|| delta.tombstones.routines.first().copied())
        .or_else(|| delta.routine_order.first().copied())
        .or_else(|| patch_order.first().copied())
        .map(|id| ResourceKey::Routine { id })
}

pub(crate) fn delta_touches(delta: &DataDelta, key: ResourceKey) -> bool {
    match key {
        ResourceKey::Action { id } => {
            delta.actions.iter().any(|action| action.id == id)
                || delta.tombstones.actions.contains(&id)
        }
        ResourceKey::Event { id } => {
            delta.events.iter().any(|event| event.id == id) || delta.tombstones.events.contains(&id)
        }
        ResourceKey::Routine { id } => {
            delta.routines.iter().any(|routine| routine.id == id)
                || delta.tombstones.routines.contains(&id)
        }
        ResourceKey::Marker { id } => {
            delta.markers.iter().any(|marker| marker.id == id)
                || delta.tombstones.markers.contains(&id)
        }
        ResourceKey::Signal { id } => {
            delta.signals.iter().any(|signal| signal.id == id)
                || delta.tombstones.signals.contains(&id)
        }
        ResourceKey::ActionTemplate { id } => {
            delta
                .action_templates
                .iter()
                .any(|template| template.id == id)
                || delta.tombstones.action_templates.contains(&id)
        }
        ResourceKey::EventTemplate { id } => {
            delta
                .event_templates
                .iter()
                .any(|template| template.id == id)
                || delta.tombstones.event_templates.contains(&id)
        }
        ResourceKey::MarkerTemplate { id } => {
            delta
                .marker_templates
                .iter()
                .any(|template| template.id == id)
                || delta.tombstones.marker_templates.contains(&id)
        }
        ResourceKey::SignalTemplate { id } => {
            delta
                .signal_templates
                .iter()
                .any(|template| template.id == id)
                || delta.tombstones.signal_templates.contains(&id)
        }
    }
}
