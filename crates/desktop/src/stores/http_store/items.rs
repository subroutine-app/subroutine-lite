use super::history::StoreChange;
use super::{AppDatabaseStore, SaveFailed, SaveResult, SyncDirection, WorkspacePersistence};
use crate::stores::UndoTransaction;
use chrono::Local;
use gpui::Context;
use subroutine_core::{
    ActionTemplate, AnyItem, ClientMutation, Event, EventTemplate, MUTATION_PROTOCOL_VERSION,
    Marker, MutationOperation, MutationRequest, OptimisticPatch, ResourceKey, ResourceValue,
    Routine, RoutineStep, Signal,
};
use uuid::Uuid;

pub(super) fn item_resource_value(item: &AnyItem) -> ResourceValue {
    match item {
        AnyItem::Action(value) => ResourceValue::Action(value.clone()),
        AnyItem::Event(value) => ResourceValue::Event(value.clone()),
        AnyItem::Routine(value) => ResourceValue::Routine(value.clone()),
        AnyItem::Marker(value) => ResourceValue::Marker(value.clone()),
        AnyItem::Signal(value) => ResourceValue::Signal(value.clone()),
        AnyItem::ActionTemplate(value) => ResourceValue::ActionTemplate(value.clone()),
        AnyItem::EventTemplate(value) => ResourceValue::EventTemplate(value.clone()),
    }
}

pub(super) fn item_resource_key(item: &AnyItem) -> ResourceKey {
    match item {
        AnyItem::Action(value) => ResourceKey::Action { id: value.id },
        AnyItem::Event(value) => ResourceKey::Event { id: value.id },
        AnyItem::Routine(value) => ResourceKey::Routine { id: value.id },
        AnyItem::Marker(value) => ResourceKey::Marker { id: value.id },
        AnyItem::Signal(value) => ResourceKey::Signal { id: value.id },
        AnyItem::ActionTemplate(value) => ResourceKey::ActionTemplate { id: value.id },
        AnyItem::EventTemplate(value) => ResourceKey::EventTemplate { id: value.id },
    }
}

pub(super) fn resource_upsert_intent(
    resources: Vec<ResourceValue>,
) -> (MutationOperation, OptimisticPatch) {
    (
        MutationOperation::UpsertResources {
            resources: resources.clone(),
        },
        OptimisticPatch {
            writes: resources,
            deletes: vec![],
            routine_order: None,
        },
    )
}

fn event_busy_override_intent(
    mut latest: Event,
    busy_override: Option<bool>,
) -> (MutationOperation, OptimisticPatch) {
    latest.busy_override = busy_override;
    (
        MutationOperation::SetEventBusyOverride {
            event_id: latest.id,
            busy_override,
        },
        OptimisticPatch {
            writes: vec![ResourceValue::Event(latest)],
            ..OptimisticPatch::default()
        },
    )
}

pub(super) fn resource_delete_intent(
    resources: Vec<ResourceKey>,
) -> (MutationOperation, OptimisticPatch) {
    (
        MutationOperation::DeleteResources {
            resources: resources.clone(),
        },
        OptimisticPatch {
            writes: vec![],
            deletes: resources,
            routine_order: None,
        },
    )
}

pub(super) fn event_conversion_intent(
    event: &Event,
    date: chrono::NaiveDate,
    local_end_date: chrono::NaiveDate,
    marker_id: Uuid,
) -> SaveResult<(MutationOperation, OptimisticPatch)> {
    let marker = event.to_marker_between_with_id(date, local_end_date, marker_id)?;
    Ok((
        MutationOperation::ConvertEventToMarker {
            event_id: event.id,
            marker_id,
            date,
            local_end_date: Some(local_end_date),
        },
        OptimisticPatch {
            writes: vec![ResourceValue::Marker(marker)],
            deletes: vec![ResourceKey::Event { id: event.id }],
            routine_order: None,
        },
    ))
}

