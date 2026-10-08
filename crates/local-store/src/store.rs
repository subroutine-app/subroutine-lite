mod worker;

use chrono::{DateTime, Utc};
use std::{
    fs,
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};
use subroutine_core::{
    AllData, ApiErrorBody, ClientMutation, DataDelta, MutationReceipt, OptimisticPatch,
};
use uuid::Uuid;

use crate::{
    LocalStoreError, OutboxEntry, Projection, Result, StaleResolution, StoreDiagnostics, SyncState,
    WorkspaceIdentity, WorkspaceSpec, db::Database,
};
use worker::{Command, Reply};

pub struct LocalStore {
    identity: WorkspaceIdentity,
    active: AtomicBool,
    gate: Mutex<()>,
    tx: mpsc::Sender<Command>,
    worker: Option<thread::JoinHandle<()>>,
}

impl LocalStore {
    pub fn open(path: impl AsRef<Path>, workspace: WorkspaceSpec) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }

        let (tx, rx) = mpsc::channel::<Command>();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("local-store".into())
            .spawn(move || match Database::open(&path, workspace) {
                Ok(mut database) => {
                    let identity = database.identity().clone();
                    if ready_tx.send(Ok(identity)).is_err() {
                        return;
                    }
                    while let Ok(command) = rx.recv() {
                        if !command.handle(&mut database) {
                            break;
                        }
                    }
                }
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                }
            })?;

        match ready_rx
            .recv()
            .map_err(|_| LocalStoreError::ActorUnavailable)?
        {
            Ok(identity) => Ok(Self {
                identity,
                active: AtomicBool::new(true),
                gate: Mutex::new(()),
                tx,
                worker: Some(worker),
            }),
            Err(error) => {
                let _ = worker.join();
                Err(error)
            }
        }
    }

    pub fn identity(&self) -> &WorkspaceIdentity {
        &self.identity
    }

    pub fn deactivate(&self) {
        let _gate = self.gate.lock().unwrap_or_else(|error| error.into_inner());
        self.active.store(false, Ordering::Release);
    }

    pub fn state(&self) -> Result<SyncState> {
        self.call(Command::State)
    }

    pub fn load_projection(&self) -> Result<Projection> {
        self.call(Command::Projection)
    }

    pub fn install_snapshot(&self, data: AllData, expected: SyncState) -> Result<Projection> {
        self.call(|reply| Command::InstallSnapshot(Box::new(data), expected, reply))
    }

    pub fn apply_delta(&self, delta: DataDelta) -> Result<Projection> {
        self.call(|reply| Command::ApplyDelta(Box::new(delta), reply))
    }

    pub fn set_event_busy_override(
        &self,
        event_id: Uuid,
        busy_override: Option<bool>,
    ) -> Result<Projection> {
        self.call(|reply| Command::SetEventBusyOverride(event_id, busy_override, reply))
    }

    pub fn apply_local_patch(&self, patch: OptimisticPatch) -> Result<Projection> {
        self.call(|reply| Command::ApplyLocalPatch(Box::new(patch), reply))
    }

    pub fn enqueue(&self, mutation: ClientMutation) -> Result<Projection> {
        self.call(|reply| Command::Enqueue(Box::new(mutation), reply))
    }

    pub fn oldest_outbox(&self) -> Result<Option<OutboxEntry>> {
        self.call(Command::OldestOutbox)
    }

    pub fn next_sendable(&self, now: DateTime<Utc>) -> Result<Option<OutboxEntry>> {
        self.call(|reply| Command::NextSendable(now, reply))
    }

    pub fn rebase_unsealed(&self, mutation_id: Uuid, new_base_seq: i64) -> Result<()> {
        self.call(|reply| Command::RebaseUnsealed(mutation_id, new_base_seq, reply))
    }

    pub fn resolve_stale_delta(
        &self,
        mutation_id: Uuid,
        replacement_mutation_id: Uuid,
        since: i64,
        delta: DataDelta,
    ) -> Result<StaleResolution> {
        self.call(|reply| Command::ResolveStaleDelta {
            mutation_id,
            replacement_mutation_id,
            since,
            delta: Box::new(delta),
            reply,
        })
    }

    pub fn resolve_stale_snapshot(
        &self,
        mutation_id: Uuid,
        data: AllData,
        expected: SyncState,
    ) -> Result<StaleResolution> {
        self.call(|reply| Command::ResolveStaleSnapshot {
            mutation_id,
            data: Box::new(data),
            expected,
            reply,
        })
    }

    pub fn record_retry(
        &self,
        mutation_id: Uuid,
        next_attempt_at: DateTime<Utc>,
        error: impl Into<String>,
    ) -> Result<()> {
        self.call(|reply| Command::RecordRetry(mutation_id, next_attempt_at, error.into(), reply))
    }

    pub fn record_receipt(&self, receipt: MutationReceipt) -> Result<()> {
        self.call(|reply| Command::RecordReceipt(Box::new(receipt), reply))
    }

    pub fn block(&self, mutation_id: Uuid, error: ApiErrorBody) -> Result<Projection> {
        self.call(|reply| Command::Block(mutation_id, Box::new(error), reply))
    }

    pub fn retry_blocked(
        &self,
        mutation_id: Uuid,
        replacement_mutation_id: Uuid,
    ) -> Result<Projection> {
        self.call(|reply| Command::RetryBlocked(mutation_id, replacement_mutation_id, reply))
    }

    pub fn retry_compatibility_rejection(&self, mutation_id: Uuid) -> Result<Projection> {
        self.call(|reply| Command::RetryCompatibilityRejection(mutation_id, reply))
    }

    pub fn discard_blocked(&self, mutation_id: Uuid) -> Result<Projection> {
        self.call(|reply| Command::DiscardBlocked(mutation_id, reply))
    }

    pub fn purge_remote(&self) -> Result<()> {
        self.call_unchecked(Command::PurgeRemote)
    }

    pub fn diagnostics(&self) -> Result<StoreDiagnostics> {
        self.call(Command::Diagnostics)
    }

    fn call<T>(&self, make_command: impl FnOnce(Reply<T>) -> Command) -> Result<T> {
        let _gate = self
            .gate
            .lock()
            .map_err(|_| LocalStoreError::ActorUnavailable)?;
        if !self.active.load(Ordering::Acquire) {
            return Err(LocalStoreError::Inactive);
        }
        self.send_command(make_command)
    }

    fn call_unchecked<T>(&self, make_command: impl FnOnce(Reply<T>) -> Command) -> Result<T> {
        let _gate = self
            .gate
            .lock()
            .map_err(|_| LocalStoreError::ActorUnavailable)?;
        self.send_command(make_command)
    }

    fn send_command<T>(&self, make_command: impl FnOnce(Reply<T>) -> Command) -> Result<T> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.tx
            .send(make_command(reply_tx))
            .map_err(|_| LocalStoreError::ActorUnavailable)?;
        reply_rx
            .recv()
            .map_err(|_| LocalStoreError::ActorUnavailable)?
    }
}

impl Drop for LocalStore {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
