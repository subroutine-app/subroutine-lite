use subroutine_core::ChangeBatch;

use super::AppState;
use crate::{
    db,
    error::{AppError, Result},
};

const TENANT_LIFECYCLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

impl AppState {
    pub async fn reconcile_recurrence(&self) -> Result<usize> {
        let scopes = db::TenantScope::all_users(&self.pool).await?;
        let mut total = 0;
        for scope in scopes {
            let user_id = scope.user_id;
            let state = self.clone();
            let mut task = tokio::spawn(async move { state.run_tenant_lifecycle(&scope).await });

            match tokio::time::timeout(TENANT_LIFECYCLE_TIMEOUT, &mut task).await {
                Ok(Ok(changed)) => total += changed,
                Ok(Err(error)) => {
                    tracing::error!(%user_id, ?error, "tenant lifecycle task panicked");
                }
                Err(_) => {
                    task.abort();
                    let _ = task.await;
                    tracing::error!(%user_id, "tenant lifecycle task timed out and was aborted");
                }
            }
        }
        Ok(total)
    }

    async fn run_tenant_lifecycle(&self, scope: &db::TenantScope) -> usize {
        let user_id = scope.user_id;
        let mut changed = match self.reconcile_recurrence_in(scope).await {
            Ok(count) => count,
            Err(error) => {
                tracing::error!(%user_id, ?error, "failed to reconcile tenant recurrence");
                0
            }
        };
        match self
            .apply_from_snapshot_in(scope, |snapshot| {
                Ok(crate::ops::pipeline::auto_queue(snapshot))
            })
            .await
        {
            Ok(queued) => changed += queued.len(),
            Err(error) => {
                tracing::error!(%user_id, ?error, "failed to auto-queue tenant actions");
            }
        }
        changed
    }

    async fn reconcile_recurrence_in(&self, scope: &db::TenantScope) -> Result<usize> {
        let mut mutation = scope.begin_mutation().await?;
        let snapshot = db::snapshot_in(mutation.connection(), scope.user_id, self.settings).await?;
        let recurrence = crate::ops::recurrence::reconcile(&snapshot);
        let total = recurrence.value.total();
        let mut events = recurrence.changes.change_events();

        let mut seq = None;

        if !recurrence.changes.is_empty() {
            let change_seq = mutation.next_change_seq().await?;
            seq = Some(change_seq);
            match db::apply_changes_in(
                mutation.connection(),
                scope.user_id,
                change_seq,
                &recurrence.changes,
            )
            .await?
            {
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
        }

        if total > 0 {
            let snapshot =
                db::snapshot_in(mutation.connection(), scope.user_id, self.settings).await?;
            let refresh = crate::ops::pipeline::refresh(&snapshot);
            for event in refresh.changes.change_events() {
                if !events.contains(&event) {
                    events.push(event);
                }
            }
            if !refresh.changes.is_empty() {
                let change_seq = match seq {
                    Some(seq) => seq,
                    None => {
                        let allocated = mutation.next_change_seq().await?;
                        seq = Some(allocated);
                        allocated
                    }
                };
                match db::apply_changes_in(
                    mutation.connection(),
                    scope.user_id,
                    change_seq,
                    &refresh.changes,
                )
                .await?
                {
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
            }
        }

        if events.is_empty() {
            mutation.rollback().await?;
            return Ok(total);
        }

        let seq = seq.expect("a persisted recurrence batch has a sequence");
        mutation.commit().await?;
        self.announce(scope.user_id, ChangeBatch::committed(seq, events));
        Ok(total)
    }
}
