use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::{DateTime, Utc};
use local_store::{
    LocalStore, OutboxEntry, Projection, StaleResolution, SyncState, WorkspaceIdentity,
    WorkspaceLocator, WorkspaceSpec, normalize_server_origin,
};
use serde::{Deserialize, Serialize};
use subroutine_core::{
    AccountInfo, AllData, ApiErrorBody, ClientMutation, DataDelta, MutationReceipt,
};
use uuid::Uuid;

const ACTIVE_WORKSPACE_FILE: &str = "active-workspace.json";
const LOCAL_STORE_DIRECTORY: &str = "local-store";

#[derive(Clone)]
pub(super) struct WorkspacePersistence {
    root: PathBuf,
    server_origin: Option<String>,
    store: Arc<LocalStore>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActiveRemoteWorkspace {
    server_origin: String,
    account_id: Uuid,
}

impl WorkspacePersistence {
    pub(super) fn open_initial(root: PathBuf, server_origin: Option<&str>) -> Result<Self, String> {
        let server_origin = server_origin
            .map(normalize_server_origin)
            .transpose()
            .map_err(|error| error.to_string())?;
        Self::open(root, server_origin, WorkspaceSpec::Local)
    }

    fn open(
        root: PathBuf,
        server_origin: Option<String>,
        spec: WorkspaceSpec,
    ) -> Result<Self, String> {
        let path = WorkspaceLocator::new(root.join(LOCAL_STORE_DIRECTORY))
            .path_for(&spec)
            .map_err(|error| error.to_string())?;
        let store = LocalStore::open(path, spec).map_err(|error| error.to_string())?;
        Ok(Self {
            root,
            server_origin,
            store: Arc::new(store),
        })
    }

    pub(super) fn open_account(
        root: PathBuf,
        server_origin: &str,
        account_id: Uuid,
    ) -> Result<Self, String> {
        let server_origin =
            normalize_server_origin(server_origin).map_err(|error| error.to_string())?;
        Self::open(
            root,
            Some(server_origin.clone()),
            WorkspaceSpec::Remote {
                server_origin,
                account_id,
            },
        )
    }

    pub(super) fn account_id(&self) -> Option<Uuid> {
        match self.store.identity() {
            WorkspaceIdentity::Remote { account_id, .. } => Some(*account_id),
            _ => None,
        }
    }

    pub(super) fn is_remote(&self) -> bool {
        matches!(self.store.identity(), WorkspaceIdentity::Remote { .. })
    }

    pub(super) fn state(&self) -> Result<SyncState, String> {
        self.store.state().map_err(|error| error.to_string())
    }

    pub(super) fn client_id(&self) -> Uuid {
        self.store.identity().client_id()
    }

    pub(super) fn projection(&self) -> Result<Projection, String> {
        self.store
            .load_projection()
            .map_err(|error| error.to_string())
    }

    pub(super) fn install_snapshot(
        &self,
        data: AllData,
        expected: SyncState,
    ) -> Result<Projection, String> {
        self.store
            .install_snapshot(data, expected)
            .map_err(|error| error.to_string())
    }

    pub(super) fn apply_delta(&self, delta: DataDelta) -> Result<Projection, String> {
        self.store
            .apply_delta(delta)
            .map_err(|error| error.to_string())
    }

    pub(super) fn apply_local_patch(
        &self,
        patch: subroutine_core::OptimisticPatch,
    ) -> Result<Projection, String> {
        self.store
            .apply_local_patch(patch)
            .map_err(|error| error.to_string())
    }

    pub(super) fn set_event_busy_override(
        &self,
        event_id: Uuid,
        busy_override: Option<bool>,
    ) -> Result<Projection, String> {
        self.store
            .set_event_busy_override(event_id, busy_override)
            .map_err(|error| error.to_string())
    }
    pub(super) fn enqueue(&self, mutation: ClientMutation) -> Result<Projection, String> {
        self.store
            .enqueue(mutation)
            .map_err(|error| error.to_string())
    }

    pub(super) fn oldest_outbox(&self) -> Result<Option<OutboxEntry>, String> {
        self.store
            .oldest_outbox()
            .map_err(|error| error.to_string())
    }

    pub(super) fn next_sendable(&self, now: DateTime<Utc>) -> Result<Option<OutboxEntry>, String> {
        self.store
            .next_sendable(now)
            .map_err(|error| error.to_string())
    }

    pub(super) fn resolve_stale_delta(
        &self,
        mutation_id: Uuid,
        replacement_mutation_id: Uuid,
        since: i64,
        delta: DataDelta,
    ) -> Result<StaleResolution, String> {
        self.store
            .resolve_stale_delta(mutation_id, replacement_mutation_id, since, delta)
            .map_err(|error| error.to_string())
    }

