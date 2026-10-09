use super::{
    SyncActivity, SyncActivityLog, SyncDirection, UndoHistory,
    local_store::{WorkspacePersistence, workspace_root_from_env},
};
use crate::auth::AuthSession;
use crate::item_subject::SavedItem;
use crate::settings::Settings;
use chrono::{DateTime, Local, Utc};
use gpui::{App, AppContext, Context, Entity, EventEmitter, Global};
use local_store::{BlockedConflict, Projection};
use std::thread;
use std::time::Instant;
use subroutine_core::{
    Action, ActionTemplate, AllData, AnyItem, Event, EventTemplate, Marker, MarkerTemplate,
    PipelineContext, Routine, Signal, SignalTemplate,
};
use uuid::Uuid;

mod actions;
mod history;
mod items;
mod replay;
mod sync;
mod transport;
mod workspace;

use history::StoreChange;
use sync::{RecoveryBackoff, SseSignal, SyncRequestLatch};
use transport::CommandSender;

#[derive(Debug, Clone, PartialEq)]
pub enum StoreStatus {
    NotConfigured,
    Ready,
    AuthenticationRequired,
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SyncStatus {
    Idle,
    Syncing,
    AuthenticationRequired,
    Offline,
}

pub struct DatabaseError {
    pub _message: String,
}

pub type SaveResult<T = ()> = Result<T, String>;

pub struct SaveFailed {
    pub message: String,
}

pub struct DataChanged;
pub struct ActionDataChanged;
pub struct EventDataChanged;
pub struct RoutineDataChanged;
pub struct ActionTemplateDataChanged;
pub struct EventTemplateDataChanged;
pub struct MarkerDataChanged;
pub struct SignalDataChanged;

pub struct AppDatabaseStore {
    cmd_tx: CommandSender,
    _worker: Option<thread::JoinHandle<()>>,
    server_url: Option<String>,
    status: StoreStatus,
    local_retry_in_flight: bool,
    actions: Vec<Action>,
    events: Vec<Event>,
    routines: Vec<Routine>,
    markers: Vec<Marker>,
    signals: Vec<Signal>,
    action_templates: Vec<ActionTemplate>,
    event_templates: Vec<EventTemplate>,
    marker_templates: Vec<MarkerTemplate>,
    signal_templates: Vec<SignalTemplate>,
    dataset_id: Option<Uuid>,
    last_applied_seq: i64,
    history: UndoHistory<StoreChange>,
    persistence: Option<WorkspacePersistence>,
    workspace_generation: u64,
    auth_scope: Option<crate::auth::RequestScope>,
    sync_in_flight: bool,
    sync_request: SyncRequestLatch,
    sync_retry_not_before: Option<Instant>,
    sync_retry_scheduled: Option<Instant>,
    recovery_backoff: RecoveryBackoff,
    sse_reconnect_tx: Option<flume::Sender<()>>,
    sync_status: SyncStatus,
    sync_activity: SyncActivityLog,
    last_successful_sync: Option<DateTime<Utc>>,
    sse_connected: bool,
    pending_count: usize,
    blocked_count: usize,
    blocked_conflict: Option<BlockedConflict>,
    outbox_replay_in_flight: bool,
    outbox_retry_waiting: bool,
    outbox_wake_tx: Option<flume::Sender<()>>,
}

impl AppDatabaseStore {
    fn new(server_url: Option<String>, auth: AuthSession, cx: &mut Context<Self>) -> Self {
        let workspace_root = workspace_root_from_env();
        let initial = workspace_root
            .and_then(|root| WorkspacePersistence::open_initial(root, server_url.as_deref()));
        let (persistence, data, pending_count, blocked_count, blocked_conflict, status) =
            match initial {
                Ok(persistence) => match persistence.projection() {
                    Ok(projection) => (
                        Some(persistence),
                        projection.data,
                        projection.pending_count,
                        projection.blocked_count,
                        projection.blocked_conflict,
                        StoreStatus::Ready,
                    ),
                    Err(error) => {
                        tracing::error!(%error, "could not load the local projection");
                        (
                            None,
                            AllData::default(),
                            0,
                            0,
                            None,
                            StoreStatus::Error(error),
                        )
                    }
                },
                Err(error) => {
                    tracing::error!(%error, "could not open the local workspace");
                    (
                        None,
                        AllData::default(),
                        0,
                        0,
                        None,
                        StoreStatus::Error(error),
                    )
                }
            };

        let should_start_bootstrap = auth.is_signed_in();
        let (cmd_tx, cmd_rx) = flume::unbounded();
        let cmd_tx = CommandSender::new(cmd_tx);
        let (sse_tx, sse_rx) = flume::unbounded::<SseSignal>();
        let (sse_status_tx, sse_status_rx) = flume::unbounded::<bool>();
        let (sse_reconnect_tx, sse_reconnect_rx) = flume::bounded::<()>(1);
        let worker = server_url.as_ref().map(|server_url| {
            transport::spawn_worker(
                server_url.clone(),
                auth,
                cmd_rx,
                sse_tx,
                sse_status_tx,
                sse_reconnect_rx,
            )
        });

        let AllData {
            dataset_id,
            seq,
            actions,
            events,
            routines,
            markers,
            signals,
            action_templates,
            event_templates,
            marker_templates,
            signal_templates,
        } = data;
        let mut store = Self {
            cmd_tx,
            _worker: worker,
            server_url,
            status,
            local_retry_in_flight: false,
            actions,
            events,
            routines,
            markers,
            signals,
            action_templates,
            event_templates,
            marker_templates,
            signal_templates,
            dataset_id: (!dataset_id.is_nil()).then_some(dataset_id),
            last_applied_seq: seq,
            history: UndoHistory::default(),
            persistence,
            workspace_generation: 0,
            auth_scope: None,
            sync_in_flight: false,
            sync_request: SyncRequestLatch::default(),
            sync_retry_not_before: None,
            sync_retry_scheduled: None,
            recovery_backoff: RecoveryBackoff::default(),
            sse_reconnect_tx: Some(sse_reconnect_tx),
            sync_status: SyncStatus::Idle,
            sync_activity: SyncActivityLog::default(),
            last_successful_sync: None,
            sse_connected: false,
            pending_count,
            blocked_count,
            blocked_conflict,
            outbox_replay_in_flight: false,
            outbox_retry_waiting: false,
            outbox_wake_tx: None,
        };

        store.log_sync(
            SyncDirection::Status,
            format!(
                "Workspace opened: sequence {}, {} queued changes, {} blocked. Activity starts with this session.",
                store.last_applied_seq, store.pending_count, store.blocked_count
            ),
            cx,
        );
        if store.persistence.is_some() && store._worker.is_some() && should_start_bootstrap {
            store.start_bootstrap(cx);
        }
        store.listen_for_sse_status(sse_status_rx, cx);
        store.listen_for_sse(sse_rx, cx);
        store.start_periodic_sync(cx);
        store
    }

