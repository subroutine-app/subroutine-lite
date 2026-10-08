use super::replay::persist_refresh;
use super::transport::{Cmd, FetchError};
use super::{
    AppDatabaseStore, DatabaseError, StoreStatus, SyncDirection, SyncStatus, WorkspacePersistence,
};
use crate::auth::AuthSession;
use crate::settings::Settings;
use crate::stores::snapshot_summary;
use chrono::Utc;
use gpui::{App, AppContext, Context};
use std::time::{Duration, Instant, SystemTime};
use uuid::Uuid;

const SSE_COALESCE_WINDOW: Duration = Duration::from_millis(250);
const SSE_COALESCE_MAX: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SseSignal {
    Delta { seq: i64 },
    Full,
}

impl SseSignal {
    pub(super) fn coalesce(self, next: Self) -> Self {
        match (self, next) {
            (Self::Full, _) | (_, Self::Full) => Self::Full,
            (Self::Delta { seq: left }, Self::Delta { seq: right }) => Self::Delta {
                seq: left.max(right),
            },
        }
    }
}

pub(super) const SYNC_RETRY_DELAY: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(super) struct RecoveryBackoff {
    pub(super) failures: u32,
}

impl RecoveryBackoff {
    pub(super) fn next_delay(&mut self) -> Duration {
        let delay = Duration::from_secs((5 * (1u64 << self.failures.min(4))).min(60));
        self.failures = self.failures.saturating_add(1);
        delay
    }

    pub(super) fn reset(&mut self) {
        self.failures = 0;
    }
}

fn resumed_after_pause(previous: SystemTime, now: SystemTime) -> bool {
    now.duration_since(previous)
        .is_ok_and(|gap| gap > Duration::from_secs(5))
}

fn manual_sync_unavailable_reason(
    ready: bool,
    local_available: bool,
    remote_configured: bool,
    worker_available: bool,
    signed_in: bool,
    busy: bool,
) -> Option<&'static str> {
    if !local_available {
        Some("Can’t open local data.")
    } else if !ready {
        Some("Wait for your data to load.")
    } else if !remote_configured {
        Some("No server connection.")
    } else if !worker_available {
        Some("Sync stopped. Restart Subroutine Lite to try again.")
    } else if !signed_in {
        Some("Sign in to sync.")
    } else if busy {
        Some("Syncing…")
    } else {
        None
    }
}

#[derive(Default)]
pub(super) struct SyncRequestLatch {
    pub(super) requested: bool,
}

impl SyncRequestLatch {
    pub(super) fn request(&mut self) {
        self.requested = true;
    }

    pub(super) fn take_if_eligible(
        &mut self,
        busy: bool,
        signed_in: bool,
        retry_ready: bool,
    ) -> bool {
        if !self.requested || busy || !signed_in || !retry_ready {
            return false;
        }
        self.requested = false;
        true
    }
}

pub(super) fn should_periodic_poll(
    signed_in: bool,
    _sse_connected: bool,
    _pending_count: usize,
    _status: &SyncStatus,
) -> bool {
    signed_in
}

impl AppDatabaseStore {
    pub(super) fn start_bootstrap(&mut self, cx: &mut Context<Self>) {
        self.request_sync(cx);
    }

    fn request_sync(&mut self, cx: &mut Context<Self>) {
        if !AuthSession::global(cx).is_signed_in() {
            return;
        }
        self.sync_request.request();
        self.start_requested_sync(cx);
    }

    fn wake_outbox_retry(&self) {
        if self.outbox_retry_waiting
            && let Some(wake) = &self.outbox_wake_tx
        {
            let _ = wake.try_send(());
        }
    }

    fn request_sync_immediately(&mut self, cx: &mut Context<Self>) {
        self.sync_retry_not_before = None;
        self.request_sync(cx);
    }

    pub(crate) fn recover_connection(&mut self, cx: &mut Context<Self>) {
        if !AuthSession::global(cx).is_signed_in() || self._worker.is_none() {
            return;
        }
        if let Some(reconnect) = &self.sse_reconnect_tx {
            let _ = reconnect.try_send(());
        }
        self.wake_outbox_retry();
        self.request_sync_immediately(cx);
    }

