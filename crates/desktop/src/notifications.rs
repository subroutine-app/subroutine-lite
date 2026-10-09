use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Local};
use gpui::{App, Global, SystemNotification, SystemNotificationAction, SystemNotificationResponse};
use subroutine_core::{Action, AnyItem};
use uuid::Uuid;

#[cfg(target_os = "macos")]
use notify_rust::Notification as FallbackNotification;
#[cfg(target_os = "macos")]
use objc2_foundation::{NSBundle, NSString};

use crate::{
    auth::{AuthSession, RequestScope},
    branding::APP_ID,
    settings::{NotificationPreview, Settings},
    stores::AppDatabaseStore,
};

const APP_NAME: &str = "Subroutine Lite";
const UPCOMING_TAG_PREFIX: &str = "upcoming-action:";
const TEST_TAG: &str = "notification-test";
const COMPLETE_ACTION: &str = "complete";
const UNQUEUE_ACTION: &str = "unqueue";
const OPEN_ACTION: &str = "open";

#[cfg(target_os = "macos")]
const SYSTEM_NOTIFICATION_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.Notifications-Settings.extension";
#[cfg(target_os = "windows")]
const SYSTEM_NOTIFICATION_SETTINGS_URL: &str = "ms-settings:notifications";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NotificationScope {
    workspace: u64,
    auth: Option<RequestScope>,
}

