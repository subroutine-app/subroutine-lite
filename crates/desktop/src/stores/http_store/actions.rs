use super::history::{ActionCompletionChange, StoreChange};
use super::transport::Cmd;
use super::{ActionDataChanged, AppDatabaseStore, DataChanged, WorkspacePersistence};
use crate::settings::Settings;
use crate::stores::UndoTransaction;
use chrono::{DateTime, Local, Utc};
use gpui::Context;
use subroutine_core::{
    Action, BatchPlacement, MutationOperation, OptimisticPatch, ResourceKey, ResourceValue,
};
use uuid::Uuid;

impl AppDatabaseStore {
    pub(super) fn delete_action_without_history(&mut self, id: Uuid, cx: &mut Context<Self>) {
        if let Some(result) = self.enqueue_remote_mutation(
            MutationOperation::DeleteAction { action_id: id },
            OptimisticPatch {
                writes: vec![],
                deletes: vec![ResourceKey::Action { id }],
                routine_order: None,
            },
            cx,
        ) {
            if let Err(error) = result {
                tracing::error!(%error, action_id = %id, "could not persist action deletion");
            }
            return;
        }
        if let Some(result) = self.apply_local_patch(
            OptimisticPatch {
                writes: vec![],
                deletes: vec![ResourceKey::Action { id }],
                routine_order: None,
            },
            cx,
        ) {
            if let Err(error) = result {
                tracing::error!(%error, action_id = %id, "could not persist local action deletion");
            }
            return;
        }

        let (tx, rx) = flume::bounded(1);
        self.dispatch(Cmd::DeleteAction(id, tx), rx, cx, move |store, _, cx| {
            store.actions.retain(|action| action.id != id);
            cx.emit(ActionDataChanged);
            cx.emit(DataChanged);
            cx.notify();
        });
    }

    pub(super) fn backlog_action_without_history(&mut self, id: Uuid, cx: &mut Context<Self>) {
        let Some(mut action) = self.actions.iter().find(|action| action.id == id).cloned() else {
            return;
        };
        action.set_queued(false);
        action.set_start(None);
        action.set_pinned(false);
        self.upsert_action(action, cx);
    }

    pub(super) fn queue_action_without_history(&mut self, id: Uuid, cx: &mut Context<Self>) {
        let snapshot = self.operation_snapshot(cx);
        match subroutine_core::ops::actions::queue(&snapshot, id) {
            Ok(outcome) => {
                for action in outcome.value {
                    self.upsert_action(action, cx);
                }
            }
            Err(error) => tracing::warn!(%error, action_id = %id, "cannot restore queued action"),
        }
    }

    pub fn upsert_action(&mut self, action: Action, cx: &mut Context<Self>) {
        if let Some(result) = self.enqueue_remote_mutation(
            MutationOperation::UpsertAction {
                action: Box::new(action.clone()),
            },
            OptimisticPatch {
                writes: vec![ResourceValue::Action(action.clone())],
                deletes: vec![],
                routine_order: None,
            },
            cx,
        ) {
            if let Err(error) = result {
                tracing::error!(%error, action_id = %action.id, "could not persist action intent");
            }
            return;
        }

        if let Some(result) = self.apply_local_patch(
            OptimisticPatch {
                writes: vec![ResourceValue::Action(action.clone())],
                deletes: vec![],
                routine_order: None,
            },
            cx,
        ) {
            if let Err(error) = result {
                tracing::error!(%error, action_id = %action.id, "could not persist local action");
            }
            return;
        }

        let applied = action.clone();
        let (tx, rx) = flume::bounded(1);
        self.dispatch(
            Cmd::UpsertAction(action, tx),
            rx,
            cx,
            move |store, _result, cx| {
                store.upsert_action_local(applied);
                cx.emit(ActionDataChanged);
                cx.emit(DataChanged);
                cx.notify();
            },
        );
    }

