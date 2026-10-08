use std::{borrow::Cow, rc::Rc};

use chrono::{DateTime, Local, Utc};
use gpui::{
    App, Context, Entity, FocusHandle, FontWeight, InteractiveElement, IntoElement, ParentElement,
    Render, StatefulInteractiveElement as _, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_kit::{
    controls::{button::Button, settings_row::SettingsRow},
    foundation::{Disableable as _, FocusRing as _, Sizable as _, StyledExt as _},
    layout::{ScrollArea, scroll_offset, scroll_to},
    overlay::Tooltip,
};
use gpui_kit_assets::{Icon, icon};
use gpui_kit_semantics::{NodeSpec, Role, Semantic as _};
use gpui_kit_theme::ActiveTheme as _;
use subroutine_core::ApiErrorCode;
use uuid::Uuid;

use crate::{
    auth::AuthSession,
    stores::{AppDatabaseStore, StoreStatus, SyncActivity, SyncDirection, SyncStatus},
};

const SYNC_STATUS_ID: &str = "settings.sync.status";
const ACTIVITY_SCROLL_ID: &str = "settings.sync.activity.scroll";
const ACTIVITY_HEIGHT: f32 = 240.0;
const STALE_CONFIRMATION: &str = "That change was already resolved.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Resolution {
    RetryLocal,
    UseServer,
}

impl Resolution {
    fn label(self) -> &'static str {
        match self {
            Self::RetryLocal => "Keep My Version",
            Self::UseServer => "Use Server Version",
        }
    }

    fn warning(self) -> &'static str {
        match self {
            Self::RetryLocal => {
                "Send your version again? It may replace the version on the server."
            }
            Self::UseServer => "Discard your local edits to this item? This can’t be undone.",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ResolutionRequest {
    mutation_id: Uuid,
    resolution: Resolution,
}

#[derive(Default)]
struct Confirmation {
    pending: Option<ResolutionRequest>,
}

impl Confirmation {
    fn request(&mut self, request: ResolutionRequest) {
        self.pending = Some(request);
    }

    fn cancel(&mut self) -> Option<ResolutionRequest> {
        self.pending.take()
    }

    fn invalidate_if_stale(&mut self, current: Option<Uuid>) -> bool {
        if self
            .pending
            .is_some_and(|request| Some(request.mutation_id) != current)
        {
            self.pending = None;
            return true;
        }
        false
    }

    fn confirm(
        &mut self,
        expected: ResolutionRequest,
        current: Option<Uuid>,
    ) -> Option<ResolutionRequest> {
        let pending = self.pending.take()?;
        (pending == expected && Some(pending.mutation_id) == current).then_some(pending)
    }
}

fn last_pull_text(at: Option<DateTime<Utc>>) -> String {
    at.map_or_else(
        || "Not synced yet".into(),
        |at| {
            format!(
                "Last synced {}",
                at.with_timezone(&Local).format("%b %-d, %H:%M")
            )
        },
    )
}

fn direction_label(direction: &SyncDirection) -> &'static str {
    match direction {
        SyncDirection::Incoming => "In",
        SyncDirection::Outgoing => "Out",
        SyncDirection::Status => "Status",
    }
}

fn activity_text(activity: &SyncActivity) -> String {
    format!(
        "{} · {} · {}",
        activity.at.with_timezone(&Local).format("%H:%M:%S"),
        direction_label(&activity.direction),
        activity.summary,
    )
}

fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

fn sync_summary(
    status: &StoreStatus,
    sync: &SyncStatus,
    signed_in: bool,
    pending: usize,
    blocked: usize,
) -> Cow<'static, str> {
    match status {
        StoreStatus::Error(_) => "Sync error".into(),
        StoreStatus::NotConfigured => "Sync unavailable".into(),
        StoreStatus::AuthenticationRequired => "Sign in to sync".into(),
        StoreStatus::Ready if !signed_in => "Not connected".into(),
        StoreStatus::Ready => match sync {
            SyncStatus::AuthenticationRequired => "Sign in to sync".into(),
            SyncStatus::Offline => "Offline".into(),
            SyncStatus::Syncing => "Syncing…".into(),
            SyncStatus::Idle if blocked > 0 => {
                format!("{} to review", plural(blocked, "change", "changes")).into()
            }
            SyncStatus::Idle if pending > 0 => {
                format!("{} waiting to sync", plural(pending, "change", "changes")).into()
            }
            SyncStatus::Idle => "Up to date".into(),
        },
    }
}

