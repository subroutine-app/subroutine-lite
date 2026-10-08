use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use gpui::{AppContext, AsyncApp, Context, WeakEntity};
use local_store::{
    OutboxEntry, OutboxStatus, Projection, StaleResolution, StaleResolutionKind, SyncState,
};
use subroutine_core::{ApiErrorBody, ApiErrorCode, MutationRequest};
use uuid::Uuid;

use super::sync::SYNC_RETRY_DELAY;
use super::transport::{
    Cmd, CommandSender, FetchError, MutationSendError, RefreshData, is_authentication_error,
};
use super::{AppDatabaseStore, SyncDirection, SyncStatus, WorkspacePersistence};

impl AppDatabaseStore {
    pub(super) fn start_outbox_replay(&mut self, cx: &mut Context<Self>) {
        if self.sync_in_flight {
            self.sync_request.request();
            return;
        }
        if self.outbox_replay_in_flight || !crate::auth::AuthSession::global(cx).is_signed_in() {
            return;
        }
        let Some(persistence) = self
            .persistence
            .clone()
            .filter(WorkspacePersistence::is_remote)
        else {
            return;
        };
        if persistence.account_id() != crate::auth::AuthSession::global(cx).account_id() {
            return;
        }
        let (wake_tx, wake_rx) = flume::bounded(1);
        let replay = Replay {
            this: cx.entity().downgrade(),
            generation: self.workspace_generation,
            persistence,
            cmd_tx: self.cmd_tx.clone(),
            wake_rx,
        };
        self.outbox_wake_tx = Some(wake_tx);
        self.outbox_retry_waiting = false;
        self.outbox_replay_in_flight = true;
        self.sync_status = SyncStatus::Syncing;
        cx.notify();

        cx.spawn(async move |_, cx| replay.run(cx).await).detach();
    }

    pub fn keep_local_conflict(&mut self, mutation_id: Uuid, cx: &mut Context<Self>) {
        let Some(persistence) = self.persistence.clone() else {
            self.report_local_store_failure(
                "cannot resolve a conflict without a local workspace".into(),
                cx,
            );
            return;
        };
        match persistence.retry_blocked(mutation_id, Uuid::now_v7()) {
            Ok(projection) => {
                self.log_sync(SyncDirection::Outgoing, "User confirmed retrying the blocked local change against the latest known server state", cx);
                self.replace_projection(projection, cx);
                self.start_outbox_replay(cx);
            }
            Err(error) => self.report_local_store_failure(error, cx),
        }
    }

    pub fn accept_server_conflict(&mut self, mutation_id: Uuid, cx: &mut Context<Self>) {
        let Some(persistence) = self.persistence.clone() else {
            self.report_local_store_failure(
                "cannot resolve a conflict without a local workspace".into(),
                cx,
            );
            return;
        };
        match persistence.discard_blocked(mutation_id) {
            Ok(projection) => {
                self.log_sync(SyncDirection::Outgoing, "User discarded the blocked local change; later changes require separate review", cx);
                self.replace_projection(projection, cx);
                self.start_outbox_replay(cx);
            }
            Err(error) => self.report_local_store_failure(error, cx),
        }
    }
}

enum ReplayProgress {
    Continue,
    Finished,
}

enum ReplayFailure {
    AuthenticationRequired,
    Offline,
    Error(String),
}

impl From<String> for ReplayFailure {
    fn from(error: String) -> Self {
        Self::Error(error)
    }
}

impl From<FetchError> for ReplayFailure {
    fn from(error: FetchError) -> Self {
        match error {
            FetchError::AuthenticationRequired => Self::AuthenticationRequired,
            FetchError::Request(error) => Self::Error(error),
        }
    }
}

enum RetryPull {
    CanonicalConfirmation,
    BeforeResend,
}

struct Replay {
    this: WeakEntity<AppDatabaseStore>,
    generation: u64,
    persistence: WorkspacePersistence,
    cmd_tx: CommandSender,
    wake_rx: flume::Receiver<()>,
}

impl Replay {
    fn update<R>(
        &self,
        cx: &mut AsyncApp,
        update: impl FnOnce(&mut AppDatabaseStore, &mut Context<AppDatabaseStore>) -> R,
    ) -> Option<R> {
        self.this
            .update(cx, |store, cx| {
                (store.workspace_generation == self.generation).then(|| update(store, cx))
            })
            .ok()
            .flatten()
    }