    pub fn batch_action(
        &mut self,
        action: Action,
        cursor: Option<DateTime<Utc>>,
        settings: &Settings,
        cx: &mut Context<Self>,
    ) -> DateTime<Utc> {
        let placement = {
            let context = self.pipeline(settings);
            let cursor = cursor.unwrap_or_else(|| context.batch_start());
            context.place_in_batch(cursor, action.clone())
        };

        if self
            .persistence
            .as_ref()
            .is_some_and(WorkspacePersistence::is_remote)
        {
            let moved = placement.moved().cloned().collect::<Vec<_>>();
            if let Some(Err(error)) = self.enqueue_remote_mutation(
                MutationOperation::UpsertActions {
                    actions: moved.clone(),
                },
                OptimisticPatch {
                    writes: moved.into_iter().map(ResourceValue::Action).collect(),
                    deletes: vec![],
                    routine_order: None,
                },
                cx,
            ) {
                tracing::error!(%error, "could not persist batched action intent");
            }
            return placement.cursor;
        }

        if self.persistence.is_some() {
            let moved = placement.moved().cloned().collect::<Vec<_>>();
            if let Some(result) = self.apply_local_patch(
                OptimisticPatch {
                    writes: moved.into_iter().map(ResourceValue::Action).collect(),
                    deletes: vec![],
                    routine_order: None,
                },
                cx,
            ) {
                if let Err(error) = result {
                    tracing::error!(%error, "could not persist local batched actions");
                }
                return placement.cursor;
            }
        }

        let (tx, rx) = flume::bounded(1);
        self.dispatch(
            Cmd::BatchAction(action, cursor, tx),
            rx,
            cx,
            |store, settled: BatchPlacement, cx| {
                for moved in settled.moved() {
                    store.upsert_action_local(moved.clone());
                }
                cx.emit(ActionDataChanged);
                cx.emit(DataChanged);
                cx.notify();
            },
        );

        placement.cursor
    }

    pub fn save_action(&mut self, id: Uuid, cx: &mut Context<Self>) {
        let Some(action) = self.actions.iter().find(|action| action.id == id) else {
            tracing::warn!(%id, "cannot save an action missing from the local store");
            return;
        };
        let template = action.as_template();
        self.persist_resource_upserts(vec![ResourceValue::ActionTemplate(template)], cx);
    }

    pub fn uncomplete_action(&mut self, id: Uuid, cx: &mut Context<Self>) {
        let Some(action) = self.actions.iter().find(|action| action.id == id).cloned() else {
            tracing::warn!(%id, "cannot uncomplete an action missing from the local store");
            return;
        };
        if !action.is_completed() {
            return;
        }

        let reopened = subroutine_core::ops::actions::uncomplete(action).value;
        self.upsert_action(reopened, cx);
    }

    fn complete_action_without_history(
        &mut self,
        id: Uuid,
        cx: &mut Context<Self>,
    ) -> Option<ActionCompletionChange> {
        let Some(previous) = self.actions.iter().find(|action| action.id == id).cloned() else {
            tracing::warn!(%id, "cannot complete an action missing from the local store");
            return None;
        };
        if previous.is_completed() {
            return None;
        }

        let completed_at = Utc::now();
        let next = previous
            .source_provider
            .is_none()
            .then(|| previous.next_occurrence())
            .flatten();
        let mut completed = previous.clone();
        completed.set_completion(Some(completed_at));
        completed.set_queued(false);
        completed.set_pinned(false);
        let change = ActionCompletionChange {
            previous,
            completed: completed.clone(),
            next: next.clone(),
        };

        let mut writes = vec![ResourceValue::Action(completed.clone())];
        writes.extend(next.clone().map(ResourceValue::Action));
        if let Some(result) = self.enqueue_remote_mutation(
            MutationOperation::CompleteAction {
                action_id: id,
                completed_at,
            },
            OptimisticPatch {
                writes,
                deletes: vec![],
                routine_order: None,
            },
            cx,
        ) {
            return match result {
                Ok(()) => Some(change),
                Err(error) => {
                    tracing::error!(%error, action_id = %id, "could not persist completion intent");
                    None
                }
            };
        }
        let mut local_writes = vec![ResourceValue::Action(completed.clone())];
        local_writes.extend(next.clone().map(ResourceValue::Action));
        if let Some(result) = self.apply_local_patch(
            OptimisticPatch {
                writes: local_writes,
                deletes: vec![],
                routine_order: None,
            },
            cx,
        ) {
            return match result {
                Ok(()) => Some(change),
                Err(error) => {
                    tracing::error!(%error, action_id = %id, "could not persist local completion");
                    None
                }
            };
        }

        tracing::error!(
            action_id = %id,
            "cannot complete an action without durable local persistence"
        );
        None
    }

    pub fn complete_action(&mut self, id: Uuid, cx: &mut Context<Self>) -> Option<UndoTransaction> {
        let change = self.complete_action_without_history(id, cx)?;
        Some(self.push_undo_transaction(StoreChange::ActionsCompleted(vec![change])))
    }

