use std::collections::HashSet;

use rusqlite::{Connection, Transaction, TransactionBehavior, params};
use subroutine_core::{ApiErrorBody, ApiErrorCode, MutationOperation, Routine};
use uuid::Uuid;

use super::{
    super::{Database, projection::commit_projection, rows::load_rows, sync::load_sync_state},
    OutboxEntry, OutboxStatus,
    conflict::first_patch_resource,
    rows::{load_oldest_outbox, load_outbox, require_outbox_change},
};
use crate::{LocalStoreError, Projection, Result};

impl Database {
    pub(crate) fn block(&mut self, mutation_id: Uuid, error: ApiErrorBody) -> Result<Projection> {
        let Some(entry) = load_outbox(&self.connection, mutation_id)? else {
            return Err(LocalStoreError::OutboxMutationNotFound(mutation_id));
        };
        if entry.status != OutboxStatus::Pending {
            return Err(LocalStoreError::InvalidMutation(
                "only a pending mutation may become blocked".into(),
            ));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        block_outbox(&tx, mutation_id, &error)?;
        commit_projection(tx)
    }

    pub(crate) fn retry_blocked(
        &mut self,
        mutation_id: Uuid,
        replacement_mutation_id: Uuid,
    ) -> Result<Projection> {
        if replacement_mutation_id == mutation_id {
            return Err(LocalStoreError::InvalidMutation(
                "retrying blocked intent requires a new mutation identity".into(),
            ));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state = load_sync_state(&tx)?;
        let entry = require_blocked_fifo_head(&tx, mutation_id)?;
        let dataset_id = state.dataset_id.ok_or_else(|| {
            LocalStoreError::InvalidMutation(
                "blocked remote intent has no canonical dataset identity".into(),
            )
        })?;
        if !state.authoritative_snapshot || entry.mutation.request.dataset_id != dataset_id {
            return Err(LocalStoreError::InvalidMutation(
                "blocked intent no longer matches canonical workspace state".into(),
            ));
        }

        let mut request = entry.mutation.request;
        let mut patch = entry.mutation.optimistic_patch;
        if let MutationOperation::ReorderRoutines { routine_ids } = &mut request.operation {
            let routines: Vec<Routine> = load_rows(&tx, "routine")?;
            if routines.is_empty() {
                return Err(LocalStoreError::InvalidMutation(
                    "no routines remain to reorder; discard this blocked order".into(),
                ));
            }
            let mut remaining = routines
                .iter()
                .map(|routine| routine.id)
                .collect::<HashSet<_>>();
            routine_ids.retain(|id| remaining.remove(id));
            routine_ids.extend(
                routines
                    .iter()
                    .map(|routine| routine.id)
                    .filter(|id| remaining.contains(id)),
            );
            patch.routine_order = Some(routine_ids.clone());
        }
        request.mutation_id = replacement_mutation_id;
        request.base_seq = state.canonical_seq;
        let changed = tx.execute(
            "UPDATE outbox
             SET mutation_id = ?1, request_json = ?2, patch_json = ?3, state = 'pending', sealed = 0,
                 attempt_count = 0, next_attempt_at_ms = NULL, last_error = NULL,
                 receipt_json = NULL, receipt_commit_seq = NULL, blocked_error_json = NULL
             WHERE mutation_id = ?4 AND state = 'blocked'",
            params![
                replacement_mutation_id.to_string(),
                serde_json::to_vec(&request)?,
                serde_json::to_vec(&patch)?,
                mutation_id.to_string()
            ],
        )?;
        require_outbox_change(changed, mutation_id)?;
        commit_projection(tx)
    }

    pub(crate) fn retry_compatibility_rejection(
        &mut self,
        mutation_id: Uuid,
    ) -> Result<Projection> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let entry = require_blocked_fifo_head(&tx, mutation_id)?;
        let request = &entry.mutation.request;
        let error = entry
            .blocked_error
            .as_ref()
            .filter(|error| request.is_compatibility_rejection(error))
            .ok_or_else(|| {
                LocalStoreError::InvalidMutation(
                    "only a compatibility rejection may be retried automatically".into(),
                )
            })?;
        let diagnostic = format!(
            "retrying compatibility rejection ({:?}) for {}; original request retained",
            error.error,
            request.operation.name()
        );
        let changed = tx.execute(
            "UPDATE outbox
             SET state = 'pending', next_attempt_at_ms = NULL,
                 last_error = ?1, blocked_error_json = NULL
             WHERE mutation_id = ?2 AND state = 'blocked'",
            params![diagnostic, mutation_id.to_string()],
        )?;
        require_outbox_change(changed, mutation_id)?;
        commit_projection(tx)
    }

    pub(crate) fn discard_blocked(&mut self, mutation_id: Uuid) -> Result<Projection> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_blocked_fifo_head(&tx, mutation_id)?;
        let changed = tx.execute(
            "DELETE FROM outbox WHERE mutation_id = ?1 AND state = 'blocked'",
            [mutation_id.to_string()],
        )?;
        require_outbox_change(changed, mutation_id)?;
        if let Some(next) = load_oldest_outbox(&tx)?
            && next.status == OutboxStatus::Pending
        {
            let next_id = next.mutation.request.mutation_id;
            let error = ApiErrorBody {
                error: ApiErrorCode::DomainConflict,
                message: "an earlier local change was discarded; confirm this queued change before synchronizing it"
                    .into(),
                mutation_id: Some(next_id),
                resource: first_patch_resource(&next.mutation.optimistic_patch),
                current_seq: Some(load_sync_state(&tx)?.canonical_seq),
                current_dataset_id: Some(next.mutation.request.dataset_id),
                retryable: false,
            };
            block_outbox(&tx, next_id, &error)?;
        }
        commit_projection(tx)
    }
}

fn require_blocked_fifo_head(connection: &Connection, mutation_id: Uuid) -> Result<OutboxEntry> {
    let Some(entry) = load_oldest_outbox(connection)? else {
        return Err(LocalStoreError::OutboxMutationNotFound(mutation_id));
    };
    if entry.mutation.request.mutation_id != mutation_id {
        return Err(LocalStoreError::InvalidMutation(
            "conflict resolution no longer targets the FIFO head".into(),
        ));
    }
    if entry.status != OutboxStatus::Blocked {
        return Err(LocalStoreError::InvalidMutation(
            "only a blocked FIFO head may be explicitly resolved".into(),
        ));
    }
    Ok(entry)
}

pub(in crate::db) fn block_outbox(
    tx: &Transaction<'_>,
    mutation_id: Uuid,
    error: &ApiErrorBody,
) -> Result<()> {
    let changed = tx.execute(
        "UPDATE outbox
         SET state = 'blocked', blocked_error_json = ?1,
             next_attempt_at_ms = NULL, last_error = NULL
         WHERE mutation_id = ?2 AND state = 'pending'",
        params![serde_json::to_vec(error)?, mutation_id.to_string()],
    )?;
    require_outbox_change(changed, mutation_id)
}