    async fn run(self, cx: &mut AsyncApp) {
        let result = loop {
            let active = self
                .update(cx, |_, cx| {
                    crate::auth::AuthSession::global(cx).is_signed_in()
                })
                .unwrap_or(false);
            if !active {
                break Ok(());
            }
            match self.advance(cx).await {
                Ok(ReplayProgress::Continue) => {}
                Ok(ReplayProgress::Finished) => break Ok(()),
                Err(error) => break Err(error),
            }
        };
        self.finish(result, cx);
    }

    async fn advance(&self, cx: &mut AsyncApp) -> Result<ReplayProgress, ReplayFailure> {
        let persistence = self.persistence.clone();
        let (state, oldest) = cx
            .background_spawn(async move {
                Ok::<_, String>((persistence.state()?, persistence.oldest_outbox()?))
            })
            .await?;
        let Some(oldest) = oldest else {
            return Ok(ReplayProgress::Finished);
        };

        match oldest.status {
            OutboxStatus::Blocked => return self.retry_blocked(oldest, cx).await,
            OutboxStatus::AwaitingCanonical => {
                let receipt = oldest.receipt.ok_or_else(|| {
                    "outbox entry is awaiting canonical state without a receipt".to_owned()
                })?;
                if let Some(refresh) = self
                    .pull_for_retry(
                        &state,
                        receipt.commit_seq,
                        RetryPull::CanonicalConfirmation,
                        cx,
                    )
                    .await?
                {
                    self.install_refresh(refresh, state, cx).await?;
                }
                return Ok(ReplayProgress::Continue);
            }
            OutboxStatus::Pending => {
                if let Some(retry_at) = oldest.next_attempt_at
                    && retry_at > Utc::now()
                    && let Ok(delay) = (retry_at - Utc::now()).to_std()
                {
                    if self.wait_for_retry(delay, cx).await {
                        self.record_retry(
                            oldest.mutation.request.mutation_id,
                            Utc::now,
                            "retry requested".into(),
                            cx,
                        )
                        .await?;
                    }
                    return Ok(ReplayProgress::Continue);
                }
            }
        }

        if oldest.sealed && oldest.attempt_count > 0 {
            let Some(refresh) = self
                .pull_for_retry(&state, state.canonical_seq, RetryPull::BeforeResend, cx)
                .await?
            else {
                return Ok(ReplayProgress::Continue);
            };
            self.install_refresh(refresh, state.clone(), cx).await?;
        }

        let request = &oldest.mutation.request;
        if !oldest.sealed && request.base_seq != state.canonical_seq {
            let refresh = request_refresh(
                &self.cmd_tx,
                request.base_seq,
                state.canonical_seq,
                state.dataset_id,
            )
            .await?;
            return self
                .resolve_stale(request, request.mutation_id, refresh, state, cx)
                .await;
        }

        self.send_next(state, cx).await
    }

    async fn retry_blocked(
        &self,
        oldest: OutboxEntry,
        cx: &mut AsyncApp,
    ) -> Result<ReplayProgress, ReplayFailure> {
        let compatibility = oldest
            .blocked_error
            .as_ref()
            .is_some_and(|error| oldest.mutation.request.is_compatibility_rejection(error));
        let legacy_stale_base = oldest.is_legacy_stale_base_block();
        if !compatibility && !legacy_stale_base {
            return Ok(ReplayProgress::Finished);
        }
        let persistence = self.persistence.clone();
        let mutation_id = oldest.mutation.request.mutation_id;
        let projection = cx
            .background_spawn(async move {
                if compatibility {
                    persistence.retry_compatibility_rejection(mutation_id)
                } else {
                    persistence.retry_blocked(mutation_id, Uuid::now_v7())
                }
            })
            .await?;
        let _ = self.update(cx, |store, cx| {
            if compatibility {
                store.log_sync(SyncDirection::Outgoing,
                    "Automatically retrying a server compatibility rejection with the original request; conflict checks remain enabled", cx);
            }
            store.replace_projection(projection, cx);
        });
        Ok(ReplayProgress::Continue)
    }