    pub fn complete_actions(
        &mut self,
        ids: &[Uuid],
        cx: &mut Context<Self>,
    ) -> Option<(UndoTransaction, usize)> {
        let changes: Vec<_> = ids
            .iter()
            .filter_map(|id| self.complete_action_without_history(*id, cx))
            .collect();
        if changes.is_empty() {
            return None;
        }
        let affected = changes.len();
        let transaction = self.push_undo_transaction(StoreChange::ActionsCompleted(changes));
        Some((transaction, affected))
    }

    pub(crate) fn plan_queue_actions(&self, ids: &[Uuid], cx: &gpui::App) -> Vec<Action> {
        let mut snapshot = self.operation_snapshot(cx);
        let mut changed = std::collections::HashSet::new();
        for id in ids {
            match subroutine_core::ops::actions::queue(&snapshot, *id) {
                Ok(outcome) => {
                    for action in outcome.value {
                        if let Some(current) =
                            snapshot.actions.iter_mut().find(|a| a.id == action.id)
                        {
                            changed.insert(action.id);
                            *current = action;
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, action_id = %id, "cannot queue dropped action")
                }
            }
        }
        snapshot
            .actions
            .into_iter()
            .filter(|action| changed.contains(&action.id))
            .collect()
    }

    pub fn auto_queue_action(&mut self, id: Uuid, cx: &mut Context<Self>) {
        if self.persistence.is_some() {
            let snapshot = self.operation_snapshot(cx);
            match subroutine_core::ops::actions::queue(&snapshot, id) {
                Ok(outcome) => {
                    let changed = outcome.value;
                    let ids = changed.iter().map(|action| action.id).collect::<Vec<_>>();
                    let patch = OptimisticPatch {
                        writes: changed.iter().cloned().map(ResourceValue::Action).collect(),
                        deletes: vec![],
                        routine_order: None,
                    };
                    let result = if self
                        .persistence
                        .as_ref()
                        .is_some_and(WorkspacePersistence::is_remote)
                    {
                        self.enqueue_remote_mutation(
                            MutationOperation::UpsertActions { actions: changed },
                            patch,
                            cx,
                        )
                    } else {
                        self.apply_local_patch(patch, cx)
                    };
                    match result {
                        Some(Ok(())) => self.push_undo(StoreChange::ActionQueued(ids)),
                        Some(Err(error)) => {
                            tracing::error!(%error, action_id = %id, "could not persist queue intent")
                        }
                        None => {}
                    }
                }
                Err(error) => tracing::warn!(%error, action_id = %id, "cannot queue action"),
            }
            return;
        }

        let (tx, rx) = flume::bounded(1);
        self.dispatch(
            Cmd::QueueAction(id, tx),
            rx,
            cx,
            |store, changed: Vec<Action>, cx| {
                let ids: Vec<Uuid> = changed.iter().map(|a| a.id).collect();
                for action in changed {
                    store.upsert_action_local(action);
                }
                store.push_undo(StoreChange::ActionQueued(ids));
                cx.emit(ActionDataChanged);
                cx.emit(DataChanged);
                cx.notify();
            },
        );
    }

    pub fn clear_action_duration(&mut self, id: Uuid, cx: &mut Context<Self>) {
        if let Some(previous) = self.actions.iter().find(|action| action.id == id).cloned() {
            let mut cleared = previous.clone();
            cleared.duration = None;
            if let Some(result) = self.enqueue_remote_mutation(
                MutationOperation::UpsertAction {
                    action: Box::new(cleared.clone()),
                },
                OptimisticPatch {
                    writes: vec![ResourceValue::Action(cleared.clone())],
                    deletes: vec![],
                    routine_order: None,
                },
                cx,
            ) {
                match result {
                    Ok(()) => {
                        self.push_undo(StoreChange::ActionDurationCleared { previous, cleared })
                    }
                    Err(error) => {
                        tracing::error!(%error, action_id = %id, "could not persist duration change")
                    }
                }
                return;
            }
            if let Some(result) = self.apply_local_patch(
                OptimisticPatch {
                    writes: vec![ResourceValue::Action(cleared.clone())],
                    deletes: vec![],
                    routine_order: None,
                },
                cx,
            ) {
                match result {
                    Ok(()) => {
                        self.push_undo(StoreChange::ActionDurationCleared { previous, cleared })
                    }
                    Err(error) => {
                        tracing::error!(%error, action_id = %id, "could not persist local duration change")
                    }
                }
                return;
            }
        }

        let (tx, rx) = flume::bounded(1);
        self.dispatch(
            Cmd::ClearActionDuration(id, tx),
            rx,
            cx,
            |store, action: Action, cx| {
                let previous = store
                    .actions
                    .iter()
                    .find(|a| a.id == action.id)
                    .cloned()
                    .expect("action must exist to clear duration");
                let cleared = action.clone();
                store.upsert_action_local(action);
                store.push_undo(StoreChange::ActionDurationCleared { previous, cleared });
                cx.emit(ActionDataChanged);
                cx.emit(DataChanged);
                cx.notify();
            },
        );
    }

    pub fn backlog_action(&mut self, id: Uuid, cx: &mut Context<Self>) {
        if let Some(mut action) = self.actions.iter().find(|action| action.id == id).cloned() {
            action.set_queued(false);
            action.set_start(None);
            action.set_pinned(false);
            if let Some(result) = self.enqueue_remote_mutation(
                MutationOperation::UpsertAction {
                    action: Box::new(action.clone()),
                },
                OptimisticPatch {
                    writes: vec![ResourceValue::Action(action.clone())],
                    deletes: vec![],
                    routine_order: None,
                },
                cx,
            ) {
                match result {
                    Ok(()) => self.push_undo(StoreChange::ActionBacklogged(id)),
                    Err(error) => {
                        tracing::error!(%error, action_id = %id, "could not persist backlog intent")
                    }
                }
                return;
            }
            if let Some(result) = self.apply_local_patch(
                OptimisticPatch {
                    writes: vec![ResourceValue::Action(action)],
                    deletes: vec![],
                    routine_order: None,
                },
                cx,
            ) {
                match result {
                    Ok(()) => self.push_undo(StoreChange::ActionBacklogged(id)),
                    Err(error) => {
                        tracing::error!(%error, action_id = %id, "could not persist local backlog intent")
                    }
                }
                return;
            }
        }

        let (tx, rx) = flume::bounded(1);
        self.dispatch(
            Cmd::BacklogAction(id, tx),
            rx,
            cx,
            move |store, action: Action, cx| {
                store.upsert_action_local(action);
                store.push_undo(StoreChange::ActionBacklogged(id));
                cx.emit(ActionDataChanged);
                cx.emit(DataChanged);
                cx.notify();
            },
        );
    }

    pub fn refresh_pipeline(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.operation_snapshot(cx);
        let refreshed_states = subroutine_core::ops::pipeline::refresh(&snapshot).value;
        if refreshed_states.is_empty() {
            return;
        }
        let changed_ids: Vec<Uuid> = refreshed_states.iter().map(|action| action.id).collect();
        let previous_states = self
            .actions
            .iter()
            .filter(|action| changed_ids.contains(&action.id))
            .cloned()
            .collect();
        let resources = refreshed_states
            .iter()
            .cloned()
            .map(ResourceValue::Action)
            .collect();
        if self.persist_resource_upserts(resources, cx) {
            self.push_undo(StoreChange::PipelineRefreshed {
                previous_states,
                refreshed_states,
            });
        }
    }

    pub fn instantiate_routine(
        &mut self,
        id: Uuid,
        start_time: Option<DateTime<Utc>>,
        cx: &mut Context<Self>,
    ) {
        self.try_instantiate_routine(id, start_time, cx);
    }

    pub(crate) fn try_instantiate_routine(
        &mut self,
        id: Uuid,
        start_time: Option<DateTime<Utc>>,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.is_ready() {
            return false;
        }
        let Some(routine) = self
            .routines
            .iter()
            .find(|routine| routine.id == id)
            .cloned()
        else {
            tracing::warn!(%id, "cannot instantiate a routine missing from the local store");
            return false;
        };
        if routine.steps.is_empty() {
            return false;
        }
        let snapshot = self.operation_snapshot(cx);
        let created =
            subroutine_core::ops::routines::instantiate(&snapshot, &routine, start_time).value;
        if created.is_empty() {
            return false;
        }
        let resources = created.iter().cloned().map(ResourceValue::Action).collect();
        let accepted = self.persist_resource_upserts(resources, cx);
        if accepted {
            self.push_undo(StoreChange::RoutineInstantiated { created });
        }
        accepted
    }

    fn operation_snapshot(&self, cx: &gpui::App) -> subroutine_core::ops::Snapshot {
        let settings = Settings::global(cx);
        subroutine_core::ops::Snapshot::new(
            Local::now(),
            subroutine_core::ops::Settings {
                schedule: settings.schedule,
                ..subroutine_core::ops::Settings::default()
            },
            self.actions.clone(),
            self.events.clone(),
            self.routines.clone(),
            self.markers.clone(),
            self.signals.clone(),
        )
    }
}
