mod blocked;
pub(crate) mod conflict;
mod receipts;
mod rows;
pub(crate) mod stale;

use chrono::{DateTime, Utc};
use rusqlite::{TransactionBehavior, params};
use subroutine_core::{
    ApiErrorBody, ApiErrorCode, ClientMutation, MUTATION_PROTOCOL_VERSION, MutationReceipt,
};
use uuid::Uuid;

use super::{
    Database,
    projection::{commit_projection, load_projection},
    sync::load_sync_state,
};
use crate::{LocalStoreError, Projection, Result, WorkspaceIdentity};
pub(super) use blocked::block_outbox;
use conflict::patches_overlap;
pub(super) use receipts::retire_confirmed;
pub(super) use rows::{load_oldest_outbox, load_outbox_entries, outbox_count};
use rows::{load_outbox, require_outbox_change};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutboxStatus {
    Pending,
    AwaitingCanonical,
    Blocked,
}

#[derive(Debug)]
pub struct OutboxEntry {
    pub position: i64,
    pub mutation: ClientMutation,
    pub status: OutboxStatus,
    pub sealed: bool,
    pub attempt_count: u32,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub receipt: Option<MutationReceipt>,
    pub blocked_error: Option<ApiErrorBody>,
}

impl OutboxEntry {
    pub fn is_legacy_stale_base_block(&self) -> bool {
        if self.status != OutboxStatus::Blocked {
            return false;
        }
        let request = &self.mutation.request;
        self.blocked_error.as_ref().is_some_and(|error| {
            error.error == ApiErrorCode::StaleBase
                && error.mutation_id == Some(request.mutation_id)
                && error.current_dataset_id == Some(request.dataset_id)
                && error
                    .current_seq
                    .is_some_and(|current_seq| current_seq > request.base_seq)
        })
    }
}

impl Database {
    pub(crate) fn enqueue(&mut self, mut mutation: ClientMutation) -> Result<Projection> {
        let patch_json = serde_json::to_vec(&mutation.optimistic_patch)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state = load_sync_state(&tx)?;
        if !matches!(self.identity, WorkspaceIdentity::Remote { .. }) {
            return Err(LocalStoreError::InvalidMutation(
                "a replay-safe server mutation requires a remote workspace".into(),
            ));
        }
        if !state.authoritative_snapshot {
            return Err(LocalStoreError::InvalidMutation(
                "remote mutations require an authoritative snapshot".into(),
            ));
        }
        if mutation.request.protocol_version != MUTATION_PROTOCOL_VERSION {
            return Err(LocalStoreError::InvalidMutation(format!(
                "unsupported mutation protocol version {}",
                mutation.request.protocol_version
            )));
        }
        if mutation.request.client_id != self.identity.client_id() {
            return Err(LocalStoreError::InvalidMutation(
                "request client identity does not match this workspace".into(),
            ));
        }
        if state.dataset_id != Some(mutation.request.dataset_id) {
            return Err(LocalStoreError::InvalidMutation(
                "request dataset does not match canonical state".into(),
            ));
        }
        if state.canonical_seq != mutation.request.base_seq {
            return Err(LocalStoreError::InvalidMutation(format!(
                "request base sequence {} does not match canonical sequence {}",
                mutation.request.base_seq, state.canonical_seq
            )));
        }

        for entry in load_outbox_entries(&tx)? {
            if patches_overlap(&mutation.optimistic_patch, &entry.mutation.optimistic_patch) {
                mutation.request.base_seq = mutation
                    .request
                    .base_seq
                    .min(entry.mutation.request.base_seq);
            }
        }
        let request_json = serde_json::to_vec(&mutation.request)?;
        tx.execute(
            "INSERT INTO outbox (
                mutation_id, request_json, patch_json, created_at_ms
             ) VALUES (?1, ?2, ?3, ?4)",
            params![
                mutation.request.mutation_id.to_string(),
                request_json,
                patch_json,
                Utc::now().timestamp_millis()
            ],
        )?;

        commit_projection(tx)
    }

    pub(crate) fn oldest_outbox(&self) -> Result<Option<OutboxEntry>> {
        load_oldest_outbox(&self.connection)
    }

    pub(crate) fn next_sendable(&mut self, now: DateTime<Utc>) -> Result<Option<OutboxEntry>> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        load_projection(&tx)?;
        let Some(mut entry) = load_oldest_outbox(&tx)? else {
            tx.commit()?;
            return Ok(None);
        };
        if entry.status != OutboxStatus::Pending
            || entry.next_attempt_at.is_some_and(|retry_at| retry_at > now)
        {
            tx.commit()?;
            return Ok(None);
        }

        entry.attempt_count = entry.attempt_count.checked_add(1).ok_or_else(|| {
            LocalStoreError::InvalidMutation("outbox attempt count overflow".into())
        })?;
        entry.sealed = true;
        entry.last_error = None;
        tx.execute(
            "UPDATE outbox
             SET sealed = 1, attempt_count = ?1, last_error = NULL
             WHERE mutation_id = ?2",
            params![
                entry.attempt_count,
                entry.mutation.request.mutation_id.to_string()
            ],
        )?;
        tx.commit()?;
        Ok(Some(entry))
    }

    pub(crate) fn rebase_unsealed(&mut self, mutation_id: Uuid, new_base_seq: i64) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state = load_sync_state(&tx)?;
        if new_base_seq != state.canonical_seq {
            return Err(LocalStoreError::InvalidMutation(format!(
                "new base sequence {new_base_seq} does not match canonical sequence {}",
                state.canonical_seq
            )));
        }
        let Some(entry) = load_outbox(&tx, mutation_id)? else {
            return Err(LocalStoreError::OutboxMutationNotFound(mutation_id));
        };
        if entry.sealed || entry.status != OutboxStatus::Pending {
            return Err(LocalStoreError::InvalidMutation(
                "only an unsealed pending mutation may be rebased".into(),
            ));
        }
        if entry.mutation.request.base_seq != new_base_seq {
            return Err(LocalStoreError::InvalidMutation(
                "rebasing requires a checked stale delta or explicit conflict resolution".into(),
            ));
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn record_retry(
        &mut self,
        mutation_id: Uuid,
        next_attempt_at: DateTime<Utc>,
        error: String,
    ) -> Result<()> {
        let changed = self.connection.execute(
            "UPDATE outbox
             SET state = 'pending', next_attempt_at_ms = ?1, last_error = ?2
             WHERE mutation_id = ?3 AND state = 'pending'",
            params![
                next_attempt_at.timestamp_millis(),
                error,
                mutation_id.to_string()
            ],
        )?;
        require_outbox_change(changed, mutation_id)
    }
}
