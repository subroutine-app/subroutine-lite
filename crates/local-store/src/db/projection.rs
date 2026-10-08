pub(crate) mod patch;

use rusqlite::Connection;
use subroutine_core::{AllData, ApiErrorBody, MutationRequest, OptimisticPatch};
use uuid::Uuid;

use super::{
    outbox::{load_oldest_outbox, outbox_count},
    rows::load_rows,
    sync::load_sync_state,
};
use crate::{LocalStoreError, OutboxStatus, Result};
use patch::apply_remote_patch;

#[derive(Debug)]
pub struct Projection {
    pub data: AllData,
    pub authoritative_snapshot: bool,
    pub pending_count: usize,
    pub blocked_count: usize,
    pub blocked_conflict: Option<BlockedConflict>,
}

#[derive(Clone, Debug)]
pub struct BlockedConflict {
    pub mutation_id: Uuid,
    pub operation: subroutine_core::MutationOperation,
    pub error: ApiErrorBody,
}

pub(super) fn load_projection(connection: &Connection) -> Result<Projection> {
    let state = load_sync_state(connection)?;
    let mut action_templates = load_rows(connection, "action_template")?;
    action_templates.sort_by_key(|template: &subroutine_core::ActionTemplate| template.sort_order);
    let mut event_templates = load_rows(connection, "event_template")?;
    event_templates.sort_by_key(|template: &subroutine_core::EventTemplate| template.sort_order);
    let mut data = AllData {
        dataset_id: state.dataset_id.unwrap_or_else(Uuid::nil),
        seq: state.canonical_seq,
        actions: load_rows(connection, "action")?,
        events: load_rows(connection, "event")?,
        routines: load_rows(connection, "routine")?,
        markers: load_rows(connection, "marker")?,
        signals: load_rows(connection, "signal")?,
        action_templates,
        event_templates,
        marker_templates: load_rows(connection, "marker_template")?,
        signal_templates: load_rows(connection, "signal_template")?,
    };

    let mut statement =
        connection.prepare("SELECT request_json, patch_json FROM outbox ORDER BY position")?;
    let pending = statement
        .query_map([], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for (request, patch) in pending {
        let request: MutationRequest = serde_json::from_slice(&request)?;
        let patch: OptimisticPatch = serde_json::from_slice(&patch)?;
        apply_remote_patch(&mut data, patch, &request.operation)?;
    }

    let pending_count = outbox_count(connection)?;
    let blocked_count = connection.query_row(
        "SELECT COUNT(*) FROM outbox WHERE state = 'blocked'",
        [],
        |row| row.get::<_, usize>(0),
    )?;
    let blocked_conflict = load_oldest_outbox(connection)?
        .filter(|entry| entry.status == OutboxStatus::Blocked)
        .map(|entry| {
            let error = entry.blocked_error.ok_or_else(|| {
                LocalStoreError::InvalidMutation(
                    "blocked outbox entry is missing its conflict details".into(),
                )
            })?;
            Ok::<_, LocalStoreError>(BlockedConflict {
                mutation_id: entry.mutation.request.mutation_id,
                operation: entry.mutation.request.operation,
                error,
            })
        })
        .transpose()?;
    Ok(Projection {
        data,
        authoritative_snapshot: state.authoritative_snapshot,
        pending_count,
        blocked_count,
        blocked_conflict,
    })
}