fn operation_label(operation: &str) -> &str {
    match operation {
        "upsert_action" => "saving an action",
        "upsert_actions" => "saving actions",
        "upsert_resources" => "saving items",

        "set_event_busy_override" => "changing event availability",
        "delete_resources" => "deleting items",
        "reorder_routines" => "reordering routines",
        "complete_action" => "completing an action",
        "delete_action" => "deleting an action",
        "convert_event_to_marker" => "converting an event to a marker",
        _ => "updating an item",
    }
}

fn resolution_unavailable_reason(status: &StoreStatus, sync: &SyncStatus) -> Option<&'static str> {
    if !matches!(status, StoreStatus::Ready) {
        Some("Available once your data has loaded.")
    } else if matches!(sync, SyncStatus::Syncing) {
        Some("Available when the current sync finishes.")
    } else {
        None
    }
}

fn rejection_explanation(code: ApiErrorCode) -> &'static str {
    match code {
        ApiErrorCode::MissingToken | ApiErrorCode::InvalidToken => {
            "Your session expired. Sign in again, then retry."
        }
        ApiErrorCode::InsufficientScope => "Your account doesn’t have permission for this change.",
        ApiErrorCode::WrongTenant => "This change belongs to a different account.",
        ApiErrorCode::AuthenticationUnavailable => {
            "The server couldn’t verify your session. Try again later."
        }
        ApiErrorCode::UnsupportedProtocol => {
            "Update Subroutine Lite or the server to compatible versions."
        }
        ApiErrorCode::ValidationFailed => "The server rejected this change as invalid.",
        ApiErrorCode::ResourceNotFound => "The item this change refers to no longer exists.",
        ApiErrorCode::DatasetMismatch => "Your account’s data was replaced on the server.",
        ApiErrorCode::StaleBase => "This item changed on another device.",
        ApiErrorCode::MutationIdReuse => "The server rejected a duplicate request.",
        ApiErrorCode::DomainConflict => "This change conflicts with your current data.",

        ApiErrorCode::TransientFailure => "A temporary server problem blocked this change.",
        ApiErrorCode::InternalFailure => "A server error blocked this change.",
    }
}

fn text_node(
    id: impl Into<gpui::SharedString>,
    text: impl Into<gpui::SharedString>,
    cx: &App,
) -> impl IntoElement {
    let text = text.into();
    div()
        .w_full()
        .min_w_0()
        .child(text.clone())
        .semantic_in(cx, NodeSpec::new(id, Role::Text).text(text))
}

#[derive(Clone, Copy)]
struct ConflictDetails {
    mutation_id: Uuid,
    operation: &'static str,
    error: ApiErrorCode,
}

struct PanelData<'a> {
    status: StoreStatus,
    sync: SyncStatus,
    signed_in: bool,
    last_pull: Option<DateTime<Utc>>,
    pending: usize,
    blocked: usize,
    unavailable: Option<&'static str>,
    conflict: Option<ConflictDetails>,
    activity: Cow<'a, [SyncActivity]>,
}

impl<'a> PanelData<'a> {
    fn from_store(store: &'a AppDatabaseStore, cx: &App) -> Self {
        Self {
            status: store.status(),
            sync: store.sync_status(),
            signed_in: AuthSession::global(cx).is_signed_in(),
            last_pull: store.last_successful_sync(),
            pending: store.pending_count(),
            blocked: store.blocked_count(),
            unavailable: store.sync_unavailable_reason(cx),
            conflict: store.blocked_conflict().map(|conflict| ConflictDetails {
                mutation_id: conflict.mutation_id,
                operation: conflict.operation.name(),
                error: conflict.error.error,
            }),
            activity: Cow::Borrowed(store.sync_activity()),
        }
    }
}

#[derive(Clone, Copy)]
enum PanelAction {
    SyncNow,
    ToggleActivity,
    RequestResolution(ResolutionRequest),
    CancelResolution,
    ConfirmResolution(ResolutionRequest),
}

#[derive(Debug, PartialEq, Eq)]
enum PanelCommand {
    SyncNow,
    Resolve(ResolutionRequest),
}

type PanelHandler = Rc<dyn Fn(&PanelAction, &mut Window, &mut App)>;

pub(super) struct SyncStatusView {
    store: Entity<AppDatabaseStore>,
    panel: PanelState,
}

struct PanelState {
    confirmation: Confirmation,
    feedback: Option<&'static str>,
    show_activity: bool,
    panel_focus: FocusHandle,
    activity_focus: FocusHandle,
    retry_focus: FocusHandle,
    server_focus: FocusHandle,
    cancel_focus: FocusHandle,
}