    async fn pull_for_retry(
        &self,
        state: &SyncState,
        expected_seq: i64,
        purpose: RetryPull,
        cx: &mut AsyncApp,
    ) -> Result<Option<RefreshData>, ReplayFailure> {
        match request_refresh(
            &self.cmd_tx,
            state.canonical_seq,
            expected_seq,
            state.dataset_id,
        )
        .await
        {
            Ok(refresh) => Ok(Some(refresh)),
            Err(FetchError::AuthenticationRequired) => Err(ReplayFailure::AuthenticationRequired),
            Err(FetchError::Request(error)) => {
                let _ = self.update(cx, |store, cx| {
                    store.sync_status = SyncStatus::Offline;
                    let summary = match purpose {
                        RetryPull::CanonicalConfirmation => {
                            tracing::warn!(%error, "canonical confirmation pull failed; retrying");
                            "Canonical confirmation failed; changes remain queued and will retry"
                        }
                        RetryPull::BeforeResend => {
                            tracing::warn!(%error, "pre-retry pull failed; retaining exact request");
                            "Pre-retry pull failed; original request retained for retry"
                        }
                    };
                    store.log_sync(SyncDirection::Incoming, summary, cx);
                    cx.notify();
                });
                self.wait_for_retry(Duration::from_secs(5), cx).await;
                Ok(None)
            }
        }
    }

    async fn install_refresh(
        &self,
        refresh: RefreshData,
        state: SyncState,
        cx: &mut AsyncApp,
    ) -> Result<(), ReplayFailure> {
        let summary = refresh.summary();
        let persistence = self.persistence.clone();
        let projection = cx
            .background_spawn(async move { persist_refresh(&persistence, refresh, state) })
            .await?;
        let _ = self.update(cx, |store, cx| {
            store.last_successful_sync = Some(Utc::now());
            store.log_sync(SyncDirection::Incoming, summary, cx);
            store.replace_projection(projection, cx);
        });
        Ok(())
    }

    async fn resolve_stale(
        &self,
        request: &MutationRequest,
        replacement_mutation_id: Uuid,
        refresh: RefreshData,
        state: SyncState,
        cx: &mut AsyncApp,
    ) -> Result<ReplayProgress, ReplayFailure> {
        let persistence = self.persistence.clone();
        let mutation_id = request.mutation_id;
        let since = request.base_seq;
        let resolution = cx
            .background_spawn(async move {
                resolve_stale_refresh(
                    &persistence,
                    mutation_id,
                    replacement_mutation_id,
                    since,
                    refresh,
                    state,
                )
            })
            .await?;
        let kind = resolution.kind;
        let _ = self.update(cx, |store, cx| {
            store.last_successful_sync = Some(Utc::now());
            store.log_sync(SyncDirection::Status, stale_resolution_summary(&kind), cx);
            store.replace_projection(resolution.projection, cx);
        });
        Ok(match kind {
            StaleResolutionKind::Rebased { .. } => ReplayProgress::Continue,
            StaleResolutionKind::Blocked { .. } => ReplayProgress::Finished,
        })
    }

    async fn send_next(
        &self,
        state: SyncState,
        cx: &mut AsyncApp,
    ) -> Result<ReplayProgress, ReplayFailure> {
        let persistence = self.persistence.clone();
        let sendable = cx
            .background_spawn(async move { persistence.next_sendable(Utc::now()) })
            .await?;
        let Some(sendable) = sendable else {
            return Ok(ReplayProgress::Finished);
        };
        let request = sendable.mutation.request;
        let mutation_id = request.mutation_id;
        let attempt_count = sendable.attempt_count;
        let operation = request.operation.name();
        let _ = self.update(cx, |store, cx| {
            store.sync_status = SyncStatus::Syncing;
            store.log_sync(
                SyncDirection::Outgoing,
                format!(
                    "Sending {operation} (attempt {attempt_count}, base sequence {})",
                    request.base_seq
                ),
                cx,
            );
        });
        let (reply_tx, reply_rx) = flume::bounded(1);
        if self
            .cmd_tx
            .send(Cmd::SendMutation(request.clone(), reply_tx))
            .is_err()
        {
            let _ = self
                .record_retry(
                    mutation_id,
                    move || retry_at(mutation_id, attempt_count),
                    "the synchronization worker is unavailable".into(),
                    cx,
                )
                .await;
            return Err(ReplayFailure::Offline);
        }

        match reply_rx.recv_async().await {
            Ok(Ok(receipt)) => {
                let persistence = self.persistence.clone();
                cx.background_spawn({
                    let receipt = receipt.clone();
                    async move { persistence.record_receipt(receipt) }
                })
                .await?;
                let _ = self.update(cx, |store, cx| {
                    store.log_sync(SyncDirection::Outgoing,
                        format!("Server acknowledged {operation} at sequence {}{}; awaiting canonical confirmation",
                            receipt.commit_seq, if receipt.replayed { " (replayed receipt)" } else { "" }), cx);
                });
                Ok(ReplayProgress::Continue)
            }
            Ok(Err(MutationSendError::AuthenticationRequired)) => {
                let _ = self
                    .record_retry(mutation_id, Utc::now, "authentication required".into(), cx)
                    .await;
                Err(ReplayFailure::AuthenticationRequired)
            }
            Ok(Err(MutationSendError::Transport(error))) => {
                let _ = self
                    .record_retry(
                        mutation_id,
                        move || retry_at(mutation_id, attempt_count),
                        error.clone(),
                        cx,
                    )
                    .await;
                let _ = self.update(cx, |store, cx| {
                    store.sync_status = SyncStatus::Offline;
                    tracing::warn!(%error, "mutation transport failed; retry remains durable");
                    store.log_sync(SyncDirection::Outgoing, format!("{operation}: transport failure; original request retained for automatic retry"), cx);
                    cx.notify();
                });
                Ok(ReplayProgress::Continue)
            }
            Ok(Err(MutationSendError::Api(error))) => {
                self.handle_api_error(request, attempt_count, error, state, cx)
                    .await
            }
            Err(_) => Err(ReplayFailure::Error(
                "the mutation worker stopped unexpectedly".into(),
            )),
        }
    }