impl NotificationScope {
    fn current(store: &AppDatabaseStore, cx: &App) -> Option<Self> {
        let auth = AuthSession::global(cx).scope();
        store.workspace_is_current(auth).then_some(Self {
            workspace: store.workspace_generation(),
            auth,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct NotificationTarget {
    scope: NotificationScope,
    action: Uuid,
}

#[derive(Clone, Copy)]
enum NotificationState {
    Pending(NotificationTarget),
    Handled(NotificationTarget),
}

impl NotificationState {
    fn target(&self) -> NotificationTarget {
        match self {
            Self::Pending(target) | Self::Handled(target) => *target,
        }
    }
}

#[derive(Default)]
struct UpcomingNotifications(HashMap<Uuid, NotificationState>);

impl Global for UpcomingNotifications {}

impl UpcomingNotifications {
    fn insert(&mut self, target: NotificationTarget) -> Option<Uuid> {
        if self.0.values().any(|existing| existing.target() == target) {
            return None;
        }
        let id = Uuid::new_v4();
        self.0.insert(id, NotificationState::Pending(target));
        Some(id)
    }

    fn respond(&mut self, id: Uuid, scope: Option<NotificationScope>) -> Option<Uuid> {
        let state = self.0.get_mut(&id)?;
        let NotificationState::Pending(target) = *state else {
            return None;
        };
        if Some(target.scope) != scope {
            return None;
        }
        *state = NotificationState::Handled(target);
        Some(target.action)
    }

    fn retain(&mut self, mut keep: impl FnMut(&NotificationTarget) -> bool) -> Vec<Uuid> {
        let mut retired = Vec::new();
        self.0.retain(|id, state| {
            if keep(&state.target()) {
                true
            } else {
                retired.push(*id);
                false
            }
        });
        retired
    }
}

pub fn init(cx: &mut App) {
    cx.set_global(UpcomingNotifications::default());
    cx.set_app_identity(APP_ID, APP_NAME);

    if !gpui_notifications_available() {
        #[cfg(target_os = "macos")]
        {
            let _ = notify_rust::set_application(APP_ID);
            tracing::info!(
                "using generic, non-actionable notification fallback outside a macOS app bundle"
            );
        }
        return;
    }

    cx.on_system_notification_response(handle_response);
}

pub(crate) fn update_upcoming(deliver: bool, cx: &mut App) {
    let settings = Settings::global(cx);
    let store = AppDatabaseStore::global(cx);
    let Some(scope) =
        NotificationScope::current(store.read(cx), cx).filter(|_| settings.notifications.enabled)
    else {
        retire_upcoming(cx);
        return;
    };
    let queue = store
        .read(cx)
        .pipeline(&settings)
        .queue()
        .into_iter()
        .cloned()
        .map(AnyItem::Action)
        .collect::<Vec<_>>();
    let queued_ids = queue.iter().map(AnyItem::id).collect::<HashSet<_>>();
    let retired = cx
        .global_mut::<UpcomingNotifications>()
        .retain(|target| target.scope == scope && queued_ids.contains(&target.action));
    dismiss(retired, cx);
    if !deliver {
        return;
    }
    let now = Local::now();
    for item in queue {
        let Some(starts_at) = item
            .start_datetime()
            .filter(|time| *time > now && *time < now + settings.notifications.lead_time)
        else {
            continue;
        };
        if let AnyItem::Action(action) = item {
            let target = NotificationTarget {
                scope,
                action: action.id,
            };
            if !action.is_completed()
                && let Some(id) = cx.global_mut::<UpcomingNotifications>().insert(target)
            {
                show_notification(
                    upcoming_notification(id, &action, starts_at, settings.notifications.preview),
                    cx,
                );
            }
        }
    }
}

pub(crate) fn retire_upcoming(cx: &mut App) {
    let retired = cx.global_mut::<UpcomingNotifications>().retain(|_| false);
    dismiss(retired, cx);
}

fn dismiss(ids: Vec<Uuid>, cx: &App) {
    if gpui_notifications_available() {
        for id in ids {
            cx.dismiss_system_notification(&upcoming_tag(id));
        }
    }
}

pub fn show_test_notification(cx: &App) {
    show_notification(test_notification(), cx);
}

pub const fn system_notification_settings_supported() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows"))
}

pub fn open_system_notification_settings(cx: &App) {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    cx.open_url(SYSTEM_NOTIFICATION_SETTINGS_URL);

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = cx;
}

fn show_notification(notification: SystemNotification, cx: &App) {
    #[cfg(target_os = "macos")]
    if !gpui_notifications_available() {
        show_fallback(notification);
        return;
    }

    cx.show_system_notification(notification);
}

#[cfg(target_os = "macos")]
fn fallback_notification(mut notification: SystemNotification) -> SystemNotification {
    if notification.tag.as_ref() != TEST_TAG {
        notification.title = "Subroutine reminder".into();
        notification.body = "Open Subroutine Lite to review your reminders.".into();
    }
    notification.actions.clear();
    notification
}

#[cfg(target_os = "macos")]
fn show_fallback(notification: SystemNotification) {
    let notification = fallback_notification(notification);
    std::thread::spawn(move || {
        if let Err(error) = FallbackNotification::new()
            .summary(notification.title.as_ref())
            .body(notification.body.as_ref())
            .show()
        {
            tracing::warn!(%error, "failed to deliver fallback system notification");
        }
    });
}

#[cfg(target_os = "macos")]
fn gpui_notifications_available() -> bool {
    let bundle = NSBundle::mainBundle();
    let identifier = bundle.bundleIdentifier().map(|value| value.to_string());
    let package_type =
        bundle.objectForInfoDictionaryKey(&NSString::from_str("CFBundlePackageType"));
    let package_type = package_type
        .as_deref()
        .and_then(|value| value.downcast_ref::<NSString>())
        .map(|value| value.to_string());
    let bundle_path = bundle.bundlePath().to_string();

    is_macos_app_bundle(
        identifier.as_deref(),
        package_type.as_deref(),
        std::path::Path::new(&bundle_path),
    )
}

#[cfg(target_os = "macos")]
fn is_macos_app_bundle(
    identifier: Option<&str>,
    package_type: Option<&str>,
    bundle_path: &std::path::Path,
) -> bool {
    identifier == Some(APP_ID)
        && package_type == Some("APPL")
        && bundle_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
        && bundle_path.join("Contents/Info.plist").is_file()
        && bundle_path.join("Contents/MacOS").is_dir()
}

#[cfg(not(target_os = "macos"))]
fn gpui_notifications_available() -> bool {
    true
}

fn handle_response(response: SystemNotificationResponse, cx: &mut App) {
    if response.tag.as_ref() == TEST_TAG {
        cx.dismiss_system_notification(TEST_TAG);
        cx.activate(true);
        return;
    }

    let Some(id) = notification_id_from_tag(response.tag.as_ref()) else {
        return;
    };

    cx.dismiss_system_notification(response.tag.as_ref());

    let store = AppDatabaseStore::global(cx);
    store.update(cx, |store, cx| {
        let scope = NotificationScope::current(store, cx);
        let Some(action_id) = cx.global_mut::<UpcomingNotifications>().respond(id, scope) else {
            return;
        };
        let Some(operation) = response.action_id.as_deref() else {
            cx.activate(true);
            return;
        };
        let current = store.actions().iter().find(|action| action.id == action_id);
        match operation {
            COMPLETE_ACTION if current.is_some_and(|action| !action.is_completed()) => {
                let _ = store.complete_action(action_id, cx);
            }
            UNQUEUE_ACTION if current.is_some_and(|action| action.queued) => {
                let _ = store.backlog_action(action_id, cx);
            }
            COMPLETE_ACTION | UNQUEUE_ACTION => {}
            unknown => tracing::warn!(
                action = unknown,
                notification = %response.tag,
                "ignored unknown system notification action"
            ),
        }
    });
}

fn upcoming_notification(
    id: Uuid,
    action: &Action,
    starts_at: DateTime<Local>,
    preview: NotificationPreview,
) -> SystemNotification {
    let mut actions = vec![SystemNotificationAction {
        id: COMPLETE_ACTION.into(),
        label: "Complete".into(),
    }];
    if action.queued {
        actions.push(SystemNotificationAction {
            id: UNQUEUE_ACTION.into(),
            label: "Unqueue".into(),
        });
    }

    let title = match preview {
        NotificationPreview::Generic => "Upcoming action".into(),
        NotificationPreview::ActionTitle if !action.title.trim().is_empty() => {
            action.title.clone().into()
        }
        NotificationPreview::ActionTitle => "Upcoming action".into(),
    };

    SystemNotification {
        tag: upcoming_tag(id).into(),
        title,
        body: format!("Starts at {}", format_time(starts_at)).into(),
        actions,
    }
}

fn test_notification() -> SystemNotification {
    SystemNotification {
        tag: TEST_TAG.into(),
        title: "Test notification".into(),
        body: "Notifications are working.".into(),
        actions: vec![SystemNotificationAction {
            id: OPEN_ACTION.into(),
            label: "Open Subroutine Lite".into(),
        }],
    }
}

fn upcoming_tag(id: Uuid) -> String {
    format!("{UPCOMING_TAG_PREFIX}{id}")
}

fn notification_id_from_tag(tag: &str) -> Option<Uuid> {
    tag.strip_prefix(UPCOMING_TAG_PREFIX)?.parse().ok()
}

fn format_time(time: DateTime<Local>) -> String {
    time.format("%-I:%M%P").to_string().replace(":00", "")
}
