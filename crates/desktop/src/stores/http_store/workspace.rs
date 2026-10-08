use super::sync::SyncRequestLatch;
use super::{AppDatabaseStore, DatabaseError, StoreStatus, SyncStatus, WorkspacePersistence};
use crate::auth::{AuthSession, RequestScope};
use crate::notifications;
use crate::stores::local_store::workspace_root_from_env;
use gpui::{AppContext, Context};
use local_store::Projection;
use std::path::PathBuf;

pub(super) fn load_workspace_projection(
    current: Option<WorkspacePersistence>,
    root: Result<PathBuf, String>,
    server_url: Option<&str>,
) -> Result<(WorkspacePersistence, Projection), String> {
    let persistence = match current {
        Some(persistence) => persistence,
        None => WorkspacePersistence::open_initial(root?, server_url)?,
    };
    let projection = persistence.projection()?;
    Ok((persistence, projection))
}

impl AppDatabaseStore {
    pub(crate) fn retry_initial_fetch(&mut self, cx: &mut Context<Self>) {
        self.start_bootstrap(cx);
    }

    pub(crate) fn authentication_changed(&mut self, cx: &mut Context<Self>) {
        let auth = AuthSession::global(cx);
        let scope = auth.scope();
        if self.change_auth_scope(scope) {
            notifications::retire_upcoming(cx);
            self.local_retry_in_flight = true;
            Self::emit_all_changed(cx);
            cx.emit(super::ActionTemplateDataChanged);
            cx.emit(super::EventTemplateDataChanged);
            let generation = self.workspace_generation;
            let server_url = self.server_url.clone();
            let root = workspace_root_from_env();
            cx.spawn(async move |this, cx| {
                let opened = cx
                    .background_spawn(async move {
                        let persistence = match scope {
                            Some(scope) => WorkspacePersistence::open_account(
                                root?,
                                server_url.as_deref().ok_or("No API configured.")?,
                                scope.account_id,
                            )?,
                            None => {
                                WorkspacePersistence::open_initial(root?, server_url.as_deref())?
                            }
                        };
                        Ok::<_, String>((persistence.projection()?, persistence))
                    })
                    .await;
                let _ = this.update(cx, |store, cx| {
                    if store.workspace_generation != generation
                        || AuthSession::global(cx).scope() != scope
                    {
                        return;
                    }
                    store.local_retry_in_flight = false;
                    match opened {
                        Ok((projection, persistence)) => {
                            store.persistence = Some(persistence);
                            store.cmd_tx.scope = scope;
                            store.replace_projection(projection, cx);
                            store.authentication_changed(cx);
                        }
                        Err(error) => store.report_local_store_failure(error, cx),
                    }
                });
            })
            .detach();
            cx.notify();
            return;
        }
        if self.local_retry_in_flight {
            return;
        }
        if auth.is_signed_in() {
            self.retry_initial_fetch(cx);
        } else if self.has_remote_workspace() {
            self.sync_status = SyncStatus::AuthenticationRequired;
            cx.notify();
        }
    }

    pub(crate) fn finish_session_restore(&mut self, cx: &mut Context<Self>) {
        self.authentication_changed(cx);
    }

    pub(crate) fn local_retry_in_flight(&self) -> bool {
        self.local_retry_in_flight
    }

    pub(crate) fn local_data_location() -> Result<PathBuf, String> {
        workspace_root_from_env()
    }