impl AppDatabaseStore {
    pub(super) fn enqueue_remote_mutation(
        &mut self,
        operation: MutationOperation,
        optimistic_patch: OptimisticPatch,
        cx: &mut Context<Self>,
    ) -> Option<Result<(), String>> {
        let persistence = self.persistence.clone()?;
        if !persistence.is_remote() {
            return None;
        }
        let result = (|| {
            let state = persistence.state()?;
            let dataset_id = state
                .dataset_id
                .ok_or_else(|| "remote workspace has no dataset identity".to_owned())?;
            if state.canonical_seq != self.last_applied_seq || self.dataset_id != Some(dataset_id) {
                return Err(
                    "the local projection advanced while the edit was being committed; retry the edit"
                        .into(),
                );
            }
            let operation_name = operation.name();
            let writes = optimistic_patch.writes.len();
            let deletes = optimistic_patch.deletes.len();
            let mutation = ClientMutation {
                request: MutationRequest {
                    protocol_version: MUTATION_PROTOCOL_VERSION,
                    dataset_id,
                    mutation_id: Uuid::now_v7(),
                    client_id: persistence.client_id(),
                    base_seq: state.canonical_seq,
                    operation,
                },
                optimistic_patch,
            };
            let projection = persistence.enqueue(mutation)?;
            self.log_sync(SyncDirection::Outgoing, format!("Queued {operation_name}: {writes} projected writes, {deletes} deletes (saved locally)"), cx);
            self.replace_projection(projection, cx);
            self.start_pull(cx);
            Ok(())
        })();
        Some(result)
    }

    pub(super) fn apply_local_patch(
        &mut self,
        patch: OptimisticPatch,
        cx: &mut Context<Self>,
    ) -> Option<Result<(), String>> {
        let persistence = self.persistence.clone()?;
        if persistence.is_remote() {
            return None;
        }
        Some(persistence.apply_local_patch(patch).map(|projection| {
            self.replace_projection(projection, cx);
        }))
    }

    pub(super) fn save_error(&self, error: impl Into<String>, cx: &mut Context<Self>) -> String {
        let error = error.into();
        tracing::error!(%error, "could not save local changes");
        cx.emit(SaveFailed {
            message: format!("Couldn’t save this change. {error} Check local storage and permissions, then retry."),
        });
        error
    }

    pub(super) fn persist_mutation(
        &mut self,
        operation: MutationOperation,
        patch: OptimisticPatch,
        cx: &mut Context<Self>,
    ) -> SaveResult {
        if self.history_pending() {
            return Err(self.save_error(
                "Retry the unfinished Undo or Redo before saving another change.",
                cx,
            ));
        }
        if !self.is_ready() {
            return Err(self.save_error(
                "The local workspace is not ready. Retry opening it in Settings.",
                cx,
            ));
        }
        patch
            .writes
            .iter()
            .try_for_each(crate::item_subject::validate_resource_timing)
            .map_err(|error| self.save_error(error, cx))?;
        let result = if self
            .persistence
            .as_ref()
            .is_some_and(WorkspacePersistence::is_remote)
        {
            self.enqueue_remote_mutation(operation, patch, cx)
        } else {
            self.apply_local_patch(patch, cx)
        };
        result
            .unwrap_or_else(|| Err("No local workspace is available.".into()))
            .map_err(|error| self.save_error(error, cx))
    }

    pub(super) fn persist_resource_upserts(
        &mut self,
        resources: Vec<ResourceValue>,
        cx: &mut Context<Self>,
    ) -> SaveResult {
        if resources.is_empty() {
            return Ok(());
        }
        let (operation, patch) = resource_upsert_intent(resources);
        self.persist_mutation(operation, patch, cx)
    }

    pub(super) fn persist_resource_deletes(
        &mut self,
        resources: Vec<ResourceKey>,
        cx: &mut Context<Self>,
    ) -> SaveResult {
        if resources.is_empty() {
            return Ok(());
        }
        let (operation, patch) = resource_delete_intent(resources);
        self.persist_mutation(operation, patch, cx)
    }

