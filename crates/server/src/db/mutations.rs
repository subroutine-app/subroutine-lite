use anyhow::{Context as _, bail};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use subroutine_core::{
    MutationEffect, MutationOperation, MutationReceipt, MutationRequest, MutationResult,
};
use uuid::Uuid;

pub(crate) struct StoredReceipt {
    pub(crate) request_hash: Vec<u8>,
    pub(crate) receipt: MutationReceipt,
}

#[derive(sqlx::FromRow)]
struct ReceiptRow {
    request_hash: Vec<u8>,
    client_id: Uuid,
    protocol_version: i16,
    base_seq: i64,
    commit_seq: i64,
    effect: String,
    result: Value,
}

pub(crate) fn request_hash(request: &MutationRequest) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"subroutine.mutation.v1\0");
    hash.update(request.protocol_version.to_be_bytes());
    hash.update(request.dataset_id.as_bytes());
    hash.update(request.mutation_id.as_bytes());
    hash.update(request.client_id.as_bytes());
    hash.update(request.base_seq.to_be_bytes());
    match &request.operation {
        MutationOperation::UpsertAction { action } => {
            hash.update([3]);
            let encoded = serde_json::to_vec(action)
                .expect("an action accepted by the typed request is serializable");
            hash.update((encoded.len() as u64).to_be_bytes());
            hash.update(encoded);
        }
        MutationOperation::UpsertActions { actions } => {
            hash.update([4]);
            let encoded = serde_json::to_vec(actions)
                .expect("actions accepted by the typed request are serializable");
            hash.update((encoded.len() as u64).to_be_bytes());
            hash.update(encoded);
        }
        MutationOperation::UpsertResources { resources } => {
            hash.update([6]);
            let encoded = serde_json::to_vec(resources)
                .expect("resources accepted by the typed request are serializable");
            hash.update((encoded.len() as u64).to_be_bytes());
            hash.update(encoded);
        }
        MutationOperation::SetEventBusyOverride {
            event_id,
            busy_override,
        } => {
            hash.update([10]);
            hash.update(event_id.as_bytes());
            hash.update([match busy_override {
                None => 0,
                Some(false) => 1,
                Some(true) => 2,
            }]);
        }

        MutationOperation::DeleteResources { resources } => {
            hash.update([7]);
            let encoded = serde_json::to_vec(resources)
                .expect("resource keys accepted by the typed request are serializable");
            hash.update((encoded.len() as u64).to_be_bytes());
            hash.update(encoded);
        }
        MutationOperation::ReorderRoutines { routine_ids } => {
            hash.update([8]);
            let encoded = serde_json::to_vec(routine_ids)
                .expect("routine ids accepted by the typed request are serializable");
            hash.update((encoded.len() as u64).to_be_bytes());
            hash.update(encoded);
        }
        MutationOperation::CompleteAction {
            action_id,
            completed_at,
        } => {
            hash.update([1]);
            hash.update(action_id.as_bytes());
            hash.update(completed_at.timestamp().to_be_bytes());
            hash.update(completed_at.timestamp_subsec_nanos().to_be_bytes());
        }
        MutationOperation::DeleteAction { action_id } => {
            hash.update([2]);
            hash.update(action_id.as_bytes());
        }
        MutationOperation::ConvertEventToMarker {
            event_id,
            marker_id,
            date,
            local_end_date,
        } => {
            hash.update([5]);
            hash.update(event_id.as_bytes());
            hash.update(marker_id.as_bytes());
            hash.update(date.to_string().as_bytes());
            if let Some(local_end_date) = local_end_date {
                hash.update(local_end_date.to_string().as_bytes());
            }
        }
    }

    hash.finalize().into()
}

pub(crate) async fn fetch_receipt(
    conn: &mut PgConnection,
    user_id: Uuid,
    dataset_id: Uuid,
    mutation_id: Uuid,
) -> anyhow::Result<Option<StoredReceipt>> {
    let row: Option<ReceiptRow> = sqlx::query_as(
        "SELECT request_hash, client_id, protocol_version, base_seq, commit_seq, effect, result \
         FROM mutation_receipts \
         WHERE user_id = $1 AND dataset_id = $2 AND mutation_id = $3",
    )
    .bind(user_id)
    .bind(dataset_id)
    .bind(mutation_id)
    .fetch_optional(&mut *conn)
    .await
    .context("read mutation receipt")?;

    row.map(|row| {
        let effect = match row.effect.as_str() {
            "applied" => MutationEffect::Applied,
            "no_op" => MutationEffect::NoOp,
            other => bail!("stored mutation receipt has unknown effect {other}"),
        };
        let result = serde_json::from_value::<MutationResult>(row.result)
            .context("decode stored mutation result")?;
        Ok(StoredReceipt {
            request_hash: row.request_hash,
            receipt: MutationReceipt {
                protocol_version: u16::try_from(row.protocol_version)
                    .context("stored mutation protocol version is invalid")?,
                dataset_id,
                mutation_id,
                client_id: row.client_id,
                base_seq: row.base_seq,
                commit_seq: row.commit_seq,
                effect,
                replayed: false,
                result,
            },
        })
    })
    .transpose()
}

pub(crate) async fn insert_receipt(
    conn: &mut PgConnection,
    user_id: Uuid,
    request_hash: &[u8; 32],
    receipt: &MutationReceipt,
) -> anyhow::Result<()> {
    let effect = match receipt.effect {
        MutationEffect::Applied => "applied",
        MutationEffect::NoOp => "no_op",
    };
    let result = serde_json::to_value(&receipt.result).context("encode mutation result")?;
    sqlx::query(
        "INSERT INTO mutation_receipts \
         (user_id, dataset_id, mutation_id, client_id, protocol_version, request_hash, \
          base_seq, commit_seq, effect, result) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(user_id)
    .bind(receipt.dataset_id)
    .bind(receipt.mutation_id)
    .bind(receipt.client_id)
    .bind(
        i16::try_from(receipt.protocol_version)
            .context("mutation protocol version is too large")?,
    )
    .bind(request_hash.as_slice())
    .bind(receipt.base_seq)
    .bind(receipt.commit_seq)
    .bind(effect)
    .bind(result)
    .execute(&mut *conn)
    .await
    .context("store mutation receipt")?;
    Ok(())
}