    pub(crate) fn retry_local_data(&mut self, cx: &mut Context<Self>) {
        if self.local_retry_in_flight {
            return;
        }

        self.workspace_generation = self.workspace_generation.wrapping_add(1);
        let generation = self.workspace_generation;
        self.sync_in_flight = false;
        self.sync_request = SyncRequestLatch::default();
        self.sync_retry_not_before = None;
        self.sync_retry_scheduled = None;
        self.outbox_replay_in_flight = false;
        self.local_retry_in_flight = true;
        let persistence = self.persistence.clone();
        let root = workspace_root_from_env();
        let server_url = self.server_url.clone();
        let scope = AuthSession::global(cx).scope();
        let account_id = scope.map(|scope| scope.account_id);
        notifications::retire_upcoming(cx);
        cx.notify();

        cx.spawn(async move |this, cx| {
            let reopened = cx
                .background_spawn(async move {
                    let persistence = if persistence
                        .as_ref()
                        .and_then(WorkspacePersistence::account_id)
                        != account_id
                    {
                        None
                    } else {
                        persistence
                    };
                    if persistence.is_none()
                        && let Some(account_id) = account_id
                    {
                        let persistence = WorkspacePersistence::open_account(
                            root?,
                            server_url.as_deref().ok_or("No API configured.")?,
                            account_id,
                        )?;
                        let projection = persistence.projection()?;
                        Ok((persistence, projection))
                    } else {
                        load_workspace_projection(persistence, root, server_url.as_deref())
                    }
                })
                .await;
            let _ = this.update(cx, |store, cx| {
                if store.workspace_generation != generation
                    || AuthSession::global(cx).scope() != scope
                {
                    return;
                }
                store.local_retry_in_flight = false;

                match reopened {
                    Ok((persistence, projection)) => {
                        store.persistence = Some(persistence);
                        store.cmd_tx.scope = scope;
                        store.sync_status = SyncStatus::Idle;
                        store.replace_projection(projection, cx);
                        if store._worker.is_some() && AuthSession::global(cx).is_signed_in() {
                            store.start_bootstrap(cx);
                        }
                    }
                    Err(error) => store.report_local_store_failure(error, cx),
                }
            });
        })
        .detach();
    }

    pub(crate) fn has_remote_workspace(&self) -> bool {
        self.persistence
            .as_ref()
            .is_some_and(WorkspacePersistence::is_remote)
    }

    pub(crate) fn remove_offline_data(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let Some(persistence) = self.persistence.clone() else {
            let error = "the local workspace is unavailable".to_owned();
            self.report_local_store_failure(error.clone(), cx);
            return Err(error);
        };
        if !persistence.is_remote() {
            let error = "the active workspace has no offline account data".to_owned();
            self.report_local_store_failure(error.clone(), cx);
            return Err(error);
        }

        self.detach_workspace();
        notifications::retire_upcoming(cx);
        Self::emit_all_changed(cx);
        cx.emit(super::ActionTemplateDataChanged);
        cx.emit(super::EventTemplateDataChanged);
        match persistence.purge_remote_and_switch_to_local() {
            Ok((local, projection)) => {
                self.persistence = Some(local);
                self.replace_projection(projection, cx);
                Ok(())
            }
            Err(error) => {
                self.report_local_store_failure(error.clone(), cx);
                Err(error)
            }
        }
    }

    pub(super) fn report_local_store_failure(&mut self, error: String, cx: &mut Context<Self>) {
        tracing::error!(%error, "could not update durable local data");
        self.status = StoreStatus::Error(error.clone());
        cx.emit(DatabaseError { _message: error });
        cx.notify();
    }

    pub(crate) fn workspace_is_current(&self, scope: Option<RequestScope>) -> bool {
        self.is_ready()
            && self.auth_scope == scope
            && self.persistence.as_ref().is_some_and(|persistence| {
                persistence.account_id() == scope.map(|scope| scope.account_id)
            })
    }

    fn change_auth_scope(&mut self, scope: Option<RequestScope>) -> bool {
        if self.auth_scope == scope {
            return false;
        }
        self.detach_workspace();
        self.auth_scope = scope;
        true
    }

    fn detach_workspace(&mut self) {
        self.reset_workspace_session();
        if let Some(persistence) = self.persistence.take() {
            persistence.deactivate();
        }
        self.cmd_tx.scope = None;
        self.actions.clear();
        self.events.clear();
        self.routines.clear();
        self.markers.clear();
        self.signals.clear();
        self.action_templates.clear();
        self.event_templates.clear();
        self.marker_templates.clear();
        self.signal_templates.clear();
        self.dataset_id = None;
        self.last_applied_seq = 0;
        self.pending_count = 0;
        self.blocked_count = 0;
        self.blocked_conflict = None;
        self.status = StoreStatus::AuthenticationRequired;
    }

    fn reset_workspace_session(&mut self) {
        self.workspace_generation = self.workspace_generation.wrapping_add(1);
        self.sync_in_flight = false;
        self.sync_request = SyncRequestLatch::default();
        self.sync_retry_not_before = None;
        self.sync_retry_scheduled = None;
        self.outbox_replay_in_flight = false;
        self.outbox_retry_waiting = false;
        self.outbox_wake_tx = None;
        self.sync_status = SyncStatus::Idle;
        self.last_successful_sync = None;
        self.sse_connected = false;
        self.sync_activity.clear();
        self.history.clear();
    }
}
