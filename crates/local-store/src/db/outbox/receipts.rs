use rusqlite::{Transaction, params};
use subroutine_core::{MutationEffect, MutationReceipt};
use uuid::Uuid;

use super::{
    super::Database,
    OutboxStatus,
    rows::{load_outbox, load_outbox_entries, require_outbox_change},
};
use crate::{LocalStoreError, Result};

impl Database {
    pub(crate) fn record_receipt(&mut self, receipt: MutationReceipt) -> Result<()> {
        let Some(entry) = load_outbox(&self.connection, receipt.mutation_id)? else {
            return Err(LocalStoreError::OutboxMutationNotFound(receipt.mutation_id));
        };
        if entry.status != OutboxStatus::Pending || !entry.sealed {
            return Err(LocalStoreError::InvalidMutation(
                "a receipt may only complete a sealed pending mutation".into(),
            ));
        }
        let request = &entry.mutation.request;
        let valid_commit_seq = match receipt.effect {
            MutationEffect::Applied => request
                .base_seq
                .checked_add(1)
                .is_some_and(|expected| receipt.commit_seq == expected),
            MutationEffect::NoOp => receipt.commit_seq == request.base_seq,
        };
        if receipt.protocol_version != request.protocol_version
            || receipt.dataset_id != request.dataset_id
            || receipt.client_id != request.client_id
            || receipt.base_seq != request.base_seq
            || !valid_commit_seq
        {
            return Err(LocalStoreError::InvalidMutation(
                "receipt does not match the durable request".into(),
            ));
        }
        let receipt_json = serde_json::to_vec(&receipt)?;
        let changed = self.connection.execute(
            "UPDATE outbox
             SET state = 'awaiting_canonical', receipt_json = ?1,
                 receipt_commit_seq = ?2, next_attempt_at_ms = NULL,
                 last_error = NULL, blocked_error_json = NULL
             WHERE mutation_id = ?3",
            params![
                receipt_json,
                receipt.commit_seq,
                receipt.mutation_id.to_string()
            ],
        )?;
        require_outbox_change(changed, receipt.mutation_id)
    }
}

fn advance_pending(tx: &Transaction<'_>, receipt: &MutationReceipt) -> Result<()> {
    if receipt.base_seq == receipt.commit_seq {
        return Ok(());
    }
    for mut entry in load_outbox_entries(tx)? {
        let request = &mut entry.mutation.request;
        if entry.status != OutboxStatus::Pending
            || entry.sealed
            || request.dataset_id != receipt.dataset_id
            || request.base_seq != receipt.base_seq
        {
            continue;
        }
        request.base_seq = receipt.commit_seq;
        tx.execute(
            "UPDATE outbox SET request_json = ?1
             WHERE mutation_id = ?2 AND state = 'pending' AND sealed = 0",
            params![
                serde_json::to_vec(request)?,
                request.mutation_id.to_string()
            ],
        )?;
    }
    Ok(())
}

pub(in crate::db) fn retire_confirmed(
    tx: &Transaction<'_>,
    dataset_id: Option<Uuid>,
    canonical_seq: i64,
) -> Result<()> {
    let Some(dataset_id) = dataset_id else {
        return Ok(());
    };
    let mut statement = tx.prepare(
        "SELECT mutation_id, receipt_json FROM outbox
         WHERE state = 'awaiting_canonical' AND receipt_commit_seq <= ?1
         ORDER BY position",
    )?;
    let receipts = statement
        .query_map([canonical_seq], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(statement);
    for (mutation_id, receipt_json) in receipts {
        let receipt: MutationReceipt = serde_json::from_slice(&receipt_json)?;
        if receipt.dataset_id == dataset_id {
            advance_pending(tx, &receipt)?;
            tx.execute("DELETE FROM outbox WHERE mutation_id = ?1", [mutation_id])?;
        }
    }
    Ok(())
}