    pub fn delete_items(
        &mut self,
        items: Vec<AnyItem>,
        cx: &mut Context<Self>,
    ) -> SaveResult<Option<(UndoTransaction, usize)>> {
        if items.is_empty() {
            return Ok(None);
        }
        self.delete_items_without_history(&items, cx)?;
        let affected = items.len();
        let transaction = self.push_undo_transaction(StoreChange::ItemsDeleted(items));
        Ok(Some((transaction, affected)))
    }

    pub(super) fn delete_items_without_history(
        &mut self,
        items: &[AnyItem],
        cx: &mut Context<Self>,
    ) -> SaveResult {
        self.persist_resource_deletes(items.iter().map(item_resource_key).collect(), cx)
    }

    pub fn create_items(&mut self, items: Vec<AnyItem>, cx: &mut Context<Self>) -> SaveResult {
        self.upsert_items_without_history(&items, cx)
    }

    pub(crate) fn try_create_items(&mut self, items: Vec<AnyItem>, cx: &mut Context<Self>) -> bool {
        !items.is_empty() && self.create_items(items, cx).is_ok()
    }

    pub(super) fn upsert_items_without_history(
        &mut self,
        items: &[AnyItem],
        cx: &mut Context<Self>,
    ) -> SaveResult {
        self.persist_resource_upserts(items.iter().map(item_resource_value).collect(), cx)
    }

    pub fn update_items(
        &mut self,
        items: Vec<AnyItem>,
        cx: &mut Context<Self>,
    ) -> SaveResult<Option<UndoTransaction>> {
        let mut previous = Vec::with_capacity(items.len());
        let mut updated = Vec::with_capacity(items.len());
        let mut created = Vec::new();
        for item in items {
            if let Some(original) = self.get_item(item.id()) {
                previous.push(original);
                updated.push(item);
            } else {
                created.push(item);
            }
        }
        if updated.is_empty() && created.is_empty() {
            return Ok(None);
        }

        let resources = updated
            .iter()
            .chain(&created)
            .map(item_resource_value)
            .collect();
        self.persist_resource_upserts(resources, cx)?;
        Ok(Some(self.push_undo_transaction(
            StoreChange::ItemsUpdated {
                previous,
                updated,
                created,
            },
        )))
    }

    pub fn reorder_action_templates(
        &mut self,
        ordered_ids: &[Uuid],
        cx: &mut Context<Self>,
    ) -> SaveResult {
        let mut templates = self.action_templates.clone();
        for (sort_order, id) in ordered_ids.iter().enumerate() {
            if let Some(template) = templates.iter_mut().find(|item| item.id == *id) {
                template.sort_order = sort_order as i64;
            }
        }
        templates.sort_by_key(|template| template.sort_order);
        self.persist_resource_upserts(
            templates
                .into_iter()
                .map(ResourceValue::ActionTemplate)
                .collect(),
            cx,
        )
    }

    pub fn upsert_event(&mut self, event: Event, cx: &mut Context<Self>) -> SaveResult {
        self.persist_resource_upserts(vec![ResourceValue::Event(event)], cx)
    }

    pub fn set_event_busy_override(
        &mut self,
        id: Uuid,
        busy_override: Option<bool>,
        cx: &mut Context<Self>,
    ) -> SaveResult {
        let Some(event) = self.events.iter().find(|event| event.id == id) else {
            return Err(self.save_error("This event is no longer available.", cx));
        };
        let before = event.busy_override;
        if before != busy_override {
            self.apply_event_busy_override(id, busy_override, cx)?;
            self.push_undo(StoreChange::EventBusyOverride {
                event_id: id,
                before,
                after: busy_override,
            });
        }
        Ok(())
    }

