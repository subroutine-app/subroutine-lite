mod lifecycle;
mod mutation;
mod operations;
mod validation;

use std::ops::Deref;

use sqlx::PgPool;
use subroutine_core::{ChangeBatch, ChangeEvent, DataDelta};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::{
    auth::{AuthenticatedIdentity, Verifier},
    db,
    error::{AppError, Result},
    ops::{Changes, Delete, Outcome, Settings, Snapshot},
};

const BROADCAST_CAPACITY: usize = 64;

#[derive(Debug, Clone)]
pub(crate) struct TenantChange {
    user_id: Uuid,
    batch: ChangeBatch,
}

impl TenantChange {
    pub(crate) fn batch_for(self, user_id: Uuid) -> Option<ChangeBatch> {
        (self.user_id == user_id).then_some(self.batch)
    }
}

#[derive(Clone)]
pub struct AppState {
    pool: PgPool,
    pub settings: Settings,

    auth: Option<Verifier>,
    changes: broadcast::Sender<TenantChange>,
}

#[derive(Clone)]
pub(crate) struct TenantState {
    app: AppState,
    scope: db::TenantScope,
}

impl AppState {
    pub fn new(pool: PgPool, settings: Settings) -> Self {
        let (changes, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            pool,
            settings,
            changes,

            auth: None,
        }
    }

    pub(crate) fn with_auth(mut self, verifier: Verifier) -> Self {
        self.auth = Some(verifier);
        self
    }

    pub(crate) fn auth_verifier(&self) -> Option<&Verifier> {
        self.auth.as_ref()
    }


    pub(crate) async fn tenant_state(
        &self,
        identity: &AuthenticatedIdentity,
    ) -> Result<TenantState> {
        let scope =
            db::TenantScope::for_identity(self.pool.clone(), &identity.issuer, &identity.subject)
                .await?;
        Ok(TenantState {
            app: self.clone(),
            scope,
        })
    }

    async fn require_in<R: db::Record>(
        &self,
        scope: &db::TenantScope,
        kind: &'static str,
        id: Uuid,
    ) -> Result<R> {
        db::fetch_by_id::<R>(scope, id)
            .await?
            .ok_or_else(|| AppError::not_found(format!("{kind} {id} not found")))
    }

    async fn apply_in<T>(&self, scope: &db::TenantScope, outcome: Outcome<T>) -> Result<T> {
        if outcome.changes.is_empty() {
            return Ok(outcome.value);
        }
        let mutation = scope.begin_mutation().await?;
        self.finish_mutation(scope.user_id, mutation, outcome).await
    }

    async fn apply_from_snapshot_in<T, F>(&self, scope: &db::TenantScope, operation: F) -> Result<T>
    where
        F: FnOnce(&Snapshot) -> Result<Outcome<T>>,
    {
        let mut mutation = scope.begin_mutation().await?;
        let snapshot = db::snapshot_in(mutation.connection(), scope.user_id, self.settings).await?;
        let outcome = match operation(&snapshot) {
            Ok(outcome) => outcome,
            Err(error) => {
                mutation.rollback().await?;
                return Err(error);
            }
        };
        self.finish_mutation(scope.user_id, mutation, outcome).await
    }

    async fn apply_required_in<R, T, F>(
        &self,
        scope: &db::TenantScope,
        kind: &'static str,
        id: Uuid,
        operation: F,
    ) -> Result<T>
    where
        R: db::Record,
        F: FnOnce(R) -> Result<Outcome<T>>,
    {
        let mut mutation = scope.begin_mutation().await?;
        let record = db::fetch_by_id_in::<R>(mutation.connection(), scope.user_id, id).await?;
        let Some(record) = record else {
            mutation.rollback().await?;
            return Err(AppError::not_found(format!("{kind} {id} not found")));
        };
        let outcome = match operation(record) {
            Ok(outcome) => outcome,
            Err(error) => {
                mutation.rollback().await?;
                return Err(error);
            }
        };
        self.finish_mutation(scope.user_id, mutation, outcome).await
    }

    async fn apply_optional_in<R, T, F>(
        &self,
        scope: &db::TenantScope,
        id: Uuid,
        operation: F,
    ) -> Result<T>
    where
        R: db::Record,
        F: FnOnce(Option<R>) -> Result<Outcome<T>>,
    {
        let mut mutation = scope.begin_mutation().await?;
        let record = db::fetch_by_id_in::<R>(mutation.connection(), scope.user_id, id).await?;
        let outcome = match operation(record) {
            Ok(outcome) => outcome,
            Err(error) => {
                mutation.rollback().await?;
                return Err(error);
            }
        };
        self.finish_mutation(scope.user_id, mutation, outcome).await
    }

