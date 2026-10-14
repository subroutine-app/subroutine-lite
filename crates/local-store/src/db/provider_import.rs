mod batch;
mod identity;

mod resource;
mod writes;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};

use rusqlite::TransactionBehavior;
use subroutine_import::provider::{Imported, SyncOutcome};
use uuid::Uuid;

use self::{
    identity::{Entry, Presence},
    resource::{Kind, Resource, Resources},
};
use super::{
    Database,
    projection::{commit_projection, load_projection},
    sync::load_sync_state,
};
use crate::{LocalStoreError, Projection, Result, WorkspaceIdentity};

#[derive(Debug)]
pub struct ProviderImportResult {
    pub projection: Projection,

    pub changed: usize,
}

impl Database {
    pub(crate) fn apply_provider_import(
        &mut self,
        outcome: SyncOutcome,
    ) -> Result<ProviderImportResult> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state = load_sync_state(&tx)?;
        if matches!(self.identity, WorkspaceIdentity::Remote { .. })
            && (!state.authoritative_snapshot || state.dataset_id.is_none_or(|id| id.is_nil()))
        {
            return Err(LocalStoreError::NoAuthoritativeSnapshot);
        }
        if matches!(self.identity, WorkspaceIdentity::Remote { .. }) {
            let transfer_pending: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM local_imports WHERE batches_json IS NOT NULL)",
                [],
                |row| row.get(0),
            )?;
            if transfer_pending {
                return Err(LocalStoreError::InvalidMutation(
                    "refresh account data to finish the local workspace transfer before importing provider items".into(),
                ));
            }
        }
        let provider = outcome.provider_id;
        let resources = Resources::new(load_projection(&tx)?.data, provider);
        let entries = Entry::load(&tx, provider)?;
        let reconciliation = Reconciliation::build(outcome, entries, &resources)?;
        let changed = reconciliation.changed;
        reconciliation.commit(&tx, &self.identity, &state, provider)?;
        Ok(ProviderImportResult {
            projection: commit_projection(tx)?,
            changed,
        })
    }
}

#[derive(Default)]
struct Reconciliation {
    entries: Vec<Entry>,
    writes: Vec<Resource>,
    deletes: Vec<(Kind, Uuid)>,

    changed: usize,
}

impl Reconciliation {
    fn build(
        outcome: SyncOutcome,
        mut entries: HashMap<String, Entry>,
        resources: &Resources,
    ) -> Result<Self> {
        let excluded: HashSet<String> =
            outcome.excluded_external_ids().map(str::to_owned).collect();
        let items = batch::normalize(outcome.items, &excluded, outcome.provider_id)?;
        let mut result = Self::default();
        for imported in items {
            let entry = entries.remove(&imported.external_id);
            result.import(imported, entry, resources, outcome.provider_id)?;
        }

        let mut excluded = excluded.into_iter().collect::<Vec<_>>();
        excluded.sort();
        for external_id in excluded {
            result.exclude(&external_id, entries.remove(&external_id), resources)?;
        }
        Ok(result)
    }

    fn import(
        &mut self,
        imported: Imported<Resource>,
        entry: Option<Entry>,
        resources: &Resources,
        provider: &str,
    ) -> Result<()> {
        let Imported {
            external_id,
            external_version,
            mut item,
        } = imported;
        let adopted = entry.is_none();
        let mut entry = match entry {
            Some(entry) => entry,
            None => match resources.adopt(&external_id)? {
                Some(previous) => Entry {
                    external_id,
                    version: None,
                    id: previous.id(),
                    kind: previous.kind(),
                    presence: Presence::Present,
                },
                None => {
                    let id = Uuid::now_v7();
                    item.reconcile(id, provider, &external_id, None, false);
                    self.entries.push(Entry {
                        external_id,
                        version: external_version,
                        id,
                        kind: item.kind(),
                        presence: Presence::Present,
                    });
                    self.writes.push(item);
                    self.changed += 1;
                    return Ok(());
                }
            },
        };
        if entry.presence == Presence::Ignored {
            return Ok(());
        }
        let previous = resources.get(entry.kind, entry.id);
        if entry.presence == Presence::Present && previous.is_none() {
            entry.presence = Presence::Ignored;
            self.entries.push(entry);
            return Ok(());
        }
        let same_version = entry.version == external_version;
        if !adopted
            && entry.presence == Presence::Present
            && entry.kind == item.kind()
            && same_version
            && external_version.is_some()
        {
            if let Some(backfilled) = item.backfill_availability(previous) {
                self.writes.push(backfilled);
                self.changed += 1;
            }
            return Ok(());
        }
        item.reconcile(
            entry.id,
            provider,
            &entry.external_id,
            previous,
            adopted || same_version,
        );
        let unchanged = previous
            .map(|previous| {
                Ok::<_, LocalStoreError>(
                    serde_json::to_vec(previous)? == serde_json::to_vec(&item)?,
                )
            })
            .transpose()?
            .unwrap_or(false);
        if !unchanged {
            if previous.is_some() && entry.kind != item.kind() {
                self.deletes.push((entry.kind, entry.id));
            }
            entry.kind = item.kind();
            self.writes.push(item);
            self.changed += 1;
        }
        entry.version = external_version;
        entry.presence = Presence::Present;
        self.entries.push(entry);
        Ok(())
    }

    fn exclude(
        &mut self,
        external_id: &str,
        entry: Option<Entry>,
        resources: &Resources,
    ) -> Result<()> {
        let mut entry = match entry {
            Some(entry) => entry,
            None => match resources.adopt(external_id)? {
                Some(previous) => Entry {
                    external_id: external_id.into(),
                    version: None,
                    id: previous.id(),
                    kind: previous.kind(),
                    presence: Presence::Present,
                },
                None => return Ok(()),
            },
        };
        if entry.presence == Presence::Ignored {
            return Ok(());
        }
        if resources.get(entry.kind, entry.id).is_some() {
            self.deletes.push((entry.kind, entry.id));
            self.changed += 1;
            entry.presence = Presence::Excluded;
        } else if entry.presence == Presence::Present {
            entry.presence = Presence::Ignored;
        }
        self.entries.push(entry);
        Ok(())
    }
}