    pub fn initialize_global(
        server_url: Option<String>,
        auth: AuthSession,
        cx: &mut App,
    ) -> Entity<Self> {
        if cx.has_global::<GlobalStore>() {
            return cx.global::<GlobalStore>().0.clone();
        }
        let store = cx.new(|cx| Self::new(server_url, auth, cx));
        cx.set_global(GlobalStore(store.clone()));
        store
    }

    pub fn global(cx: &App) -> Entity<Self> {
        cx.global::<GlobalStore>().0.clone()
    }

    fn replace_projection(&mut self, projection: Projection, cx: &mut Context<Self>) {
        if let Some(conflict) = &projection.blocked_conflict
            && self.blocked_conflict.as_ref().is_none_or(|old| {
                old.mutation_id != conflict.mutation_id || old.error.error != conflict.error.error
            })
        {
            self.log_sync(SyncDirection::Outgoing, format!(
                "{} rejected ({:?}); {} queued changes are waiting. Review the blocked change below.",
                conflict.operation.name(), conflict.error.error, projection.pending_count
            ), cx);
        }
        if self.dataset_id == Some(projection.data.dataset_id)
            && projection.pending_count < self.pending_count
            && projection.blocked_count == 0
        {
            self.log_sync(
                SyncDirection::Outgoing,
                format!(
                    "{} queued change(s) retired; {} remaining",
                    self.pending_count - projection.pending_count,
                    projection.pending_count
                ),
                cx,
            );
        }
        self.pending_count = projection.pending_count;
        self.blocked_count = projection.blocked_count;
        self.blocked_conflict = projection.blocked_conflict;
        self.replace_all_data(projection.data, cx);
    }

    fn replace_all_data(&mut self, data: AllData, cx: &mut Context<Self>) {
        let dataset_id = (!data.dataset_id.is_nil()).then_some(data.dataset_id);
        if self.dataset_id != dataset_id {
            self.history.clear();
        }
        self.dataset_id = dataset_id;
        self.last_applied_seq = data.seq;
        self.actions = data.actions;
        self.events = data.events;
        self.routines = data.routines;
        self.markers = data.markers;
        self.signals = data.signals;
        self.action_templates = data.action_templates;
        self.event_templates = data.event_templates;
        self.marker_templates = data.marker_templates;
        self.signal_templates = data.signal_templates;
        self.status = StoreStatus::Ready;
        cx.emit(DataChanged);
        cx.emit(ActionDataChanged);
        cx.emit(EventDataChanged);
        cx.emit(RoutineDataChanged);
        cx.emit(MarkerDataChanged);
        cx.emit(SignalDataChanged);
        cx.emit(ActionTemplateDataChanged);
        cx.emit(EventTemplateDataChanged);
        cx.notify();
    }

    fn emit_all_changed(cx: &mut Context<Self>) {
        cx.emit(ActionDataChanged);
        cx.emit(EventDataChanged);
        cx.emit(RoutineDataChanged);
        cx.emit(MarkerDataChanged);
        cx.emit(SignalDataChanged);
        cx.emit(DataChanged);
        cx.notify();
    }

    pub fn status(&self) -> StoreStatus {
        self.status.clone()
    }

