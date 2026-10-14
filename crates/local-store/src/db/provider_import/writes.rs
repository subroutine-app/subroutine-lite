use rusqlite::Transaction;
use serde::Serialize;
use subroutine_core::{
    ClientMutation, MUTATION_PROTOCOL_VERSION, MutationOperation, MutationRequest, OptimisticPatch,
};
use uuid::Uuid;

use super::{Reconciliation, resource::Resource};
use crate::{
    LocalStoreError, Result, SyncState, WorkspaceIdentity,
    db::{outbox::enqueue_mutation, rows::delete_rows},
};

const BATCH_RESOURCES: usize = 200;
// Keep the request envelope comfortably below the server's 2 MiB body limit.
const BATCH_BYTES: usize = 512 * 1024;

impl Reconciliation {
    pub(super) fn commit(
        self,
        tx: &Transaction<'_>,
        identity: &WorkspaceIdentity,
        state: &SyncState,
        provider: &str,
    ) -> Result<()> {
        self.writes.iter().try_for_each(Resource::validate_timing)?;
        match identity {
            WorkspaceIdentity::Local { .. } => {
                for (kind, id) in self.deletes {
                    delete_rows(tx, kind.name(), &[id])?;
                }
                for resource in self.writes {
                    resource.upsert(tx)?;
                }
            }
            WorkspaceIdentity::Remote { .. } => {
                let dataset_id = state
                    .dataset_id
                    .ok_or(LocalStoreError::NoAuthoritativeSnapshot)?;
                let enqueue = |operation, optimistic_patch| {
                    enqueue_mutation(
                        tx,
                        identity,
                        ClientMutation {
                            request: MutationRequest {
                                protocol_version: MUTATION_PROTOCOL_VERSION,
                                dataset_id,
                                mutation_id: Uuid::now_v7(),
                                client_id: identity.client_id(),
                                base_seq: state.canonical_seq,
                                operation,
                            },
                            optimistic_patch,
                        },
                    )
                };
                let deletes = self
                    .deletes
                    .into_iter()
                    .map(|(kind, id)| kind.key(id))
                    .collect();
                for resources in batches(deletes)? {
                    let patch = OptimisticPatch {
                        deletes: resources.clone(),
                        ..Default::default()
                    };
                    enqueue(MutationOperation::DeleteResources { resources }, patch)?;
                }
                let writes = self.writes.into_iter().map(Resource::into_value).collect();
                for resources in batches(writes)? {
                    let patch = OptimisticPatch {
                        writes: resources.clone(),
                        ..Default::default()
                    };
                    enqueue(MutationOperation::UpsertResources { resources }, patch)?;
                }
            }
        }
        for entry in self.entries {
            entry.save(tx, provider)?;
        }

        Ok(())
    }
}

fn batches<T: Serialize>(resources: Vec<T>) -> Result<Vec<Vec<T>>> {
    let mut batches = Vec::new();
    let mut batch = Vec::new();
    let mut bytes = 2;
    for resource in resources {
        let size = serde_json::to_vec(&resource)?.len() + 1;
        if size + 2 > BATCH_BYTES {
            return Err(LocalStoreError::InvalidMutation(
                "provider item is too large to sync; import left unchanged".into(),
            ));
        }
        if batch.len() == BATCH_RESOURCES || bytes + size > BATCH_BYTES {
            batches.push(std::mem::take(&mut batch));
            bytes = 2;
        }
        bytes += size;
        batch.push(resource);
    }
    if !batch.is_empty() {
        batches.push(batch);
    }
    Ok(batches)
}