    pub(super) fn apply_event_busy_override(
        &mut self,
        id: Uuid,
        busy_override: Option<bool>,
        cx: &mut Context<Self>,
    ) -> SaveResult {
        let Some(event) = self.events.iter().find(|event| event.id == id).cloned() else {
            return Err(self.save_error("This event is no longer available.", cx));
        };
        if self.history_pending() {
            return Err(self.save_error(
                "Retry the unfinished Undo or Redo before saving another change.",
                cx,
            ));
        }
        if !self.is_ready() {
            return Err(self.save_error("The local workspace is not ready.", cx));
        }
        if event.busy_override == busy_override {
            return Ok(());
        }
        let Some(persistence) = self.persistence.clone() else {
            return Err(self.save_error("No local workspace is available.", cx));
        };
        let result = if persistence.is_remote() {
            let (operation, patch) = event_busy_override_intent(event, busy_override);
            self.enqueue_remote_mutation(operation, patch, cx)
                .expect("the active workspace is remote")
        } else {
            persistence
                .set_event_busy_override(id, busy_override)
                .map(|projection| {
                    self.replace_projection(projection, cx);
                })
        };
        result.map_err(|error| self.save_error(error, cx))
    }

    pub fn convert_event_to_marker(&mut self, event: Event, cx: &mut Context<Self>) -> SaveResult {
        let event_id = event.id;
        let Some(event) = self
            .events
            .iter()
            .find(|current| current.id == event_id)
            .cloned()
        else {
            return Err(self.save_error("This event is no longer available.", cx));
        };
        let (date, local_end_date) = event
            .marker_date_range_in(&Local)
            .map_err(|error| self.save_error(error, cx))?;
        let marker_id = Uuid::now_v7();
        let (operation, patch) = event_conversion_intent(&event, date, local_end_date, marker_id)
            .map_err(|error| self.save_error(error, cx))?;
        self.persist_mutation(operation, patch, cx)
    }

    pub fn convert_events_to_markers(
        &mut self,
        events: &[Event],
        cx: &mut Context<Self>,
    ) -> SaveResult {
        let events: Vec<_> = events
            .iter()
            .filter_map(|event| {
                self.events
                    .iter()
                    .find(|current| current.id == event.id)
                    .cloned()
            })
            .collect();
        let intents = events
            .iter()
            .map(|event| {
                let (date, end) = event.marker_date_range_in(&Local)?;
                event_conversion_intent(event, date, end, Uuid::now_v7())
            })
            .collect::<SaveResult<Vec<_>>>()
            .map_err(|error| self.save_error(error, cx))?;
        if self
            .persistence
            .as_ref()
            .is_some_and(WorkspacePersistence::is_remote)
        {
            for (saved, (operation, patch)) in intents.into_iter().enumerate() {
                if let Err(error) = self.persist_mutation(operation, patch, cx) {
                    return Err(self.save_error(format!("Converted {saved} of {} events. Retry to convert the remaining events. {error}", events.len()), cx));
                }
            }
            Ok(())
        } else {
            if intents.is_empty() {
                return Ok(());
            }
            if !self.is_ready() || self.history_pending() {
                return Err(self.save_error(
                    "The local workspace is not ready or has unfinished Undo or Redo changes.",
                    cx,
                ));
            }
            let mut patch = OptimisticPatch::default();
            for (_, conversion) in intents {
                patch.writes.extend(conversion.writes);
                patch.deletes.extend(conversion.deletes);
            }
            self.apply_local_patch(patch, cx)
                .unwrap_or_else(|| Err("No local workspace is available.".into()))
                .map_err(|error| self.save_error(error, cx))
        }
    }

    pub fn update_action_template(
        &mut self,
        template: ActionTemplate,
        cx: &mut Context<Self>,
    ) -> SaveResult {
        if !self
            .action_templates
            .iter()
            .any(|current| current.id == template.id)
        {
            return Err(self.save_error("This saved action is no longer available.", cx));
        }
        self.persist_resource_upserts(vec![ResourceValue::ActionTemplate(template)], cx)
    }

    pub fn update_event_template(
        &mut self,
        template: EventTemplate,
        cx: &mut Context<Self>,
    ) -> SaveResult {
        if !self
            .event_templates
            .iter()
            .any(|current| current.id == template.id)
        {
            return Err(self.save_error("This saved event is no longer available.", cx));
        }
        self.persist_resource_upserts(vec![ResourceValue::EventTemplate(template)], cx)
    }

