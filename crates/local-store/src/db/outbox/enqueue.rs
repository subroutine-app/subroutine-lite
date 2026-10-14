use chrono::Utc;
use rusqlite::{Transaction, params};
use subroutine_core::{ClientMutation, MUTATION_PROTOCOL_VERSION};

use super::{conflict::patches_overlap, load_outbox_entries};
use crate::{LocalStoreError, Result, WorkspaceIdentity, db::sync::load_sync_state};

pub(in crate::db) fn enqueue_mutation(
    tx: &Transaction<'_>,
    identity: &WorkspaceIdentity,
    mut mutation: ClientMutation,
) -> Result<()> {
    let state = load_sync_state(tx)?;
    if !matches!(identity, WorkspaceIdentity::Remote { .. }) {
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
    if mutation.request.client_id != identity.client_id() {
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

    for entry in load_outbox_entries(tx)? {
        if patches_overlap(&mutation.optimistic_patch, &entry.mutation.optimistic_patch) {
            mutation.request.base_seq = mutation
                .request
                .base_seq
                .min(entry.mutation.request.base_seq);
        }
    }
    tx.execute(
        "INSERT INTO outbox (mutation_id, request_json, patch_json, created_at_ms)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            mutation.request.mutation_id.to_string(),
            serde_json::to_vec(&mutation.request)?,
            serde_json::to_vec(&mutation.optimistic_patch)?,
            Utc::now().timestamp_millis()
        ],
    )?;
    Ok(())
}