    pub(super) fn resolve_stale_snapshot(
        &self,
        mutation_id: Uuid,
        data: AllData,
        expected: SyncState,
    ) -> Result<StaleResolution, String> {
        self.store
            .resolve_stale_snapshot(mutation_id, data, expected)
            .map_err(|error| error.to_string())
    }

    pub(super) fn record_retry(
        &self,
        mutation_id: Uuid,
        next_attempt_at: DateTime<Utc>,
        error: String,
    ) -> Result<(), String> {
        self.store
            .record_retry(mutation_id, next_attempt_at, error)
            .map_err(|error| error.to_string())
    }

    pub(super) fn record_receipt(&self, receipt: MutationReceipt) -> Result<(), String> {
        self.store
            .record_receipt(receipt)
            .map_err(|error| error.to_string())
    }

    pub(super) fn block(
        &self,
        mutation_id: Uuid,
        error: ApiErrorBody,
    ) -> Result<Projection, String> {
        self.store
            .block(mutation_id, error)
            .map_err(|error| error.to_string())
    }

    pub(super) fn retry_compatibility_rejection(
        &self,
        mutation_id: Uuid,
    ) -> Result<Projection, String> {
        self.store
            .retry_compatibility_rejection(mutation_id)
            .map_err(|error| error.to_string())
    }

    pub(super) fn retry_blocked(
        &self,
        mutation_id: Uuid,
        replacement_mutation_id: Uuid,
    ) -> Result<Projection, String> {
        self.store
            .retry_blocked(mutation_id, replacement_mutation_id)
            .map_err(|error| error.to_string())
    }

    pub(super) fn discard_blocked(&self, mutation_id: Uuid) -> Result<Projection, String> {
        self.store
            .discard_blocked(mutation_id)
            .map_err(|error| error.to_string())
    }

    pub(super) fn purge_remote_and_switch_to_local(&self) -> Result<(Self, Projection), String> {
        if !self.is_remote() {
            return Err("the active workspace does not contain offline account data".into());
        }
        let local = Self::open(
            self.root.clone(),
            self.server_origin.clone(),
            WorkspaceSpec::Local,
        )?;
        let projection = local.projection()?;
        self.store.deactivate();
        self.store
            .purge_remote()
            .map_err(|error| error.to_string())?;
        if let Err(error) = remove_active_remote(&self.root) {
            tracing::warn!(%error, "offline account data was removed but the active-workspace marker could not be cleared");
        }
        Ok((local, projection))
    }

    pub(super) fn activate_remote(
        &self,
        account: AccountInfo,
        data: AllData,
    ) -> Result<(Self, Projection), String> {
        let server_origin = self
            .server_origin
            .clone()
            .ok_or_else(|| "no server origin is configured".to_owned())?;
        if data.dataset_id != account.dataset_id {
            return Err("account and snapshot dataset identities do not match".into());
        }

        let next = if matches!(
            self.store.identity(),
            WorkspaceIdentity::Remote {
                server_origin: current_origin,
                account_id: current_account,
                ..
            } if current_origin == &server_origin && current_account == &account.account_id
        ) {
            self.clone()
        } else {
            Self::open(
                self.root.clone(),
                Some(server_origin.clone()),
                WorkspaceSpec::Remote {
                    server_origin,
                    account_id: account.account_id,
                },
            )?
        };
        let expected = next.state()?;
        let projection = next.install_snapshot(data, expected)?;
        Ok((next, projection))
    }

    pub(super) fn mark_active(&self) -> Result<(), String> {
        let WorkspaceIdentity::Remote {
            server_origin,
            account_id,
            ..
        } = self.store.identity()
        else {
            return Ok(());
        };
        save_active_remote(
            &self.root,
            &ActiveRemoteWorkspace {
                server_origin: server_origin.clone(),
                account_id: *account_id,
            },
        )
    }

    pub(super) fn deactivate(&self) {
        self.store.deactivate();
    }
}

pub(super) fn workspace_root_from_env() -> Result<PathBuf, String> {
    crate::paths::get().map(|paths| paths.data_dir.clone())
}

fn active_workspace_path(root: &Path) -> PathBuf {
    root.join(ACTIVE_WORKSPACE_FILE)
}

fn save_active_remote(root: &Path, active: &ActiveRemoteWorkspace) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|error| format!("create {}: {error}", root.display()))?;
    let path = active_workspace_path(root);
    let temporary = root.join(format!("{ACTIVE_WORKSPACE_FILE}.tmp"));
    let encoded = serde_json::to_vec(active).map_err(|error| error.to_string())?;
    fs::write(&temporary, encoded)
        .map_err(|error| format!("write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, &path).map_err(|error| format!("replace {}: {error}", path.display()))
}

fn remove_active_remote(root: &Path) -> Result<(), String> {
    let path = active_workspace_path(root);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("remove {}: {error}", path.display())),
    }
}