    pub fn sync_status(&self) -> SyncStatus {
        self.sync_status.clone()
    }

    pub fn sync_activity(&self) -> &[SyncActivity] {
        &self.sync_activity.entries
    }

    fn log_sync(
        &mut self,
        direction: SyncDirection,
        summary: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        let summary = summary.into();
        tracing::info!(?direction, %summary, "sync activity");
        self.sync_activity.push(direction, summary);
        cx.notify();
    }
    pub fn last_successful_sync(&self) -> Option<DateTime<Utc>> {
        self.last_successful_sync
    }

    pub fn pending_count(&self) -> usize {
        self.pending_count
    }

    pub fn blocked_count(&self) -> usize {
        self.blocked_count
    }

    pub fn blocked_conflict(&self) -> Option<&BlockedConflict> {
        self.blocked_conflict.as_ref()
    }

    pub fn is_ready(&self) -> bool {
        self.status == StoreStatus::Ready && !self.local_retry_in_flight
    }

    pub(crate) fn workspace_generation(&self) -> u64 {
        self.workspace_generation
    }

    pub fn get_item(&self, id: Uuid) -> Option<AnyItem> {
        if let Some(action) = self.actions.iter().find(|a| a.id == id) {
            return Some(AnyItem::Action(action.clone()));
        }
        if let Some(event) = self.events.iter().find(|e| e.id == id) {
            return Some(AnyItem::Event(event.clone()));
        }
        if let Some(routine) = self.routines.iter().find(|r| r.id == id) {
            return Some(AnyItem::Routine(routine.clone()));
        }
        if let Some(marker) = self.markers.iter().find(|m| m.id == id) {
            return Some(AnyItem::Marker(marker.clone()));
        }
        if let Some(signal) = self.signals.iter().find(|s| s.id == id) {
            return Some(AnyItem::Signal(signal.clone()));
        }
        if let Some(template) = self.action_templates.iter().find(|t| t.id == id) {
            return Some(AnyItem::ActionTemplate(template.clone()));
        }
        self.event_templates
            .iter()
            .find(|t| t.id == id)
            .map(|template| AnyItem::EventTemplate(template.clone()))
    }

    pub fn all_items(&self) -> Vec<AnyItem> {
        let mut items = Vec::with_capacity(
            self.actions.len()
                + self.events.len()
                + self.routines.len()
                + self.markers.len()
                + self.signals.len(),
        );
        items.extend(self.actions.iter().cloned().map(AnyItem::Action));
        items.extend(self.events.iter().cloned().map(AnyItem::Event));
        items.extend(self.routines.iter().cloned().map(AnyItem::Routine));
        items.extend(self.markers.iter().cloned().map(AnyItem::Marker));
        items.extend(self.signals.iter().cloned().map(AnyItem::Signal));
        items
    }

    pub fn actions(&self) -> &[Action] {
        &self.actions
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn routines(&self) -> &[Routine] {
        &self.routines
    }

    pub fn markers(&self) -> &[Marker] {
        &self.markers
    }

    pub fn signals(&self) -> &[Signal] {
        &self.signals
    }

    pub fn action_templates(&self) -> Vec<ActionTemplate> {
        self.action_templates.clone()
    }

    pub fn event_templates(&self) -> Vec<EventTemplate> {
        self.event_templates.clone()
    }

    pub fn get_action_template(&self, id: Uuid) -> Option<ActionTemplate> {
        self.action_templates
            .iter()
            .find(|template| template.id == id)
            .cloned()
    }

    pub fn get_event_template(&self, id: Uuid) -> Option<EventTemplate> {
        self.event_templates
            .iter()
            .find(|template| template.id == id)
            .cloned()
    }

    pub fn get_saved_item(&self, id: Uuid) -> Option<SavedItem> {
        self.get_action_template(id)
            .map(SavedItem::Action)
            .or_else(|| self.get_event_template(id).map(SavedItem::Event))
    }

    pub fn pipeline<'a>(&'a self, settings: &Settings) -> PipelineContext<'a> {
        PipelineContext::new(
            Local::now(),
            settings.schedule,
            &self.actions,
            &self.events,
            &self.routines,
            &self.markers,
            &self.signals,
        )
    }
}

impl EventEmitter<DatabaseError> for AppDatabaseStore {}
impl EventEmitter<SaveFailed> for AppDatabaseStore {}
impl EventEmitter<DataChanged> for AppDatabaseStore {}
impl EventEmitter<ActionDataChanged> for AppDatabaseStore {}
impl EventEmitter<EventDataChanged> for AppDatabaseStore {}
impl EventEmitter<RoutineDataChanged> for AppDatabaseStore {}
impl EventEmitter<ActionTemplateDataChanged> for AppDatabaseStore {}
impl EventEmitter<EventTemplateDataChanged> for AppDatabaseStore {}
impl EventEmitter<MarkerDataChanged> for AppDatabaseStore {}
impl EventEmitter<SignalDataChanged> for AppDatabaseStore {}

struct GlobalStore(Entity<AppDatabaseStore>);
impl Global for GlobalStore {}