    pub(super) fn start_requested_sync(&mut self, cx: &mut Context<Self>) {
        let signed_in = AuthSession::global(cx).is_signed_in();
        let retry_ready = self
            .sync_retry_not_before
            .is_none_or(|deadline| Instant::now() >= deadline);
        let busy = self.sync_in_flight || self.outbox_replay_in_flight;
        if !self
            .sync_request
            .take_if_eligible(busy, signed_in, retry_ready)
        {
            if self.sync_request.requested && !busy && signed_in && !retry_ready {
                self.schedule_sync_retry(cx);
            }
            return;
        }

        let Some(persistence) = self.persistence.clone() else {
            self.sync_request.request();
            return;
        };
        let auth = AuthSession::global(cx);
        if self.cmd_tx.scope != auth.scope() || persistence.account_id() != auth.account_id() {
            self.sync_status = SyncStatus::AuthenticationRequired;
            return;
        }
        self.sync_retry_not_before = None;
        if let Some(dataset_id) = self.dataset_id {
            self.begin_pull(persistence, dataset_id, cx);
        } else {
            self.begin_bootstrap(persistence, cx);
        }
    }

    fn schedule_sync_retry(&mut self, cx: &mut Context<Self>) {
        let Some(deadline) = self.sync_retry_not_before else {
            return;
        };
        if self.sync_retry_scheduled == Some(deadline) {
            return;
        }
        self.sync_retry_scheduled = Some(deadline);
        let generation = self.workspace_generation;
        let delay = deadline.saturating_duration_since(Instant::now());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |store, cx| {
                if store.workspace_generation != generation
                    || store.sync_retry_scheduled != Some(deadline)
                {
                    return;
                }
                store.sync_retry_scheduled = None;
                store.start_requested_sync(cx);
            });
        })
        .detach();
    }

    fn begin_bootstrap(&mut self, persistence: WorkspacePersistence, cx: &mut Context<Self>) {
        self.log_sync(SyncDirection::Incoming, "Requesting account snapshot", cx);
        self.sync_in_flight = true;
        self.sync_status = SyncStatus::Syncing;
        cx.notify();
        let generation = self.workspace_generation;
        let (tx, rx) = flume::bounded(1);
        if self.cmd_tx.send(Cmd::Bootstrap(tx)).is_err() {
            self.report_sync_failure("the synchronization worker is unavailable".into(), cx);
            self.finish_sync_attempt(false, cx);
            return;
        }
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.recv_async().await else {
                let _ = this.update(cx, |store, cx| {
                    if store.workspace_generation == generation {
                        store.report_sync_failure(
                            "the synchronization worker stopped unexpectedly".into(),
                            cx,
                        );
                        store.finish_sync_attempt(false, cx);
                    }
                });
                return;
            };
            match result {
                Ok(bootstrap) => {
                    let summary = snapshot_summary(&bootstrap.data);
                    let materialized = cx
                        .background_spawn(async move {
                            persistence.activate_remote(bootstrap.account, bootstrap.data)
                        })
                        .await;
                    let _ = this.update(cx, |store, cx| {
                        if store.workspace_generation != generation {
                            return;
                        }
                        match materialized {
                            Ok((persistence, projection)) => {
                                if let Err(error) = persistence.mark_active() {
                                    tracing::warn!(%error, "could not remember the active account workspace");
                                }
                                store.persistence = Some(persistence);
                                store.workspace_generation =
                                    store.workspace_generation.wrapping_add(1);
                                store.sync_retry_scheduled = None;
                                store.sync_activity.clear();
                                store.log_sync(SyncDirection::Incoming, summary, cx);
                                store.sync_status = SyncStatus::Idle;
                                store.last_successful_sync = Some(Utc::now());
                                store.replace_projection(projection, cx);
                                store.finish_sync_attempt(true, cx);
                            }
                            Err(error) => {
                                store.report_sync_failure(error, cx);
                                store.finish_sync_attempt(false, cx);
                            }
                        }
                    });
                }
                Err(FetchError::AuthenticationRequired) => {
                    let _ = this.update(cx, |store, cx| {
                        if store.workspace_generation == generation {
                            tracing::info!("synchronization is paused pending authentication");
                            store.log_sync(SyncDirection::Status, "Sync paused: authentication required; local changes retained", cx);
                            store.sync_in_flight = false;
                            store.sync_status = SyncStatus::AuthenticationRequired;
                            store.sync_retry_not_before =
                                Some(Instant::now() + SYNC_RETRY_DELAY);
                            store.start_requested_sync(cx);
                            cx.notify();
                        }
                    });
                }
                Err(FetchError::Request(error)) => {
                    let _ = this.update(cx, |store, cx| {
                        if store.workspace_generation == generation {
                            store.report_sync_failure(error, cx);
                            store.finish_sync_attempt(false, cx);
                        }
                    });
                }
            }
        })
        .detach();
    }

    pub(super) fn listen_for_sse_status(
        &self,
        status_rx: flume::Receiver<bool>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            while let Ok(connected) = status_rx.recv_async().await {
                if this
                    .update(cx, |store, cx| {
                        let reconnected = connected && !store.sse_connected;
                        if connected != store.sse_connected {
                            store.log_sync(SyncDirection::Status, if connected {
                                "Live change stream connected; checking for missed changes"
                            } else {
                                "Live change stream disconnected; automatic reconnect and polling remain enabled"
                            }, cx);
                        }
                        store.sse_connected = connected;
                        if reconnected {
                            store.wake_outbox_retry();
                            store.request_sync_immediately(cx);
                        } else {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    pub(super) fn start_periodic_sync(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let mut interval = cx.update(|cx| Settings::global(cx).account_sync.interval());
            let mut next_sync = Instant::now() + interval;
            let mut previous_tick = SystemTime::now();
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let now = SystemTime::now();
                let resumed = resumed_after_pause(previous_tick, now);
                previous_tick = now;
                if resumed {
                    if this
                        .update(cx, |store, cx| store.recover_connection(cx))
                        .is_err()
                    {
                        break;
                    }
                    next_sync = Instant::now() + interval;
                }
                let configured = cx.update(|cx| Settings::global(cx).account_sync.interval());
                if configured != interval {
                    interval = configured;
                    next_sync = Instant::now() + interval;
                }
                if Instant::now() < next_sync {
                    continue;
                }
                next_sync = Instant::now() + interval;
                if this
                    .update(cx, |store, cx| store.start_periodic_pull(cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    fn start_periodic_pull(&mut self, cx: &mut Context<Self>) {
        if should_periodic_poll(
            AuthSession::global(cx).is_signed_in(),
            self.sse_connected,
            self.pending_count,
            &self.sync_status,
        ) {
            self.start_pull(cx);
        }
    }

    pub(super) fn start_pull(&mut self, cx: &mut Context<Self>) {
        self.request_sync(cx);
    }

    fn begin_pull(
        &mut self,
        persistence: WorkspacePersistence,
        dataset_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        let expected_state = match persistence.state() {
            Ok(state) => state,
            Err(error) => {
                self.report_sync_failure(error, cx);
                self.finish_sync_attempt(false, cx);
                return;
            }
        };
        let generation = self.workspace_generation;
        let since = self.last_applied_seq;
        self.log_sync(
            SyncDirection::Incoming,
            format!("Checking server changes after sequence {since}"),
            cx,
        );
        let (tx, rx) = flume::bounded(1);
        self.sync_in_flight = true;
        self.sync_status = SyncStatus::Syncing;
        cx.notify();
        if self
            .cmd_tx
            .send(Cmd::FetchDelta {
                since,
                expected_seq: since,
                expected_dataset_id: Some(dataset_id),
                reply: tx,
            })
            .is_err()
        {
            self.report_sync_failure("the synchronization worker is unavailable".into(), cx);
            self.finish_sync_attempt(false, cx);
            return;
        }

        cx.spawn(async move |this, cx| {
            let result = match rx.recv_async().await {
                Ok(Ok(data)) => {
                    let summary = data.summary();
                    cx.background_spawn(async move {
                        persist_refresh(&persistence, data, expected_state)
                    })
                    .await
                    .map(|projection| (projection, summary))
                }
                Ok(Err(FetchError::AuthenticationRequired)) => {
                    let _ = this.update(cx, |store, cx| {
                        if store.workspace_generation == generation {
                            store.sync_in_flight = false;
                            store.sync_status = SyncStatus::AuthenticationRequired;
                            store.log_sync(
                                SyncDirection::Status,
                                "Pull paused: authentication required; local changes retained",
                                cx,
                            );
                            store.sync_retry_not_before = Some(Instant::now() + SYNC_RETRY_DELAY);
                            store.start_requested_sync(cx);
                            cx.notify();
                        }
                    });
                    return;
                }
                Ok(Err(FetchError::Request(error))) => Err(error),
                Err(_) => Err("the synchronization worker stopped unexpectedly".into()),
            };
            let _ = this.update(cx, |store, cx| {
                if store.workspace_generation != generation {
                    return;
                }
                match result {
                    Ok((projection, summary)) => {
                        store.log_sync(SyncDirection::Incoming, summary, cx);
                        store.sync_status = SyncStatus::Idle;
                        store.last_successful_sync = Some(Utc::now());
                        store.replace_projection(projection, cx);
                        store.finish_sync_attempt(true, cx);
                    }
                    Err(error) => {
                        store.report_sync_failure(error, cx);
                        store.finish_sync_attempt(false, cx);
                    }
                }
            });
        })
        .detach();
    }

    fn finish_sync_attempt(&mut self, succeeded: bool, cx: &mut Context<Self>) {
        self.sync_in_flight = false;
        if succeeded {
            self.recovery_backoff.reset();
            self.sync_retry_not_before = None;
        } else {
            self.sync_request.request();
            self.sync_retry_not_before = Some(Instant::now() + self.recovery_backoff.next_delay());
        }

        if self.pending_count > 0 {
            self.start_outbox_replay(cx);
        } else {
            self.start_requested_sync(cx);
        }
        cx.notify();
    }

    pub(super) fn listen_for_sse(
        &self,
        sse_rx: flume::Receiver<SseSignal>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            loop {
                let Ok(mut signal) = sse_rx.recv_async().await else {
                    break;
                };
                let deadline = Instant::now() + SSE_COALESCE_MAX;
                let mut coalesced = 1usize;
                loop {
                    cx.background_executor().timer(SSE_COALESCE_WINDOW).await;
                    let drained: Vec<_> = sse_rx.drain().collect();
                    let drained_count = drained.len();
                    coalesced += drained_count;
                    for next in drained {
                        signal = signal.coalesce(next);
                    }
                    if drained_count == 0 || Instant::now() >= deadline {
                        break;
                    }
                }

                if this
                    .update(cx, |store, cx| {
                        if matches!(signal, SseSignal::Delta { seq } if seq <= store.last_applied_seq)
                        {
                            return;
                        }
                        tracing::debug!(
                            coalesced,
                            ?signal,
                            last_seq = store.last_applied_seq,
                            "SSE change burst, requesting synchronization"
                        );
                        store.request_sync_immediately(cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    fn report_sync_failure(&mut self, error: String, cx: &mut Context<Self>) {
        tracing::error!(%error, "synchronization failed; retaining the local projection");
        self.log_sync(SyncDirection::Status, "Sync failed; last verified data and local changes retained. Automatic retry scheduled.", cx);
        self.sync_status = SyncStatus::Offline;
        if self.persistence.is_none() {
            self.status = StoreStatus::Error(error.clone());
            cx.emit(DatabaseError { _message: error });
        }
        cx.notify();
    }

    pub(crate) fn sync_unavailable_reason(&self, cx: &App) -> Option<&'static str> {
        manual_sync_unavailable_reason(
            self.is_ready() && !self.local_retry_in_flight,
            self.persistence.is_some(),
            self.server_url
                .as_deref()
                .is_some_and(|url| !url.trim().is_empty()),
            self._worker
                .as_ref()
                .is_some_and(|worker| !worker.is_finished())
                && !self.cmd_tx.is_disconnected(),
            AuthSession::global(cx).is_signed_in(),
            self.sync_busy(),
        )
    }

    pub(crate) fn sync_now(&mut self, cx: &mut Context<Self>) -> Result<(), &'static str> {
        if let Some(reason) = self.sync_unavailable_reason(cx) {
            return Err(reason);
        }
        self.wake_outbox_retry();
        self.request_sync_immediately(cx);
        Ok(())
    }

    pub(crate) fn sync_busy(&self) -> bool {
        self.sync_in_flight || (self.outbox_replay_in_flight && !self.outbox_retry_waiting)
    }
}
