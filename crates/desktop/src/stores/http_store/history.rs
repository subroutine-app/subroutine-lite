use super::transport::{Cmd, remove_cmd, restore_cmd};
use super::{ActionDataChanged, AppDatabaseStore, DataChanged, EventDataChanged};
use crate::stores::UndoTransaction;
use crate::utils::LogErr;
use gpui::Context;
use subroutine_core::{Action, ActionTemplate, AnyItem, EventTemplate};
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
    ActionQueued(Vec<Uuid>),
    ActionDurationCleared {
        previous: Action,
        cleared: Action,
    },
    ActionBacklogged(Uuid),
    RoutineInstantiated {
        created: Vec<Action>,
    },
    PipelineRefreshed {
        previous_states: Vec<Action>,
        refreshed_states: Vec<Action>,
    },
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
}

impl AppDatabaseStore {
    pub(super) fn push_undo(&mut self, change: StoreChange) {
        self.history.record(change);
    }

    pub(super) fn push_undo_transaction(&mut self, change: StoreChange) -> UndoTransaction {
        self.history.record(change)
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn undo_transaction(
        &mut self,
        transaction: UndoTransaction,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.history.next_undo_is(transaction) {
            return false;
        }
        self.undo(cx);
        true
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self.history.pop_undo() else {
            return;
        };

        if let StoreChange::EventBusyOverride {
            event_id, before, ..
        } = &entry.value
        {
            if self.apply_event_busy_override(*event_id, *before, cx) {
                self.history.push_redo(entry);
            } else {
                self.history.push_undo(entry);
            }
            cx.notify();
            return;
        }
        let change = entry.value.clone();
        self.history.push_redo(entry);
        cx.notify();
        if self.persistence.is_some() {
            self.undo_persisted(change, cx);
        } else {
            self.undo_without_persistence(change, cx);
        }
    }

    fn undo_persisted(&mut self, change: StoreChange, cx: &mut Context<Self>) {
        match change {
            StoreChange::EventBusyOverride { .. } => {
                unreachable!("field-only history handled above")
            }
            StoreChange::ActionsCompleted(changes) => {
                for change in changes.into_iter().rev() {
                    self.upsert_action(change.previous, cx);
                    if let Some(next) = change.next {
                        self.delete_action_without_history(next.id, cx);
                    }
                }
            }
            StoreChange::ActionQueued(ids) => {
                for id in ids {
                    self.backlog_action_without_history(id, cx);
                }
            }
            StoreChange::ActionDurationCleared { previous, .. } => {
                self.upsert_action(previous, cx);
            }
            StoreChange::ActionBacklogged(id) => self.queue_action_without_history(id, cx),
            StoreChange::RoutineInstantiated { created } => {
                for action in created {
                    self.delete_action_without_history(action.id, cx);
                }
            }
            StoreChange::PipelineRefreshed {
                previous_states, ..
            } => {
                for action in previous_states {
                    self.upsert_action(action, cx);
                }
                cx.emit(EventDataChanged);
            }
            StoreChange::ItemsDeleted(items) => self.create_items(items, cx),
            StoreChange::ItemsUpdated {
                previous, created, ..
            } => {
                self.upsert_items_without_history(&previous, cx);
                self.delete_items_without_history(&created, cx);
            }
            StoreChange::SavedItemsDeleted { actions, events } => {
                self.restore_saved_items_without_history(actions, events, cx);
            }
        }
    }

    fn undo_without_persistence(&mut self, change: StoreChange, cx: &mut Context<Self>) {
        let cmd_tx = self.cmd_tx.clone();

        match change {
            StoreChange::EventBusyOverride { .. } => {
                unreachable!("field-only history handled above")
            }
            StoreChange::ActionsCompleted(changes) => {
                cx.spawn(async move |this, cx| {
                    for change in changes.into_iter().rev() {
                        let (tx, rx) = flume::bounded(1);
                        let _ = cmd_tx.send(Cmd::UpsertAction(change.previous.clone(), tx));
                        if let Ok(Ok(())) = rx.recv_async().await {
                            this.update(cx, |store, cx| {
                                store.upsert_action_local(change.previous);
                                cx.emit(ActionDataChanged);
                                cx.emit(DataChanged);
                                cx.notify();
                            })
                            .log_err();
                        }

                        if let Some(next_action) = change.next {
                            let (tx, rx) = flume::bounded(1);
                            let _ = cmd_tx.send(Cmd::DeleteAction(next_action.id, tx));
                            if let Ok(Ok(())) = rx.recv_async().await {
                                this.update(cx, |store, cx| {
                                    store.actions.retain(|a| a.id != next_action.id);
                                    cx.emit(ActionDataChanged);
                                    cx.emit(DataChanged);
                                    cx.notify();
                                })
                                .log_err();
                            }
                        }
                    }
                })
                .detach();
            }

            StoreChange::ActionQueued(ids) => {
                cx.spawn(async move |this, cx| {
                    for id in ids {
                        let (tx, rx) = flume::bounded(1);
                        let _ = cmd_tx.send(Cmd::BacklogAction(id, tx));
                        if let Ok(Ok(action)) = rx.recv_async().await {
                            this.update(cx, |store, cx| {
                                store.upsert_action_local(action);
                                cx.emit(ActionDataChanged);
                                cx.emit(DataChanged);
                                cx.notify();
                            })
                            .log_err();
                        }
                    }
                })
                .detach();
            }

            StoreChange::ActionDurationCleared { previous, .. } => {
                cx.spawn(async move |this, cx| {
                    let (tx, rx) = flume::bounded(1);
                    let _ = cmd_tx.send(Cmd::UpsertAction(previous.clone(), tx));
                    if let Ok(Ok(())) = rx.recv_async().await {
                        this.update(cx, |store, cx| {
                            store.upsert_action_local(previous);
                            cx.emit(ActionDataChanged);
                            cx.emit(DataChanged);
                            cx.notify();
                        })
                        .log_err();
                    }
                })
                .detach();
            }

            StoreChange::ActionBacklogged(id) => {
                cx.spawn(async move |this, cx| {
                    let (tx, rx) = flume::bounded(1);
                    let _ = cmd_tx.send(Cmd::QueueAction(id, tx));
                    if let Ok(Ok(changed)) = rx.recv_async().await {
                        this.update(cx, |store, cx| {
                            for action in changed {
                                store.upsert_action_local(action);
                            }
                            cx.emit(ActionDataChanged);
                            cx.emit(DataChanged);
                            cx.notify();
                        })
                        .log_err();
                    }
                })
                .detach();
            }

            StoreChange::RoutineInstantiated { created } => {
                cx.spawn(async move |this, cx| {
                    for action in created {
                        let (tx, rx) = flume::bounded(1);
                        let _ = cmd_tx.send(Cmd::DeleteAction(action.id, tx));
                        if let Ok(Ok(())) = rx.recv_async().await {
                            this.update(cx, |store, cx| {
                                store.actions.retain(|a| a.id != action.id);
                                cx.emit(ActionDataChanged);
                                cx.emit(DataChanged);
                                cx.notify();
                            })
                            .log_err();
                        }
                    }
                })
                .detach();
            }

            StoreChange::PipelineRefreshed {
                previous_states, ..
            } => {
                cx.spawn(async move |this, cx| {
                    for previous in previous_states {
                        let (tx, rx) = flume::bounded(1);
                        let _ = cmd_tx.send(Cmd::UpsertAction(previous.clone(), tx));
                        if let Ok(Ok(())) = rx.recv_async().await {
                            this.update(cx, |store, cx| {
                                store.upsert_action_local(previous);
                                cx.emit(ActionDataChanged);
                                cx.emit(DataChanged);
                                cx.notify();
                            })
                            .log_err();
                        }
                    }
                    this.update(cx, |_, cx| {
                        cx.emit(EventDataChanged);
                    })
                    .log_err();
                })
                .detach();
            }

            StoreChange::ItemsDeleted(items) => {
                cx.spawn(async move |this, cx| {
                    for item in items {
                        let (tx, rx) = flume::bounded(1);
                        let _ = cmd_tx.send(restore_cmd(item.clone(), tx));
                        if let Ok(Ok(())) = rx.recv_async().await {
                            this.update(cx, |store, cx| {
                                store.upsert_item_local(item);
                                Self::emit_all_changed(cx);
                            })
                            .log_err();
                        }
                    }
                })
                .detach();
            }
            StoreChange::ItemsUpdated {
                previous, created, ..
            } => {
                for item in previous {
                    self.upsert_item(item, cx);
                }
                for item in created {
                    self.delete_item_without_history(&item, cx);
                }
                Self::emit_all_changed(cx);
            }
            StoreChange::SavedItemsDeleted { actions, events } => {
                self.restore_saved_items_without_history(actions, events, cx);
            }
        }
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self.history.pop_redo() else {
            return;
        };

        if let StoreChange::EventBusyOverride {
            event_id, after, ..
        } = &entry.value
        {
            if self.apply_event_busy_override(*event_id, *after, cx) {
                self.history.push_undo(entry);
            } else {
                self.history.push_redo(entry);
            }
            cx.notify();
            return;
        }
        let change = entry.value.clone();
        self.history.push_undo(entry);
        cx.notify();
        if self.persistence.is_some() {
            self.redo_persisted(change, cx);
        } else {
            self.redo_without_persistence(change, cx);
        }
    }

    fn redo_persisted(&mut self, change: StoreChange, cx: &mut Context<Self>) {
        match change {
            StoreChange::EventBusyOverride { .. } => {
                unreachable!("field-only history handled above")
            }
            StoreChange::ActionsCompleted(changes) => {
                for change in changes {
                    self.upsert_action(change.completed, cx);
                    if let Some(next) = change.next {
                        self.upsert_action(next, cx);
                    }
                }
            }
            StoreChange::ActionQueued(ids) => {
                for id in ids {
                    self.queue_action_without_history(id, cx);
                }
            }
            StoreChange::ActionDurationCleared { cleared, .. } => {
                self.upsert_action(cleared, cx);
            }
            StoreChange::ActionBacklogged(id) => self.backlog_action_without_history(id, cx),
            StoreChange::RoutineInstantiated { created } => {
                for action in created {
                    self.upsert_action(action, cx);
                }
            }
            StoreChange::PipelineRefreshed {
                refreshed_states, ..
            } => {
                for action in refreshed_states {
                    self.upsert_action(action, cx);
                }
                cx.emit(EventDataChanged);
            }
            StoreChange::ItemsDeleted(items) => {
                self.delete_items_without_history(&items, cx);
            }
            StoreChange::ItemsUpdated {
                updated, created, ..
            } => {
                let items = updated.into_iter().chain(created).collect::<Vec<_>>();
                self.upsert_items_without_history(&items, cx);
            }
            StoreChange::SavedItemsDeleted { actions, events } => {
                let action_ids: Vec<_> = actions.iter().map(|item| item.id).collect();
                let event_ids: Vec<_> = events.iter().map(|item| item.id).collect();
                self.delete_saved_items_without_history(&action_ids, &event_ids, cx);
            }
        }
    }

    fn redo_without_persistence(&mut self, change: StoreChange, cx: &mut Context<Self>) {
        let cmd_tx = self.cmd_tx.clone();

        match change {
            StoreChange::EventBusyOverride { .. } => {
                unreachable!("field-only history handled above")
            }
            StoreChange::ActionsCompleted(changes) => {
                cx.spawn(async move |this, cx| {
                    for change in changes {
                        let (tx, rx) = flume::bounded(1);
                        let _ = cmd_tx.send(Cmd::UpsertAction(change.completed.clone(), tx));
                        if let Ok(Ok(())) = rx.recv_async().await {
                            this.update(cx, |store, cx| {
                                store.upsert_action_local(change.completed);
                                cx.emit(ActionDataChanged);
                                cx.emit(DataChanged);
                                cx.notify();
                            })
                            .log_err();
                        }

                        if let Some(next_action) = change.next {
                            let (tx, rx) = flume::bounded(1);
                            let _ = cmd_tx.send(Cmd::UpsertAction(next_action.clone(), tx));
                            if let Ok(Ok(())) = rx.recv_async().await {
                                this.update(cx, |store, cx| {
                                    store.upsert_action_local(next_action);
                                    cx.emit(ActionDataChanged);
                                    cx.emit(DataChanged);
                                    cx.notify();
                                })
                                .log_err();
                            }
                        }
                    }
                })
                .detach();
            }

            StoreChange::ActionQueued(ids) => {
                cx.spawn(async move |this, cx| {
                    for id in ids {
                        let (tx, rx) = flume::bounded(1);
                        let _ = cmd_tx.send(Cmd::QueueAction(id, tx));
                        if let Ok(Ok(changed)) = rx.recv_async().await {
                            this.update(cx, |store, cx| {
                                for action in changed {
                                    store.upsert_action_local(action);
                                }
                                cx.emit(ActionDataChanged);
                                cx.emit(DataChanged);
                                cx.notify();
                            })
                            .log_err();
                        }
                    }
                })
                .detach();
            }

            StoreChange::ActionDurationCleared { cleared, .. } => {
                cx.spawn(async move |this, cx| {
                    let (tx, rx) = flume::bounded(1);
                    let _ = cmd_tx.send(Cmd::UpsertAction(cleared.clone(), tx));
                    if let Ok(Ok(())) = rx.recv_async().await {
                        this.update(cx, |store, cx| {
                            store.upsert_action_local(cleared);
                            cx.emit(ActionDataChanged);
                            cx.emit(DataChanged);
                            cx.notify();
                        })
                        .log_err();
                    }
                })
                .detach();
            }

            StoreChange::ActionBacklogged(id) => {
                cx.spawn(async move |this, cx| {
                    let (tx, rx) = flume::bounded(1);
                    let _ = cmd_tx.send(Cmd::BacklogAction(id, tx));
                    if let Ok(Ok(action)) = rx.recv_async().await {
                        this.update(cx, |store, cx| {
                            store.upsert_action_local(action);
                            cx.emit(ActionDataChanged);
                            cx.emit(DataChanged);
                            cx.notify();
                        })
                        .log_err();
                    }
                })
                .detach();
            }

            StoreChange::RoutineInstantiated { created } => {
                cx.spawn(async move |this, cx| {
                    for action in created {
                        let (tx, rx) = flume::bounded(1);
                        let _ = cmd_tx.send(Cmd::UpsertAction(action.clone(), tx));
                        if let Ok(Ok(())) = rx.recv_async().await {
                            this.update(cx, |store, cx| {
                                store.upsert_action_local(action);
                                cx.emit(ActionDataChanged);
                                cx.emit(DataChanged);
                                cx.notify();
                            })
                            .log_err();
                        }
                    }
                })
                .detach();
            }

            StoreChange::PipelineRefreshed {
                refreshed_states, ..
            } => {
                cx.spawn(async move |this, cx| {
                    for refreshed in refreshed_states {
                        let (tx, rx) = flume::bounded(1);
                        let _ = cmd_tx.send(Cmd::UpsertAction(refreshed.clone(), tx));
                        if let Ok(Ok(())) = rx.recv_async().await {
                            this.update(cx, |store, cx| {
                                store.upsert_action_local(refreshed);
                                cx.emit(ActionDataChanged);
                                cx.emit(DataChanged);
                                cx.notify();
                            })
                            .log_err();
                        }
                    }
                    this.update(cx, |_, cx| {
                        cx.emit(EventDataChanged);
                    })
                    .log_err();
                })
                .detach();
            }

            StoreChange::ItemsDeleted(items) => {
                cx.spawn(async move |this, cx| {
                    for item in items {
                        let (tx, rx) = flume::bounded(1);
                        let _ = cmd_tx.send(remove_cmd(&item, tx));
                        if let Ok(Ok(())) = rx.recv_async().await {
                            this.update(cx, |store, cx| {
                                store.remove_item_local(&item);
                                Self::emit_all_changed(cx);
                            })
                            .log_err();
                        }
                    }
                })
                .detach();
            }
            StoreChange::ItemsUpdated {
                updated, created, ..
            } => {
                for item in updated.into_iter().chain(created) {
                    self.upsert_item(item, cx);
                }
            }
            StoreChange::SavedItemsDeleted { actions, events } => {
                let action_ids: Vec<_> = actions.iter().map(|item| item.id).collect();
                let event_ids: Vec<_> = events.iter().map(|item| item.id).collect();
                self.delete_saved_items_without_history(&action_ids, &event_ids, cx);
            }
        }
    }
}