    async fn handle_api_error(
        &self,
        request: MutationRequest,
        attempt_count: u32,
        error: ApiErrorBody,
        state: SyncState,
        cx: &mut AsyncApp,
    ) -> Result<ReplayProgress, ReplayFailure> {
        let mutation_id = request.mutation_id;
        if is_authentication_error(error.error) {
            let _ = self
                .record_retry(mutation_id, Utc::now, error.message, cx)
                .await;
            return Err(ReplayFailure::AuthenticationRequired);
        }
        if error.error == ApiErrorCode::StaleBase {
            let current_seq = validate_stale_base_response(&request, &error)?;
            let refresh = request_refresh(
                &self.cmd_tx,
                request.base_seq,
                current_seq,
                Some(request.dataset_id),
            )
            .await?;
            return self
                .resolve_stale(&request, Uuid::now_v7(), refresh, state, cx)
                .await;
        }
        let compatibility = request.is_compatibility_rejection(&error);
        if error.retryable
            || error.error == ApiErrorCode::AuthenticationUnavailable
            || compatibility
        {
            let next_attempt = mutation_retry_at(&request, &error, attempt_count);
            self.record_retry(mutation_id, move || next_attempt, error.message, cx)
                .await?;
            let _ = self.update(cx, |store, cx| {
                store.sync_status = SyncStatus::Offline;
                let operation = request.operation.name();
                store.log_sync(SyncDirection::Outgoing, if compatibility {
                    format!("{operation}: server upgrade required; original request will retry automatically at {}", next_attempt.format("%H:%M:%S UTC"))
                } else {
                    format!("{operation}: temporary server failure; automatic retry scheduled")
                }, cx);
                cx.notify();
            });
            return Ok(ReplayProgress::Continue);
        }
        let persistence = self.persistence.clone();
        let projection = cx
            .background_spawn(async move { persistence.block(mutation_id, error) })
            .await?;
        let _ = self.update(cx, |store, cx| store.replace_projection(projection, cx));
        Ok(ReplayProgress::Finished)
    }

    async fn record_retry(
        &self,
        mutation_id: Uuid,
        at: impl FnOnce() -> DateTime<Utc> + Send + 'static,
        error: String,
        cx: &mut AsyncApp,
    ) -> Result<(), String> {
        let persistence = self.persistence.clone();
        cx.background_spawn(async move { persistence.record_retry(mutation_id, at(), error) })
            .await
    }

    async fn wait_for_retry(&self, delay: Duration, cx: &mut AsyncApp) -> bool {
        let _ = self.update(cx, |store, cx| {
            store.outbox_retry_waiting = true;
            cx.notify();
        });
        let timer = cx.background_executor().timer(delay);
        let woken = matches!(
            futures_util::future::select(Box::pin(timer), Box::pin(self.wake_rx.recv_async()))
                .await,
            futures_util::future::Either::Right((Ok(()), _))
        );
        let _ = self.update(cx, |store, cx| {
            store.outbox_retry_waiting = false;
            if woken {
                store.log_sync(
                    SyncDirection::Status,
                    "Retry wait interrupted; checking synchronization now",
                    cx,
                );
            }
            cx.notify();
        });
        woken
    }