    pub fn reorder_event_templates(
        &mut self,
        ordered_ids: &[Uuid],
        cx: &mut Context<Self>,
    ) -> SaveResult {
        let mut templates = self.event_templates.clone();
        for (sort_order, id) in ordered_ids.iter().enumerate() {
            if let Some(template) = templates.iter_mut().find(|item| item.id == *id) {
                template.sort_order = sort_order as i64;
            }
        }
        templates.sort_by_key(|template| template.sort_order);
        self.persist_resource_upserts(
            templates
                .into_iter()
                .map(ResourceValue::EventTemplate)
                .collect(),
            cx,
        )
    }

    pub fn upsert_marker(&mut self, marker: Marker, cx: &mut Context<Self>) -> SaveResult {
        self.persist_resource_upserts(vec![ResourceValue::Marker(marker)], cx)
    }

    pub fn upsert_signal(&mut self, signal: Signal, cx: &mut Context<Self>) -> SaveResult {
        self.persist_resource_upserts(vec![ResourceValue::Signal(signal)], cx)
    }

    pub(super) fn delete_saved_items_without_history(
        &mut self,
        action_ids: &[Uuid],
        event_ids: &[Uuid],
        cx: &mut Context<Self>,
    ) -> SaveResult {
        let resources = action_ids
            .iter()
            .map(|id| ResourceKey::ActionTemplate { id: *id })
            .chain(
                event_ids
                    .iter()
                    .map(|id| ResourceKey::EventTemplate { id: *id }),
            )
            .collect();
        self.persist_resource_deletes(resources, cx)
    }

    pub fn delete_saved_items(
        &mut self,
        ids: &[Uuid],
        cx: &mut Context<Self>,
    ) -> SaveResult<Option<(UndoTransaction, usize)>> {
        let actions: Vec<_> = self
            .action_templates
            .iter()
            .filter(|item| ids.contains(&item.id))
            .cloned()
            .collect();
        let events: Vec<_> = self
            .event_templates
            .iter()
            .filter(|item| ids.contains(&item.id))
            .cloned()
            .collect();
        let affected = actions.len() + events.len();
        if affected == 0 {
            return Ok(None);
        }

        let action_ids: Vec<_> = actions.iter().map(|item| item.id).collect();
        let event_ids: Vec<_> = events.iter().map(|item| item.id).collect();
        self.delete_saved_items_without_history(&action_ids, &event_ids, cx)?;
        let transaction =
            self.push_undo_transaction(StoreChange::SavedItemsDeleted { actions, events });
        Ok(Some((transaction, affected)))
    }

    pub fn upsert_routine(&mut self, routine: Routine, cx: &mut Context<Self>) -> SaveResult {
        self.persist_resource_upserts(vec![ResourceValue::Routine(routine)], cx)
    }

    pub fn replace_routine_steps(
        &mut self,
        id: Uuid,
        steps: Vec<RoutineStep>,
        cx: &mut Context<Self>,
    ) -> SaveResult {
        let Some(mut routine) = self
            .routines
            .iter()
            .find(|routine| routine.id == id)
            .cloned()
        else {
            return Err(self.save_error("This routine is no longer available.", cx));
        };
        routine.steps = steps;
        self.persist_resource_upserts(vec![ResourceValue::Routine(routine)], cx)
    }

    pub fn reorder_routines(&mut self, ids: Vec<Uuid>, cx: &mut Context<Self>) -> SaveResult {
        let current: std::collections::HashSet<_> =
            self.routines.iter().map(|routine| routine.id).collect();
        let requested: std::collections::HashSet<_> = ids.iter().copied().collect();
        if ids.len() != self.routines.len() || requested.len() != ids.len() || requested != current
        {
            return Err(self.save_error("The routine list changed. Retry the reorder.", cx));
        }

        let patch = OptimisticPatch {
            writes: vec![],
            deletes: vec![],
            routine_order: Some(ids.clone()),
        };
        self.persist_mutation(
            MutationOperation::ReorderRoutines { routine_ids: ids },
            patch,
            cx,
        )
    }
}
