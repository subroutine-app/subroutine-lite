use std::path::Path;

use rusqlite::{Transaction, TransactionBehavior, params};
use subroutine_core::{
    ClientMutation, MUTATION_PROTOCOL_VERSION, MutationOperation, MutationRequest, OptimisticPatch,
    ResourceValue,
};
use uuid::Uuid;

use super::{
    Database, import_resources::import_resources, outbox::enqueue_mutation,
    projection::load_projection, sync::load_sync_state,
};
use crate::{LocalStoreError, Projection, Result, WorkspaceIdentity, WorkspaceSpec};

const BATCH_RESOURCES: usize = 200;

const BATCH_BYTES: usize = 512 * 1024;

type ImportBatches = Vec<Vec<ResourceValue>>;

impl Database {
    pub(crate) fn import_local_workspace(&mut self, path: &Path) -> Result<Projection> {
        let WorkspaceIdentity::Remote {
            server_origin,
            account_id,
            ..
        } = &self.identity
        else {
            return Err(LocalStoreError::InvalidMutation(
                "local data can only be imported into an account workspace".into(),
            ));
        };
        let mut local = Self::open(path, WorkspaceSpec::Local)?;
        local.claim_export(server_origin, *account_id)?;
        let exports = {
            let mut statement = local.connection.prepare(
                "SELECT export_id, batches_json FROM local_exports
                 WHERE server_origin = ?1 AND account_id = ?2 ORDER BY position",
            )?;
            statement
                .query_map(params![server_origin, account_id.to_string()], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        for (export_id, batches) in exports {
            self.accept_export(&export_id, &batches)?;
            local.connection.execute(
                "DELETE FROM local_exports WHERE export_id = ?1",
                [&export_id],
            )?;
        }
        self.projection()
    }

    fn claim_export(&mut self, server_origin: &str, account_id: Uuid) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let data = load_projection(&tx)?.data;
        let export_id = Uuid::now_v7();
        let batches = batches(import_resources(data, export_id))?;
        if !batches.is_empty() {
            // Claim and remove the signed-out rows together. A crash leaves this account-bound
            // export intact until the receiving database has durably acknowledged it.
            tx.execute(
                "INSERT INTO local_exports (export_id, server_origin, account_id, batches_json)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    export_id.to_string(),
                    server_origin,
                    account_id.to_string(),
                    serde_json::to_vec(&batches)?
                ],
            )?;
            tx.execute("DELETE FROM canonical_resources", [])?;
            tx.execute("DELETE FROM integration_entries", [])?;
            tx.execute(
                "UPDATE workspace SET holiday_setup = 'complete' WHERE singleton = 1",
                [],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn accept_export(&mut self, export_id: &str, batches: &[u8]) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Keep the receipt after enqueueing: the source may not have observed our commit.
        tx.execute(
            "INSERT INTO local_imports (export_id, batches_json) VALUES (?1, ?2)
             ON CONFLICT(export_id) DO NOTHING",
            params![export_id, batches],
        )?;
        tx.commit()?;
        Ok(())
    }
}

fn batches(resources: Vec<ResourceValue>) -> Result<ImportBatches> {
    let mut batches = Vec::new();
    let mut batch = Vec::new();
    let mut bytes = 2;
    for resource in resources {
        let size = serde_json::to_vec(&resource)?.len() + 1;
        if size + 2 > BATCH_BYTES {
            return Err(LocalStoreError::InvalidMutation(format!(
                "item {} is too large to import; shorten its content in the signed-out workspace and sign in again",
                resource.key().id(),
            )));
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

pub(super) fn enqueue_imports(tx: &Transaction<'_>, identity: &WorkspaceIdentity) -> Result<()> {
    let state = load_sync_state(tx)?;
    let Some(dataset_id) = state.dataset_id.filter(|_| state.authoritative_snapshot) else {
        return Ok(());
    };
    let imports = {
        let mut statement = tx.prepare(
            "SELECT export_id, batches_json FROM local_imports
             WHERE batches_json IS NOT NULL ORDER BY position",
        )?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?
    };
    for (export_id, encoded) in imports {
        let batches: ImportBatches = serde_json::from_slice(&encoded)?;
        for resources in batches {
            let patch = OptimisticPatch {
                writes: resources.clone(),
                ..Default::default()
            };
            let request = MutationRequest {
                protocol_version: MUTATION_PROTOCOL_VERSION,
                dataset_id,
                mutation_id: Uuid::now_v7(),
                client_id: identity.client_id(),
                base_seq: state.canonical_seq,
                operation: MutationOperation::UpsertResources { resources },
            };
            enqueue_mutation(
                tx,
                identity,
                ClientMutation {
                    request,
                    optimistic_patch: patch,
                },
            )?;
        }
        tx.execute(
            "UPDATE local_imports SET batches_json = NULL WHERE export_id = ?1",
            [&export_id],
        )?;
    }
    Ok(())
}
