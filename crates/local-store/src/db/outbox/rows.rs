use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension};
use subroutine_core::ClientMutation;
use uuid::Uuid;

use super::{OutboxEntry, OutboxStatus};
use crate::{LocalStoreError, Result};

pub(in crate::db) fn outbox_count(connection: &Connection) -> Result<usize> {
    connection
        .query_row("SELECT COUNT(*) FROM outbox", [], |row| row.get(0))
        .map_err(Into::into)
}

struct StoredOutbox {
    position: i64,
    request_json: Vec<u8>,
    patch_json: Vec<u8>,
    state: String,
    sealed: bool,
    attempt_count: u32,
    next_attempt_at_ms: Option<i64>,
    last_error: Option<String>,
    receipt_json: Option<Vec<u8>>,
    blocked_error_json: Option<Vec<u8>>,
}

pub(in crate::db) fn load_oldest_outbox(connection: &Connection) -> Result<Option<OutboxEntry>> {
    load_stored_outbox(
        connection,
        "SELECT position, request_json, patch_json, state, sealed, attempt_count,
                next_attempt_at_ms, last_error, receipt_json, blocked_error_json
         FROM outbox ORDER BY position LIMIT 1",
        [],
    )
}

pub(in crate::db) fn load_outbox(
    connection: &Connection,
    mutation_id: Uuid,
) -> Result<Option<OutboxEntry>> {
    load_stored_outbox(
        connection,
        "SELECT position, request_json, patch_json, state, sealed, attempt_count,
                next_attempt_at_ms, last_error, receipt_json, blocked_error_json
         FROM outbox WHERE mutation_id = ?1",
        [mutation_id.to_string()],
    )
}

fn load_stored_outbox<P: rusqlite::Params>(
    connection: &Connection,
    sql: &str,
    params: P,
) -> Result<Option<OutboxEntry>> {
    let stored = connection
        .query_row(sql, params, |row| {
            Ok(StoredOutbox {
                position: row.get(0)?,
                request_json: row.get(1)?,
                patch_json: row.get(2)?,
                state: row.get(3)?,
                sealed: row.get::<_, i64>(4)? != 0,
                attempt_count: row.get(5)?,
                next_attempt_at_ms: row.get(6)?,
                last_error: row.get(7)?,
                receipt_json: row.get(8)?,
                blocked_error_json: row.get(9)?,
            })
        })
        .optional()?;
    stored.map(decode_outbox).transpose()
}

fn decode_outbox(stored: StoredOutbox) -> Result<OutboxEntry> {
    let status = match stored.state.as_str() {
        "pending" => OutboxStatus::Pending,
        "awaiting_canonical" => OutboxStatus::AwaitingCanonical,
        "blocked" => OutboxStatus::Blocked,
        other => {
            return Err(LocalStoreError::InvalidMutation(format!(
                "unknown outbox state {other}"
            )));
        }
    };
    let request = serde_json::from_slice(&stored.request_json)?;
    let optimistic_patch = serde_json::from_slice(&stored.patch_json)?;
    let next_attempt_at = stored
        .next_attempt_at_ms
        .map(|timestamp| {
            DateTime::<Utc>::from_timestamp_millis(timestamp).ok_or_else(|| {
                LocalStoreError::InvalidMutation(format!(
                    "invalid outbox retry timestamp {timestamp}"
                ))
            })
        })
        .transpose()?;
    Ok(OutboxEntry {
        position: stored.position,
        mutation: ClientMutation {
            request,
            optimistic_patch,
        },
        status,
        sealed: stored.sealed,
        attempt_count: stored.attempt_count,
        next_attempt_at,
        last_error: stored.last_error,
        receipt: stored
            .receipt_json
            .map(|value| serde_json::from_slice(&value))
            .transpose()?,
        blocked_error: stored
            .blocked_error_json
            .map(|value| serde_json::from_slice(&value))
            .transpose()?,
    })
}

pub(in crate::db) fn require_outbox_change(changed: usize, mutation_id: Uuid) -> Result<()> {
    if changed == 0 {
        Err(LocalStoreError::OutboxMutationNotFound(mutation_id))
    } else {
        Ok(())
    }
}