    fn finish(&self, result: Result<(), ReplayFailure>, cx: &mut AsyncApp) {
        let (status, error) = match result {
            Ok(_) => (SyncStatus::Idle, None),
            Err(ReplayFailure::AuthenticationRequired) => {
                (SyncStatus::AuthenticationRequired, None)
            }
            Err(ReplayFailure::Offline) => (SyncStatus::Offline, None),
            Err(ReplayFailure::Error(error)) => (SyncStatus::Offline, Some(error)),
        };
        let _ = self.update(cx, |store, cx| {
            store.outbox_replay_in_flight = false;
            store.outbox_retry_waiting = false;
            store.outbox_wake_tx = None;
            store.sync_status = status.clone();
            if status == SyncStatus::AuthenticationRequired {
                store.log_sync(SyncDirection::Status, "Uploads paused: authentication required; original requests retained", cx);
            }
            if let Some(error) = error {
                tracing::warn!(%error, "outbox replay paused; durable local intent was retained");
                store.log_sync(SyncDirection::Status, "Upload replay paused by a request or persistence failure; durable changes retained for retry", cx);
            }
            if status == SyncStatus::Offline && store.pending_count > 0 {
                store.sync_request.request();
                store.sync_retry_not_before = Some(Instant::now() + SYNC_RETRY_DELAY);
            }
            store.start_requested_sync(cx);
            cx.notify();
        });
    }
}

fn stale_resolution_summary(kind: &StaleResolutionKind) -> &'static str {
    match kind {
        StaleResolutionKind::Rebased { .. } => {
            "Automatically rebased a stale change after proving no overlapping server edits"
        }
        StaleResolutionKind::Blocked { .. } => {
            "Stale change needs review: overlapping server edits or insufficient history; local intent retained"
        }
    }
}

fn mutation_retry_at(
    request: &MutationRequest,
    error: &ApiErrorBody,
    attempt_count: u32,
) -> DateTime<Utc> {
    if request.is_compatibility_rejection(error) {
        Utc::now() + chrono::Duration::seconds(30)
    } else {
        retry_at(request.mutation_id, attempt_count)
    }
}

fn retry_at(mutation_id: Uuid, attempt_count: u32) -> DateTime<Utc> {
    let exponent = attempt_count.saturating_sub(1).min(8);
    let seconds = 2_u64.saturating_pow(exponent).min(30);
    let bytes = mutation_id.as_bytes();
    let jitter_ms = u16::from_be_bytes([bytes[0], bytes[1]]) as i64 % 1_000;
    Utc::now()
        + chrono::Duration::seconds(seconds as i64)
        + chrono::Duration::milliseconds(jitter_ms)
}

async fn request_refresh(
    cmd_tx: &CommandSender,
    since: i64,
    expected_seq: i64,
    expected_dataset_id: Option<Uuid>,
) -> Result<RefreshData, FetchError> {
    let (reply, rx) = flume::bounded(1);
    cmd_tx
        .send(Cmd::FetchDelta {
            since,
            expected_seq,
            expected_dataset_id,
            reply,
        })
        .map_err(|_| FetchError::Request("the synchronization worker is unavailable".into()))?;
    rx.recv_async().await.map_err(|_| {
        FetchError::Request("the synchronization worker stopped unexpectedly".into())
    })?
}

pub(super) fn persist_refresh(
    persistence: &WorkspacePersistence,
    refresh: RefreshData,
    expected_state: SyncState,
) -> Result<Projection, String> {
    match refresh {
        RefreshData::Full(data) => persistence.install_snapshot(*data, expected_state),
        RefreshData::Delta(delta) => persistence.apply_delta(*delta),
    }
}

fn resolve_stale_refresh(
    persistence: &WorkspacePersistence,
    mutation_id: Uuid,
    replacement_mutation_id: Uuid,
    since: i64,
    refresh: RefreshData,
    expected_state: SyncState,
) -> Result<StaleResolution, String> {
    match refresh {
        RefreshData::Delta(delta) => {
            persistence.resolve_stale_delta(mutation_id, replacement_mutation_id, since, *delta)
        }
        RefreshData::Full(data) => {
            persistence.resolve_stale_snapshot(mutation_id, *data, expected_state)
        }
    }
}

pub(super) fn validate_stale_base_response(
    request: &MutationRequest,
    error: &ApiErrorBody,
) -> Result<i64, String> {
    if error.error != ApiErrorCode::StaleBase
        || error.mutation_id != Some(request.mutation_id)
        || error.current_dataset_id != Some(request.dataset_id)
    {
        return Err(
            "server returned a stale-base response for a different mutation or dataset".into(),
        );
    }
    let current_seq = error
        .current_seq
        .ok_or_else(|| "server stale-base response omitted current_seq".to_owned())?;
    if current_seq <= request.base_seq {
        return Err(format!(
            "server stale-base response regressed from base {} to {current_seq}",
            request.base_seq
        ));
    }
    Ok(current_seq)
}