    async fn finish_mutation<T>(
        &self,
        user_id: Uuid,
        mut mutation: db::TenantMutation,
        outcome: Outcome<T>,
    ) -> Result<T> {
        if outcome.changes.is_empty() {
            mutation.rollback().await?;
            return Ok(outcome.value);
        }

        let events = outcome.changes.change_events();
        let seq = mutation.next_change_seq().await?;
        match db::apply_changes_in(mutation.connection(), user_id, seq, &outcome.changes).await? {
            db::ApplyResult::Applied => {}
            db::ApplyResult::Missing(target) => {
                mutation.rollback().await?;
                return Err(AppError::not_found(format!(
                    "{} {} not found",
                    target.label(),
                    target.id()
                )));
            }
        }
        mutation.commit().await?;
        self.announce(user_id, ChangeBatch::committed(seq, events));
        Ok(outcome.value)
    }

    async fn trash_in(&self, scope: &db::TenantScope, target: Delete) -> Result<()> {
        let mut changes = Changes::default();
        changes.delete(target);
        self.apply_in(scope, Outcome::new((), changes)).await
    }

    async fn data_delta_in(&self, scope: &db::TenantScope, since: i64) -> Result<DataDelta> {
        if since < 0 {
            return Err(AppError::bad_request(
                "data cursor must be zero or greater".into(),
            ));
        }
        let mut read = scope.begin_mutation().await?;
        let (dataset_id, current_seq) = read.identity().await?;
        if since > current_seq {
            read.rollback().await?;
            return Err(AppError::bad_request(format!(
                "data cursor {since} is ahead of current sequence {current_seq}"
            )));
        }
        let delta = db::data_delta_in(
            read.connection(),
            scope.user_id,
            dataset_id,
            since,
            current_seq,
        )
        .await?;
        read.rollback().await?;
        Ok(delta)
    }

    fn announce(&self, user_id: Uuid, batch: ChangeBatch) {
        let _ = self.changes.send(TenantChange { user_id, batch });
    }
}

impl TenantState {
    pub(crate) fn scope(&self) -> &db::TenantScope {
        &self.scope
    }

    pub(crate) async fn require<R: db::Record>(&self, kind: &'static str, id: Uuid) -> Result<R> {
        self.app.require_in(&self.scope, kind, id).await
    }

    pub(crate) async fn apply<T>(&self, outcome: Outcome<T>) -> Result<T> {
        self.app.apply_in(&self.scope, outcome).await
    }

    pub(crate) async fn apply_from_snapshot<T, F>(&self, operation: F) -> Result<T>
    where
        F: FnOnce(&Snapshot) -> Result<Outcome<T>>,
    {
        self.app
            .apply_from_snapshot_in(&self.scope, operation)
            .await
    }

    pub(crate) async fn apply_required<R, T, F>(
        &self,
        kind: &'static str,
        id: Uuid,
        operation: F,
    ) -> Result<T>
    where
        R: db::Record,
        F: FnOnce(R) -> Result<Outcome<T>>,
    {
        self.app
            .apply_required_in::<R, T, F>(&self.scope, kind, id, operation)
            .await
    }

    pub(crate) async fn apply_optional<R, T, F>(&self, id: Uuid, operation: F) -> Result<T>
    where
        R: db::Record,
        F: FnOnce(Option<R>) -> Result<Outcome<T>>,
    {
        self.app
            .apply_optional_in::<R, T, F>(&self.scope, id, operation)
            .await
    }

    pub(crate) async fn trash(&self, target: Delete) -> Result<()> {
        self.app.trash_in(&self.scope, target).await
    }

    pub(crate) async fn data_delta(&self, since: i64) -> Result<DataDelta> {
        self.app.data_delta_in(&self.scope, since).await
    }

    pub(crate) fn announce(&self, seq: i64, event: ChangeEvent) {
        self.app
            .announce(self.scope.user_id, ChangeBatch::committed(seq, vec![event]));
    }

    pub(crate) async fn current_change_seq(&self) -> Result<i64> {
        Ok(self.scope.current_change_seq().await?)
    }

    pub(crate) fn subscribe_changes(&self) -> broadcast::Receiver<TenantChange> {
        self.app.changes.subscribe()
    }
}

impl Deref for TenantState {
    type Target = AppState;

    fn deref(&self) -> &Self::Target {
        &self.app
    }
}
