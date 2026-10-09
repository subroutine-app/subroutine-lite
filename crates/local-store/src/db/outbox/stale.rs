use rusqlite::{Connection, Transaction, TransactionBehavior, params};
use subroutine_core::{AllData, ApiErrorBody, ApiErrorCode, DataDelta, ResourceKey};
use uuid::Uuid;

use super::{
    super::{
        Database, non_nil_uuid,
        projection::{commit_projection, load_projection},
        rows::{apply_delta_rows, insert_snapshot_rows},
        sync::load_sync_state,
    },
    OutboxEntry, OutboxStatus,
    blocked::block_outbox,
    conflict::first_patch_delta_conflict,
    receipts::retire_confirmed,
    rows::{load_oldest_outbox, load_outbox_entries, require_outbox_change},
};
use crate::{LocalStoreError, Projection, Result, SyncState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StaleResolutionKind {
    Rebased {
        mutation_id: Uuid,
        base_seq: i64,
    },
    Blocked {
        conflicting_resource: Option<ResourceKey>,
    },
}

#[derive(Debug)]
pub struct StaleResolution {
    pub projection: Projection,
    pub kind: StaleResolutionKind,
}

impl Database {
    pub(crate) fn resolve_stale_delta(
        &mut self,
        mutation_id: Uuid,
        replacement_mutation_id: Uuid,
        since: i64,
        delta: DataDelta,
    ) -> Result<StaleResolution> {
        let incoming_dataset = non_nil_uuid(delta.dataset_id).ok_or_else(|| {
            LocalStoreError::InvalidMutation("delta is missing its dataset identity".into())
        })?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state = load_sync_state(&tx)?;
        let entry = require_stale_fifo_head(&tx, mutation_id)?;
        if entry.mutation.request.base_seq != since {
            return Err(LocalStoreError::InvalidMutation(format!(
                "stale delta starts at {since}, but the durable request starts at {}",
                entry.mutation.request.base_seq
            )));
        }
        if state.dataset_id != Some(incoming_dataset)
            || entry.mutation.request.dataset_id != incoming_dataset
        {
            return Err(LocalStoreError::DatasetMismatch {
                current: state.dataset_id.unwrap_or_else(Uuid::nil),
                incoming: incoming_dataset,
            });
        }
        if delta.seq < state.canonical_seq || delta.seq <= since {
            return Err(LocalStoreError::SequenceRegression {
                current: state.canonical_seq.max(since.saturating_add(1)),
                incoming: delta.seq,
            });
        }
        if entry.sealed && replacement_mutation_id == mutation_id {
            return Err(LocalStoreError::InvalidMutation(
                "a server-rejected sealed request requires a new mutation identity".into(),
            ));
        }
        if !entry.sealed && replacement_mutation_id != mutation_id {
            return Err(LocalStoreError::InvalidMutation(
                "an unsealed request must retain its mutation identity".into(),
            ));
        }

        let conflict = first_patch_delta_conflict(&entry.mutation.optimistic_patch, &delta);
        reconcile_queued_delta(&tx, entry.position, since, &delta)?;
        apply_delta_rows(&tx, &delta)?;
        tx.execute(
            "UPDATE sync_state SET canonical_seq = ?1 WHERE singleton = 1",
            [delta.seq],
        )?;
        retire_confirmed(&tx, Some(incoming_dataset), delta.seq)?;

        let kind = if let Some(resource) = conflict {
            let error = ApiErrorBody {
                error: ApiErrorCode::DomainConflict,
                message: "this item changed on the server; review your local change before syncing"
                    .into(),
                mutation_id: Some(mutation_id),
                resource: Some(resource),
                current_seq: Some(delta.seq),
                current_dataset_id: Some(incoming_dataset),
                retryable: false,
            };
            block_outbox(&tx, mutation_id, &error)?;
            StaleResolutionKind::Blocked {
                conflicting_resource: Some(resource),
            }
        } else {
            let mut request = entry.mutation.request;
            request.mutation_id = replacement_mutation_id;
            request.base_seq = delta.seq;
            let changed = tx.execute(
                "UPDATE outbox
                 SET mutation_id = ?1, request_json = ?2, sealed = 0,
                     attempt_count = 0, next_attempt_at_ms = NULL,
                     last_error = NULL, blocked_error_json = NULL
                 WHERE mutation_id = ?3 AND state = 'pending'",
                params![
                    replacement_mutation_id.to_string(),
                    serde_json::to_vec(&request)?,
                    mutation_id.to_string()
                ],
            )?;
            require_outbox_change(changed, mutation_id)?;
            StaleResolutionKind::Rebased {
                mutation_id: replacement_mutation_id,
                base_seq: delta.seq,
            }
        };
        let projection = load_projection(&tx)?;
        let kind = projection
            .blocked_conflict
            .as_ref()
            .map_or(kind, |conflict| StaleResolutionKind::Blocked {
                conflicting_resource: conflict.error.resource,
            });
        tx.commit()?;
        Ok(StaleResolution { projection, kind })
    }

    pub(crate) fn resolve_stale_snapshot(
        &mut self,
        mutation_id: Uuid,
        data: AllData,
        expected: SyncState,
    ) -> Result<StaleResolution> {
        let incoming_dataset = non_nil_uuid(data.dataset_id).ok_or_else(|| {
            LocalStoreError::InvalidMutation(
                "remote snapshots must carry a dataset identity".into(),
            )
        })?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_sync_state(&tx)?;
        if current != expected {
            return Err(LocalStoreError::StaleCanonicalState);
        }
        let entry = require_stale_fifo_head(&tx, mutation_id)?;
        if current.dataset_id != Some(incoming_dataset)
            || entry.mutation.request.dataset_id != incoming_dataset
        {
            return Err(LocalStoreError::DatasetMismatch {
                current: current.dataset_id.unwrap_or_else(Uuid::nil),
                incoming: incoming_dataset,
            });
        }
        if data.seq < current.canonical_seq {
            return Err(LocalStoreError::SequenceRegression {
                current: current.canonical_seq,
                incoming: data.seq,
            });
        }

        tx.execute("DELETE FROM canonical_resources", [])?;
        insert_snapshot_rows(&tx, &data)?;
        tx.execute(
            "UPDATE sync_state SET canonical_seq = ?1, authoritative_snapshot = 1 WHERE singleton = 1",
            [data.seq],
        )?;
        retire_confirmed(&tx, Some(incoming_dataset), data.seq)?;
        let error = ApiErrorBody {
            error: ApiErrorCode::DomainConflict,
            message: "server data was refreshed; review your local change before syncing".into(),
            mutation_id: Some(mutation_id),
            resource: None,
            current_seq: Some(data.seq),
            current_dataset_id: Some(incoming_dataset),
            retryable: false,
        };
        block_outbox(&tx, mutation_id, &error)?;
        let projection = commit_projection(tx)?;
        Ok(StaleResolution {
            projection,
            kind: StaleResolutionKind::Blocked {
                conflicting_resource: None,
            },
        })
    }
}

fn reconcile_queued_delta(
    tx: &Transaction<'_>,
    head_position: i64,
    since: i64,
    delta: &DataDelta,
) -> Result<()> {
    for mut entry in load_outbox_entries(tx)? {
        let request = &mut entry.mutation.request;
        if entry.position <= head_position
            || entry.status != OutboxStatus::Pending
            || entry.sealed
            || request.dataset_id != delta.dataset_id
            || request.base_seq < since
            || request.base_seq >= delta.seq
        {
            continue;
        }
        if let Some(resource) = first_patch_delta_conflict(&entry.mutation.optimistic_patch, delta)
        {
            let error = ApiErrorBody {
                error: ApiErrorCode::DomainConflict,
                message: "this item changed on the server; review your local change before syncing"
                    .into(),
                mutation_id: Some(request.mutation_id),
                resource: Some(resource),
                current_seq: Some(delta.seq),
                current_dataset_id: Some(delta.dataset_id),
                retryable: false,
            };
            block_outbox(tx, request.mutation_id, &error)?;
        } else {
            request.base_seq = delta.seq;
            tx.execute(
                "UPDATE outbox SET request_json = ?1
                 WHERE mutation_id = ?2 AND state = 'pending' AND sealed = 0",
                params![
                    serde_json::to_vec(request)?,
                    request.mutation_id.to_string()
                ],
            )?;
        }
    }
    Ok(())
}

fn require_stale_fifo_head(connection: &Connection, mutation_id: Uuid) -> Result<OutboxEntry> {
    let Some(entry) = load_oldest_outbox(connection)? else {
        return Err(LocalStoreError::OutboxMutationNotFound(mutation_id));
    };
    if entry.mutation.request.mutation_id != mutation_id {
        return Err(LocalStoreError::InvalidMutation(
            "stale resolution no longer targets the FIFO head".into(),
        ));
    }
    if entry.status != OutboxStatus::Pending {
        return Err(LocalStoreError::InvalidMutation(
            "only a pending FIFO head may be resolved after a stale base".into(),
        ));
    }
    Ok(entry)
}
