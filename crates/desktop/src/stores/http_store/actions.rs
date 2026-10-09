use super::history::{ActionCompletionChange, StoreChange};
use super::{AppDatabaseStore, SaveResult, WorkspacePersistence};
use crate::settings::Settings;
use crate::stores::UndoTransaction;
use chrono::{DateTime, Local, Utc};
use gpui::Context;
use subroutine_core::{Action, AnyItem, MutationOperation, OptimisticPatch, ResourceValue};
use uuid::Uuid;

impl AppDatabaseStore {
    pub fn upsert_action(&mut self, action: Action, cx: &mut Context<Self>) -> SaveResult {
        self.persist_mutation(
            MutationOperation::UpsertAction {
                action: Box::new(action.clone()),
            },
            OptimisticPatch {
                writes: vec![ResourceValue::Action(action)],
                ..OptimisticPatch::default()
            },
            cx,
        )
    }

    pub fn batch_action(
        &mut self,
        action: Action,
        cursor: Option<DateTime<Utc>>,
        settings: &Settings,
        cx: &mut Context<Self>,
    ) -> SaveResult<DateTime<Utc>> {
        let placement = (|| {
            let context = self.pipeline(settings);
            let cursor = match cursor {
                Some(cursor) => cursor,
                None => context.batch_start()?,
            };
            context.place_in_batch(cursor, action)
        })()
        .map_err(|error| self.save_error(error, cx))?;
        let moved = placement.moved().cloned().collect::<Vec<_>>();
        self.persist_mutation(
            MutationOperation::UpsertActions {
                actions: moved.clone(),
            },
            OptimisticPatch {
                writes: moved.into_iter().map(ResourceValue::Action).collect(),
                ..OptimisticPatch::default()
            },
            cx,
        )?;
        Ok(placement.cursor)
    }

    pub fn save_action(&mut self, id: Uuid, cx: &mut Context<Self>) -> SaveResult {
        let Some(action) = self.actions.iter().find(|action| action.id == id) else {
            return Err(self.save_error("This action is no longer available.", cx));
        };
        let template = action.as_template();
        self.persist_resource_upserts(vec![ResourceValue::ActionTemplate(template)], cx)
    }

    pub fn uncomplete_action(&mut self, id: Uuid, cx: &mut Context<Self>) -> SaveResult {
        let Some(action) = self.actions.iter().find(|action| action.id == id).cloned() else {
            return Err(self.save_error("This action is no longer available.", cx));
        };
        if !action.is_completed() {
            return Ok(());
        }
        let reopened = subroutine_core::ops::actions::uncomplete(action).value;
        self.upsert_action(reopened, cx)
    }

    pub fn complete_action(
        &mut self,
        id: Uuid,
        cx: &mut Context<Self>,
    ) -> SaveResult<Option<UndoTransaction>> {
        self.complete_actions(&[id], cx)
            .map(|outcome| outcome.map(|(transaction, _)| transaction))
    }

    pub fn complete_actions(
        &mut self,
        ids: &[Uuid],
        cx: &mut Context<Self>,
    ) -> SaveResult<Option<(UndoTransaction, usize)>> {
        let completed_at = Utc::now();
        let changes: Vec<_> = self
            .actions
            .iter()
            .filter(|action| ids.contains(&action.id) && !action.is_completed())
            .map(|previous| {
                let next = previous
                    .source_provider
                    .is_none()
                    .then(|| previous.next_occurrence())
                    .flatten();
                let mut completed = previous.clone();
                completed.set_completion(Some(completed_at));
                completed.set_queued(false);
                completed.set_pinned(false);
                ActionCompletionChange {
                    previous: previous.clone(),
                    completed,
                    next,
                }
            })
            .collect();
        if changes.is_empty() {
            return Ok(None);
        }
        let affected = changes.len();
        if self
            .persistence
            .as_ref()
            .is_some_and(WorkspacePersistence::is_remote)
        {
            let mut accepted = Vec::new();
            for change in changes {
                let mut writes = vec![ResourceValue::Action(change.completed.clone())];
                writes.extend(change.next.clone().map(ResourceValue::Action));
                if let Err(error) = self.persist_mutation(
                    MutationOperation::CompleteAction {
                        action_id: change.previous.id,
                        completed_at,
                    },
                    OptimisticPatch {
                        writes,
                        ..OptimisticPatch::default()
                    },
                    cx,
                ) {
                    let saved = accepted.len();
                    if saved > 0 {
                        self.push_undo(StoreChange::ActionsCompleted(accepted));
                        return Err(self.save_error(format!("Completed {saved} of {affected} actions. Retry to complete the remaining actions. {error}"), cx));
                    }
                    return Err(error);
                }
                accepted.push(change);
            }
            let transaction = self.push_undo_transaction(StoreChange::ActionsCompleted(accepted));
            Ok(Some((transaction, affected)))
        } else {
            let writes = changes
                .iter()
                .flat_map(|change| {
                    std::iter::once(change.completed.clone()).chain(change.next.clone())
                })
                .map(ResourceValue::Action)
                .collect();
            self.persist_resource_upserts(writes, cx)?;
            let transaction = self.push_undo_transaction(StoreChange::ActionsCompleted(changes));
            Ok(Some((transaction, affected)))
        }
    }

