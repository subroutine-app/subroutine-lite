use super::items::{item_resource_key, item_resource_value};
use super::{AppDatabaseStore, SaveResult, WorkspacePersistence};
use crate::stores::UndoTransaction;
use gpui::Context;
use subroutine_core::{
    Action, ActionTemplate, AnyItem, EventTemplate, OptimisticPatch, ResourceKey, ResourceValue,
};
use uuid::Uuid;

#[derive(Clone)]
pub(super) struct ActionCompletionChange {
    pub(super) previous: Action,
    pub(super) completed: Action,
    pub(super) next: Option<Action>,
}

#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
pub(super) enum StoreChange {
    EventBusyOverride {
        event_id: Uuid,
        before: Option<bool>,
        after: Option<bool>,
    },
    ActionsCompleted(Vec<ActionCompletionChange>),
    ItemsDeleted(Vec<AnyItem>),
    ItemsUpdated {
        previous: Vec<AnyItem>,
        updated: Vec<AnyItem>,
        created: Vec<AnyItem>,
    },
    SavedItemsDeleted {
        actions: Vec<ActionTemplate>,
        events: Vec<EventTemplate>,
    },
    Pending {
        original: Box<StoreChange>,
        remaining: OptimisticPatch,
    },
}

impl StoreChange {
    fn patch(&self, undo: bool) -> OptimisticPatch {
        let mut patch = OptimisticPatch::default();
        match self {
            Self::EventBusyOverride { .. } => unreachable!(),
            Self::ActionsCompleted(changes) => {
                for change in changes {
                    if undo {
                        patch
                            .writes
                            .push(ResourceValue::Action(change.previous.clone()));
                        patch.deletes.extend(
                            change
                                .next
                                .as_ref()
                                .map(|next| ResourceKey::Action { id: next.id }),
                        );
                    } else {
                        patch
                            .writes
                            .push(ResourceValue::Action(change.completed.clone()));
                        patch
                            .writes
                            .extend(change.next.clone().map(ResourceValue::Action));
                    }
                }
            }
            Self::ItemsDeleted(items) => {
                if undo {
                    patch.writes = items.iter().map(item_resource_value).collect();
                } else {
                    patch.deletes = items.iter().map(item_resource_key).collect();
                }
            }
            Self::ItemsUpdated {
                previous,
                updated,
                created,
            } => {
                if undo {
                    patch.writes = previous.iter().map(item_resource_value).collect();
                    patch.deletes = created.iter().map(item_resource_key).collect();
                } else {
                    patch.writes = updated
                        .iter()
                        .chain(created)
                        .map(item_resource_value)
                        .collect();
                }
            }
            Self::SavedItemsDeleted { actions, events } => {
                if undo {
                    patch.writes = actions
                        .iter()
                        .cloned()
                        .map(ResourceValue::ActionTemplate)
                        .chain(events.iter().cloned().map(ResourceValue::EventTemplate))
                        .collect();
                } else {
                    patch.deletes = actions
                        .iter()
                        .map(|item| ResourceKey::ActionTemplate { id: item.id })
                        .chain(
                            events
                                .iter()
                                .map(|item| ResourceKey::EventTemplate { id: item.id }),
                        )
                        .collect();
                }
            }
            Self::Pending { remaining, .. } => return remaining.clone(),
        }
        patch
    }
}

impl AppDatabaseStore {
    pub(super) fn push_undo(&mut self, change: StoreChange) {
        self.history.record(change);
    }

    pub(super) fn push_undo_transaction(&mut self, change: StoreChange) -> UndoTransaction {
        self.history.record(change)
    }

    pub(super) fn history_pending(&self) -> bool {
        matches!(self.history.next_undo(), Some(StoreChange::Pending { .. }))
            || matches!(self.history.next_redo(), Some(StoreChange::Pending { .. }))
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
            && !matches!(self.history.next_redo(), Some(StoreChange::Pending { .. }))
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
            && !matches!(self.history.next_undo(), Some(StoreChange::Pending { .. }))
    }

    pub fn undo_transaction(
        &mut self,
        transaction: UndoTransaction,
        cx: &mut Context<Self>,
    ) -> SaveResult<bool> {
        if !self.history.next_undo_is(transaction) {
            return Ok(false);
        }
        self.undo(cx)
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) -> SaveResult<bool> {
        if !self.can_undo() {
            return Ok(false);
        }
        let Some(mut entry) = self.history.pop_undo() else {
            return Ok(false);
        };
        let result = self.apply_history(&mut entry.value, true, cx);
        if result.is_ok() {
            self.history.push_redo(entry);
        } else {
            self.history.push_undo(entry);
        }
        cx.notify();
        result.map(|_| true)
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) -> SaveResult<bool> {
        if !self.can_redo() {
            return Ok(false);
        }
        let Some(mut entry) = self.history.pop_redo() else {
            return Ok(false);
        };
        let result = self.apply_history(&mut entry.value, false, cx);
        if result.is_ok() {
            self.history.push_undo(entry);
        } else {
            self.history.push_redo(entry);
        }
        cx.notify();
        result.map(|_| true)
    }

    fn apply_history(
        &mut self,
        change: &mut StoreChange,
        undo: bool,
        cx: &mut Context<Self>,
    ) -> SaveResult {
        if let StoreChange::EventBusyOverride {
            event_id,
            before,
            after,
        } = change
        {
            return self.apply_event_busy_override(
                *event_id,
                if undo { *before } else { *after },
                cx,
            );
        }
        let mut patch = change.patch(undo);
        if self
            .persistence
            .as_ref()
            .is_some_and(WorkspacePersistence::is_remote)
        {
            let partial =
                !patch.writes.is_empty() || matches!(&*change, StoreChange::Pending { .. });
            self.persist_resource_upserts(patch.writes.clone(), cx)?;
            patch.writes.clear();
            if let Err(error) = self.persist_resource_deletes(patch.deletes.clone(), cx) {
                if !partial {
                    return Err(error);
                }
                let original = match &*change {
                    StoreChange::Pending { original, .. } => original.clone(),
                    original => Box::new(original.clone()),
                };
                *change = StoreChange::Pending {
                    original,
                    remaining: patch,
                };
                let command = if undo { "Undo" } else { "Redo" };
                return Err(self.save_error(format!("{command} is unfinished; any accepted changes remain saved. Retry {command} to finish the remaining changes. {error}"), cx));
            }
        } else {
            if !self.is_ready() {
                return Err(self.save_error("The local workspace is not ready.", cx));
            }
            self.apply_local_patch(patch, cx)
                .unwrap_or_else(|| Err("No local workspace is available.".into()))
                .map_err(|error| self.save_error(error, cx))?;
        }
        if let StoreChange::Pending { original, .. } = change {
            *change = *original.clone();
        }
        Ok(())
    }
}
