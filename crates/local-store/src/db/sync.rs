use rusqlite::{Connection, TransactionBehavior, params};
use subroutine_core::{AllData, DataDelta};
use uuid::Uuid;

use super::{
    Database, non_nil_uuid,
    outbox::{outbox_count, retire_confirmed},
    parse_uuid,
    projection::commit_projection,
    rows::{apply_delta_rows, insert_snapshot_rows},
};
use crate::{LocalStoreError, Projection, Result, WorkspaceIdentity};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncState {
    pub dataset_id: Option<Uuid>,
    pub canonical_seq: i64,
    pub authoritative_snapshot: bool,
}

impl Database {
    pub(crate) fn state(&self) -> Result<SyncState> {
        load_sync_state(&self.connection)
    }

    pub(crate) fn install_snapshot(
        &mut self,
        data: AllData,
        expected: SyncState,
    ) -> Result<Projection> {
        let incoming_dataset = non_nil_uuid(data.dataset_id);
        if matches!(self.identity, WorkspaceIdentity::Remote { .. }) && incoming_dataset.is_none() {
            return Err(LocalStoreError::InvalidMutation(
                "remote snapshots must carry a dataset identity".into(),
            ));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_sync_state(&tx)?;
        if current != expected {
            return Err(LocalStoreError::StaleCanonicalState);
        }
        let pending = outbox_count(&tx)?;

        if current.dataset_id != incoming_dataset && pending > 0 {
            return Err(LocalStoreError::DatasetChangedWithPendingIntent {
                current: current.dataset_id,
                incoming: incoming_dataset,
                pending,
            });
        }
        if data.seq < 0 {
            return Err(LocalStoreError::InvalidMutation(
                "snapshot sequence cannot be negative".into(),
            ));
        }
        if current.dataset_id == incoming_dataset && data.seq < current.canonical_seq {
            return Err(LocalStoreError::SequenceRegression {
                current: current.canonical_seq,
                incoming: data.seq,
            });
        }

        tx.execute("DELETE FROM canonical_resources", [])?;
        insert_snapshot_rows(&tx, &data)?;
        tx.execute(
            "UPDATE sync_state SET dataset_id = ?1, canonical_seq = ?2, authoritative_snapshot = 1 WHERE singleton = 1",
            params![incoming_dataset.map(|id| id.to_string()), data.seq],
        )?;
        if current.dataset_id != incoming_dataset {
            tx.execute("DELETE FROM integration_entries", [])?;
        }
        retire_confirmed(&tx, incoming_dataset, data.seq)?;
        commit_projection(tx)
    }

    pub(crate) fn apply_delta(&mut self, delta: DataDelta) -> Result<Projection> {
        let incoming_dataset = non_nil_uuid(delta.dataset_id).ok_or_else(|| {
            LocalStoreError::InvalidMutation("delta is missing its dataset identity".into())
        })?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_sync_state(&tx)?;
        if !current.authoritative_snapshot {
            return Err(LocalStoreError::NoAuthoritativeSnapshot);
        }
        let current_dataset = current.dataset_id.ok_or_else(|| {
            LocalStoreError::InvalidMutation(
                "authoritative canonical state has no dataset identity".into(),
            )
        })?;
        if current_dataset != incoming_dataset {
            return Err(LocalStoreError::DatasetMismatch {
                current: current_dataset,
                incoming: incoming_dataset,
            });
        }
        if delta.seq < current.canonical_seq {
            return Err(LocalStoreError::SequenceRegression {
                current: current.canonical_seq,
                incoming: delta.seq,
            });
        }

        apply_delta_rows(&tx, &delta)?;
        tx.execute(
            "UPDATE sync_state SET canonical_seq = ?1 WHERE singleton = 1",
            [delta.seq],
        )?;
        retire_confirmed(&tx, Some(incoming_dataset), delta.seq)?;
        commit_projection(tx)
    }
}

pub(super) fn load_sync_state(connection: &Connection) -> Result<SyncState> {
    let (dataset_id, canonical_seq, authoritative): (Option<String>, i64, i64) = connection
        .query_row(
            "SELECT dataset_id, canonical_seq, authoritative_snapshot
             FROM sync_state WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
    Ok(SyncState {
        dataset_id: dataset_id
            .map(|value| parse_uuid(&value, "dataset_id"))
            .transpose()?,
        canonical_seq,
        authoritative_snapshot: authoritative != 0,
    })
}