    pub(crate) fn plan_queue_actions(
        &self,
        ids: &[Uuid],
        cx: &gpui::App,
    ) -> SaveResult<Vec<Action>> {
        let mut snapshot = self.operation_snapshot(cx);
        let mut changed = std::collections::HashSet::new();
        for id in ids {
            let outcome = subroutine_core::ops::actions::queue(&snapshot, *id)
                .map_err(|error| error.to_string())?;
            for action in outcome.value {
                if let Some(current) = snapshot.actions.iter_mut().find(|a| a.id == action.id) {
                    changed.insert(action.id);
                    *current = action;
                }
            }
        }
        Ok(snapshot
            .actions
            .into_iter()
            .filter(|action| changed.contains(&action.id))
            .collect())
    }

    pub fn auto_queue_action(&mut self, id: Uuid, cx: &mut Context<Self>) -> SaveResult {
        self.queue_actions(&[id], cx)
    }

    pub fn queue_actions(&mut self, ids: &[Uuid], cx: &mut Context<Self>) -> SaveResult {
        let changed = self
            .plan_queue_actions(ids, cx)
            .map_err(|error| self.save_error(error, cx))?;
        self.update_items(changed.into_iter().map(AnyItem::Action).collect(), cx)
            .map(|_| ())
    }

    pub fn clear_action_duration(&mut self, id: Uuid, cx: &mut Context<Self>) -> SaveResult {
        let Some(mut action) = self.actions.iter().find(|action| action.id == id).cloned() else {
            return Err(self.save_error("This action is no longer available.", cx));
        };
        action.duration = None;
        self.update_items(vec![AnyItem::Action(action)], cx)
            .map(|_| ())
    }

    pub fn backlog_action(&mut self, id: Uuid, cx: &mut Context<Self>) -> SaveResult {
        self.backlog_actions(&[id], cx)
    }

    pub fn backlog_actions(&mut self, ids: &[Uuid], cx: &mut Context<Self>) -> SaveResult {
        let changed = ids
            .iter()
            .map(|id| {
                let mut action = self
                    .actions
                    .iter()
                    .find(|action| action.id == *id)
                    .cloned()
                    .ok_or_else(|| self.save_error("This action is no longer available.", cx))?;
                action.set_queued(false);
                action.set_start(None);
                action.set_pinned(false);
                Ok(AnyItem::Action(action))
            })
            .collect::<SaveResult<Vec<_>>>()?;
        self.update_items(changed, cx).map(|_| ())
    }

    pub fn refresh_pipeline(&mut self, cx: &mut Context<Self>) -> SaveResult {
        let snapshot = self.operation_snapshot(cx);
        let refreshed_states = subroutine_core::ops::pipeline::refresh(&snapshot)
            .map_err(|error| self.save_error(error.to_string(), cx))?
            .value;
        self.update_items(
            refreshed_states.into_iter().map(AnyItem::Action).collect(),
            cx,
        )
        .map(|_| ())
    }

    pub fn instantiate_routine(
        &mut self,
        id: Uuid,
        start_time: Option<DateTime<Utc>>,
        cx: &mut Context<Self>,
    ) -> SaveResult {
        let created = self
            .plan_routine(id, start_time, cx)
            .map_err(|error| self.save_error(error, cx))?;
        self.update_items(created.into_iter().map(AnyItem::Action).collect(), cx)
            .map(|_| ())
    }

    pub(crate) fn update_items_with_routines(
        &mut self,
        mut items: Vec<AnyItem>,
        routines: Vec<(Uuid, Option<DateTime<Utc>>)>,
        cx: &mut Context<Self>,
    ) -> SaveResult {
        let mut snapshot = self.operation_snapshot(cx);
        for item in &items {
            match item {
                AnyItem::Action(action) => {
                    snapshot.actions.retain(|current| current.id != action.id);
                    snapshot.actions.push(action.clone());
                }
                AnyItem::Event(event) => {
                    snapshot.events.retain(|current| current.id != event.id);
                    snapshot.events.push(event.clone());
                }
                _ => {}
            }
        }
        for (id, start) in routines {
            let Some(routine) = self.routines.iter().find(|routine| routine.id == id) else {
                return Err(self.save_error("This routine is no longer available.", cx));
            };
            if routine.steps.is_empty() {
                return Err(self.save_error("Add a step to this routine first.", cx));
            }
            let created = subroutine_core::ops::routines::instantiate(&snapshot, routine, start)
                .map_err(|error| self.save_error(error.to_string(), cx))?
                .value;
            snapshot.actions.extend(created.clone());
            items.extend(created.into_iter().map(AnyItem::Action));
        }
        self.update_items(items, cx).map(|_| ())
    }

    pub(crate) fn try_instantiate_routine(
        &mut self,
        id: Uuid,
        start_time: Option<DateTime<Utc>>,
        cx: &mut Context<Self>,
    ) -> bool {
        self.instantiate_routine(id, start_time, cx).is_ok()
    }

    pub(crate) fn plan_routine(
        &self,
        id: Uuid,
        start_time: Option<DateTime<Utc>>,
        cx: &gpui::App,
    ) -> SaveResult<Vec<Action>> {
        let routine = self
            .routines
            .iter()
            .find(|routine| routine.id == id)
            .ok_or_else(|| "This routine is no longer available.".to_owned())?;
        if routine.steps.is_empty() {
            return Err("Add a step to this routine first.".into());
        }
        let snapshot = self.operation_snapshot(cx);
        subroutine_core::ops::routines::instantiate(&snapshot, routine, start_time)
            .map(|outcome| outcome.value)
            .map_err(|error| error.to_string())
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