impl SyncStatusView {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        let store = AppDatabaseStore::global(cx);
        cx.observe(&store, |view, store, cx| {
            let current = store
                .read(cx)
                .blocked_conflict()
                .map(|conflict| conflict.mutation_id);
            if view.panel.confirmation.invalidate_if_stale(current) {
                view.panel.feedback = Some(STALE_CONFIRMATION);
            }
            cx.notify();
        })
        .detach();
        let auth_changes = AuthSession::global(cx).subscribe();
        cx.spawn(async move |view, cx| {
            while auth_changes.recv_async().await.is_ok() {
                if view
                    .update(cx, |view, cx| {
                        view.panel.confirmation.cancel();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        Self {
            store,
            panel: PanelState::new(cx),
        }
    }

    pub(super) fn settings_row(panel: Entity<Self>) -> SettingsRow {
        SettingsRow::new("settings.account.sync", "Sync")
            .search_terms([
                "sync now",
                "synchronization",
                "last synced",
                "pending",
                "conflict",
                "keep my version",
                "use server version",
                "activity",
            ])
            .stacked()
            .control(panel)
    }

    fn on_panel_action(
        &mut self,
        action: &PanelAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.store.clone().update(cx, |store, cx| {
            let data = PanelData::from_store(store, cx);
            match self.panel.act(action, &data, window, cx) {
                Some(PanelCommand::SyncNow) => {
                    if let Err(reason) = store.sync_now(cx) {
                        self.panel.feedback = Some(reason);
                    }
                }
                Some(PanelCommand::Resolve(request)) => match request.resolution {
                    Resolution::RetryLocal => store.keep_local_conflict(request.mutation_id, cx),
                    Resolution::UseServer => store.accept_server_conflict(request.mutation_id, cx),
                },
                None => {}
            }
        });
        cx.notify();
    }
}

impl Render for SyncStatusView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let data = PanelData::from_store(self.store.read(cx), cx);
        self.panel
            .render(&data, Rc::new(cx.listener(Self::on_panel_action)), cx)
    }
}

impl PanelState {
    fn new(cx: &mut App) -> Self {
        Self {
            confirmation: Confirmation::default(),
            feedback: None,
            show_activity: false,
            panel_focus: cx.focus_handle(),
            activity_focus: cx.focus_handle(),
            retry_focus: cx.focus_handle(),
            server_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
        }
    }

    fn act(
        &mut self,
        action: &PanelAction,
        data: &PanelData<'_>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<PanelCommand> {
        let current = data.conflict.map(|conflict| conflict.mutation_id);
        let unavailable = resolution_unavailable_reason(&data.status, &data.sync);
        match *action {
            PanelAction::SyncNow => {
                if let Some(reason) = data.unavailable {
                    self.feedback = Some(reason);
                } else {
                    self.feedback = None;
                    return Some(PanelCommand::SyncNow);
                }
            }
            PanelAction::ToggleActivity => self.show_activity = !self.show_activity,
            PanelAction::RequestResolution(request) => {
                if current != Some(request.mutation_id) {
                    self.confirmation.cancel();
                    self.feedback = Some(STALE_CONFIRMATION);
                } else if let Some(reason) = unavailable {
                    self.feedback = Some(reason);
                } else {
                    self.confirmation.request(request);
                    self.feedback = None;
                    window.focus(&self.cancel_focus, cx);
                }
            }
            PanelAction::CancelResolution => {
                if let Some(request) = self.confirmation.cancel() {
                    self.restore_resolution_focus(request.resolution, window, cx);
                }
            }
            PanelAction::ConfirmResolution(expected) => {
                window.focus(&self.panel_focus, cx);
                let Some(request) = self.confirmation.confirm(expected, current) else {
                    self.feedback = Some(STALE_CONFIRMATION);
                    return None;
                };
                if let Some(reason) = unavailable {
                    self.feedback = Some(reason);
                } else {
                    self.feedback = None;
                    return Some(PanelCommand::Resolve(request));
                }
            }
        }
        None
    }

    fn restore_resolution_focus(&self, resolution: Resolution, window: &mut Window, cx: &mut App) {
        window.focus(
            match resolution {
                Resolution::RetryLocal => &self.retry_focus,
                Resolution::UseServer => &self.server_focus,
            },
            cx,
        );
    }

    fn render(
        &mut self,
        data: &PanelData<'_>,
        on_action: PanelHandler,
        cx: &App,
    ) -> gpui::AnyElement {
        let theme = cx.theme().clone();
        let pending = data.pending;
        let blocked = data.blocked;
        let summary = sync_summary(&data.status, &data.sync, data.signed_in, pending, blocked);
        let unavailable = data.unavailable;
        let resolution_unavailable = resolution_unavailable_reason(&data.status, &data.sync);
        let current = data.conflict.map(|conflict| conflict.mutation_id);
        if self.confirmation.invalidate_if_stale(current) {
            self.feedback = Some(STALE_CONFIRMATION);
        }

        let sync_now = unavailable.is_none().then(|| {
            let on_action = on_action.clone();
            Button::new("settings.sync.now")
                .label("Sync Now")
                .icon(Icon::Refresh)
                .secondary()
                .on_click(move |window, cx| on_action(&PanelAction::SyncNow, window, cx))
        });

        let muted = theme.colors.text_muted;
        let mut detail = summary.to_string();
        if data.last_pull.is_some() {
            detail.push_str(&format!("\n{}", last_pull_text(data.last_pull)));
        }
        if let Some(reason) = unavailable.filter(|_| !matches!(data.sync, SyncStatus::Syncing)) {
            detail.push_str(&format!("\n{reason}"));
        }
        let needs_attention = pending > 0
            || blocked > 0
            || !matches!(data.status, StoreStatus::Ready)
            || matches!(
                data.sync,
                SyncStatus::Offline | SyncStatus::AuthenticationRequired
            );
        let (glyph, tint) = match (&data.status, &data.sync) {
            (StoreStatus::Error(_), _) => (Icon::Danger, theme.colors.danger),
            (_, _) if blocked > 0 => (Icon::Danger, theme.colors.warning),
            (_, SyncStatus::Syncing) => (Icon::Refresh, theme.colors.accent),
            (StoreStatus::Ready, SyncStatus::Idle) if data.signed_in && pending == 0 => {
                (Icon::CheckCircle, theme.colors.success)
            }
            _ => (Icon::Refresh, muted),
        };
        let mut panel = div()
            .column()
            .w_full()
            .min_w_0()
            .text_sm()
            .gap_3()
            .semantic_in(
                cx,
                NodeSpec::new("settings.sync.panel", Role::Region)
                    .text("Account sync")
                    .focus(&self.panel_focus),
            )
            .track_focus(&self.panel_focus)
            .child(
                div()
                    .row()
                    .flex_wrap()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .id(SYNC_STATUS_ID)
                            .row()
                            .items_center()
                            .gap_2()
                            .p_1()
                            .child(icon(glyph).size(px(16.)).text_color(tint))
                            .when(needs_attention, |row| row.child(summary.to_string()))
                            .semantic_in(
                                cx,
                                NodeSpec::new(SYNC_STATUS_ID, Role::Status).text(detail.clone()),
                            )
                            .tooltip(move |_, cx| {
                                Tooltip::new("settings.sync.details", detail.clone()).view(cx)
                            }),
                    )
                    .children(sync_now),
            );
        if let Some(feedback) = self.feedback {
            panel = panel.child(div().child(feedback).text_sm().semantic_in(
                cx,
                NodeSpec::new("settings.sync.feedback", Role::Status).text(feedback),
            ));
        }

        if let Some(conflict) = data.conflict {
            let id = conflict.mutation_id;
            let prefix = format!("settings.sync.conflict.{id}");
            let mut rejection = div()
                .column()
                .w_full()
                .min_w_0()
                .gap_2()
                .p_3()
                .border_1()
                .border_color(theme.colors.danger)
                .rounded(px(theme.radii.control))
                .semantic_in(
                    cx,
                    NodeSpec::new(prefix.clone(), Role::Group).text("A change couldn’t sync"),
                )
                .child(div().font_weight(FontWeight::SEMIBOLD).child(text_node(
                    format!("{prefix}.operation"),
                    format!("Couldn’t finish {}", operation_label(conflict.operation)),
                    cx,
                )))
                .child(text_node(
                    format!("{prefix}.reason"),
                    rejection_explanation(conflict.error),
                    cx,
                ));
            let mut actions = div().row().flex_wrap().gap_2();
            for (resolution, suffix, focus) in [
                (Resolution::RetryLocal, "retry", &self.retry_focus),
                (Resolution::UseServer, "server", &self.server_focus),
            ] {
                let request = ResolutionRequest {
                    mutation_id: id,
                    resolution,
                };
                let mut button = Button::new(format!("{prefix}.{suffix}"))
                    .label(resolution.label())
                    .large()
                    .secondary()
                    .track_focus(focus)
                    .accessible_description(resolution.warning())
                    .disabled(
                        resolution_unavailable.is_some() || self.confirmation.pending.is_some(),
                    );
                if resolution_unavailable.is_none() && self.confirmation.pending.is_none() {
                    let on_action = on_action.clone();
                    button = button.on_click(move |window, cx| {
                        on_action(&PanelAction::RequestResolution(request), window, cx);
                    });
                }
                actions = actions.child(button);
            }
            rejection = rejection.child(actions);
            if let Some(reason) = resolution_unavailable {
                rejection = rejection.child(div().text_color(muted).child(text_node(
                    format!("{prefix}.unavailable"),
                    reason,
                    cx,
                )));
            }
            panel = panel.child(rejection);
        } else if blocked > 0 {
            panel = panel.child(text_node(
                "settings.sync.no-conflict",
                "Details for the blocked change aren’t available yet.",
                cx,
            ));
        }

        if let Some(request) = self.confirmation.pending {
            let prefix = format!("settings.sync.confirm.{}", request.mutation_id);
            let on_cancel = on_action.clone();
            let cancel = Button::new(format!("{prefix}.cancel"))
                .label("Cancel")
                .large()
                .secondary()
                .track_focus(&self.cancel_focus)
                .on_click(move |window, cx| on_cancel(&PanelAction::CancelResolution, window, cx));
            let mut confirm = Button::new(format!("{prefix}.confirm"))
                .label(request.resolution.label())
                .accessible_description(request.resolution.warning())
                .large()
                .danger()
                .disabled(resolution_unavailable.is_some());
            if resolution_unavailable.is_none() {
                let on_action = on_action.clone();
                confirm = confirm.on_click(move |window, cx| {
                    on_action(&PanelAction::ConfirmResolution(request), window, cx);
                });
            }
            panel = panel.child(
                div()
                    .column()
                    .w_full()
                    .min_w_0()
                    .gap_3()
                    .p_3()
                    .border_1()
                    .border_color(theme.colors.danger)
                    .rounded(px(theme.radii.control))
                    .semantic_in(
                        cx,
                        NodeSpec::new(prefix.clone(), Role::Group).text("Confirm sync resolution"),
                    )
                    .child(text_node(
                        format!("{prefix}.warning"),
                        request.resolution.warning(),
                        cx,
                    ))
                    .child(div().row().flex_wrap().gap_2().child(cancel).child(confirm)),
            );
        }

        let show_activity = self.show_activity;
        let toggle_activity = {
            let on_action = on_action.clone();
            Button::new("settings.sync.activity.toggle")
                .label("Recent activity")
                .icon(if show_activity {
                    Icon::AltArrowDown
                } else {
                    Icon::AltArrowRight
                })
                .ghost()
                .small()
                .on_click(move |window, cx| on_action(&PanelAction::ToggleActivity, window, cx))
        };
        if show_activity || !data.activity.is_empty() {
            panel = panel.child(div().row().child(toggle_activity));
        }
        if !show_activity {
            return panel.into_any_element();
        }

        let mut log = div().column().w_full().min_w_0().text_xs().gap_2().p_3();
        if data.activity.is_empty() {
            log = log.child(div().text_color(muted).child(text_node(
                "settings.sync.activity.empty",
                "No recent activity",
                cx,
            )));
        } else {
            for activity in data.activity.iter().rev() {
                log = log.child(text_node(
                    format!("settings.sync.activity.{}", activity.id),
                    activity_text(activity),
                    cx,
                ));
            }
        }
        panel
            .child(
                div()
                    .semantic_in(
                        cx,
                        NodeSpec::new("settings.sync.activity.keyboard", Role::Region)
                            .text("Sync activity log")
                            .description("Scroll with Up/Down or Page Up/Page Down")
                            .focus(&self.activity_focus),
                    )
                    .track_focus(&self.activity_focus)
                    .tab_stop(true)
                    .focus_ring(&theme)
                    .on_key_down(|event: &gpui::KeyDownEvent, window, cx| {
                        let delta = match event.keystroke.key.as_str() {
                            "up" => -32.0,
                            "down" => 32.0,
                            "pageup" => -ACTIVITY_HEIGHT,
                            "pagedown" => ACTIVITY_HEIGHT,
                            _ => return,
                        };
                        let mut offset = scroll_offset(ACTIVITY_SCROLL_ID, window, cx);
                        offset.y = (offset.y + px(delta)).max(px(0.0));
                        scroll_to(ACTIVITY_SCROLL_ID, offset, window, cx);
                        cx.stop_propagation();
                        window.refresh();
                    })
                    .child(
                        ScrollArea::new(ACTIVITY_SCROLL_ID)
                            .label("Sync activity, newest first")
                            .vertical()
                            .height(ACTIVITY_HEIGHT)
                            .child(log),
                    ),
            )
            .into_any_element()
    }
}
