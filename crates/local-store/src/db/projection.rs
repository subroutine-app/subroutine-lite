pub(crate) mod patch;

use rusqlite::Transaction;
use subroutine_core::{AllData, ApiErrorBody, ApiErrorCode, MutationOperation, ResourceKey};
use uuid::Uuid;

use super::{
    outbox::{block_outbox, load_oldest_outbox, load_outbox_entries, outbox_count},
    rows::load_rows,
    sync::load_sync_state,
};
use crate::{LocalStoreError, OutboxStatus, Result};
use patch::{apply_remote_patch, validate_routine_order};

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

pub(super) fn commit_projection(tx: Transaction<'_>) -> Result<Projection> {
    let projection = load_projection(&tx)?;
    tx.commit()?;
    Ok(projection)
}

pub(super) fn load_projection(connection: &Transaction<'_>) -> Result<Projection> {
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

    for entry in load_outbox_entries(connection)? {
        let request = entry.mutation.request;
        if let MutationOperation::ReorderRoutines { routine_ids } = &request.operation
            && validate_routine_order(data.routines.iter().map(|routine| routine.id), routine_ids)
                .is_err()
        {
            if entry.status == OutboxStatus::Pending && !entry.sealed {
                let error = ApiErrorBody {
                    error: ApiErrorCode::DomainConflict,
                    message: "routine membership changed; review the saved order before syncing"
                        .into(),
                    mutation_id: Some(request.mutation_id),
                    resource: routine_ids
                        .first()
                        .copied()
                        .or_else(|| data.routines.first().map(|routine| routine.id))
                        .map(|id| ResourceKey::Routine { id }),
                    current_seq: Some(state.canonical_seq),
                    current_dataset_id: state.dataset_id,
                    retryable: false,
                };
                block_outbox(connection, request.mutation_id, &error)?;
            }
            continue;
        }
        apply_remote_patch(
            &mut data,
            entry.mutation.optimistic_patch,
            &request.operation,
        )?;
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
