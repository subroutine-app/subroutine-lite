use std::{collections::HashSet, path::Path};

use rusqlite::{Transaction, TransactionBehavior, params};
use subroutine_core::{
    ClientMutation, MUTATION_PROTOCOL_VERSION, MutationOperation, MutationRequest, OptimisticPatch,
    ResourceValue,
};
use uuid::Uuid;

use super::{
    Database, import_resources::import_resources, outbox::enqueue_mutation,
    projection::load_projection, rows::load_rows, sync::load_sync_state,
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

fn known_provider_items(tx: &Transaction<'_>) -> Result<HashSet<(String, String)>> {
    let mut data = load_projection(tx)?.data;

    data.actions.extend(load_rows(tx, "action")?);
    data.events.extend(load_rows(tx, "event")?);
    data.markers.extend(load_rows(tx, "marker")?);
    let mut known = data
        .actions
        .into_iter()
        .map(|value| (value.source_provider, value.source_external_id))
        .chain(
            data.events
                .into_iter()
                .map(|value| (value.source_provider, value.source_external_id)),
        )
        .chain(
            data.markers
                .into_iter()
                .map(|value| (value.source_provider, value.source_external_id)),
        )
        .filter_map(|(provider, external_id)| provider.zip(external_id))
        .collect::<HashSet<_>>();
    let mut statement = tx.prepare("SELECT provider_id, external_id FROM integration_entries")?;
    let entries = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    for entry in entries {
        known.insert(entry?);
    }
    Ok(known)
}

fn provider_identity(resource: &ResourceValue) -> Option<(String, String)> {
    let (provider, external_id) = match resource {
        ResourceValue::Action(value) => (&value.source_provider, &value.source_external_id),
        ResourceValue::Event(value) => (&value.source_provider, &value.source_external_id),
        ResourceValue::Marker(value) => (&value.source_provider, &value.source_external_id),
        ResourceValue::Routine(_)
        | ResourceValue::Signal(_)
        | ResourceValue::ActionTemplate(_)
        | ResourceValue::EventTemplate(_)
        | ResourceValue::MarkerTemplate(_)
        | ResourceValue::SignalTemplate(_) => return None,
    };
    provider.clone().zip(external_id.clone())
}

fn transfer_batch(
    resources: Vec<ResourceValue>,
    known: &HashSet<(String, String)>,
    incoming: &mut HashSet<(String, String)>,
) -> Result<Vec<ResourceValue>> {
    let mut writes = Vec::new();
    for resource in resources {
        if let Some(identity) = provider_identity(&resource) {
            if known.contains(&identity) {
                continue;
            }
            if !incoming.insert(identity) {
                return Err(LocalStoreError::InvalidMutation(
                    "multiple signed-out items share a provider identity; workspace transfer left pending for resolution".into(),
                ));
            }
        }
        writes.push(resource);
    }
    Ok(writes)
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
    if imports.is_empty() {
        return Ok(());
    }
    let mut known = known_provider_items(tx)?;
    for (export_id, encoded) in imports {
        let batches: ImportBatches = serde_json::from_slice(&encoded)?;
        let mut incoming = HashSet::new();
        for resources in batches {
            let resources = transfer_batch(resources, &known, &mut incoming)?;
            if resources.is_empty() {
                continue;
            }
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
        known.extend(incoming);
        tx.execute(
            "UPDATE local_imports SET batches_json = NULL WHERE export_id = ?1",
            [&export_id],
        )?;
    }
    Ok(())
}
