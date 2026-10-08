use super::{OpenSavedItemInspector, RootView};
use crate::{
    auth::AuthSession,
    components::timed_toast,
    selection::SelectionManager,
    settings::Settings,
    stores::{AppDatabaseStore, SyncStatus},
};
use chrono::{DateTime, Utc};
use gpui::{Context, Window};
use gpui_kit::{display::badge::Tone, overlay::toast};

pub(super) struct ManualSync {
    previous_success: Option<DateTime<Utc>>,
}

struct SyncObservation {
    ready: bool,
    signed_in: bool,
    busy: bool,
    status: SyncStatus,
    confirmed_pull: bool,
    pending: usize,
    blocked: usize,
}

impl SyncObservation {
    fn feedback(&self) -> Option<(&'static str, Tone)> {
        if !self.signed_in || self.status == SyncStatus::AuthenticationRequired {
            Some(("Sign in to sync.", Tone::Warning))
        } else if !self.ready {
            Some(("Couldn’t sync. Your data isn’t available.", Tone::Danger))
        } else if self.busy {
            None
        } else if self.status == SyncStatus::Offline {
            Some(("Couldn’t sync while offline.", Tone::Warning))
        } else if self.blocked > 0 {
            Some((
                "Some changes need review in Account settings.",
                Tone::Warning,
            ))
        } else if self.pending > 0 {
            Some(("Some changes are still waiting to sync.", Tone::Warning))
        } else if self.confirmed_pull && self.status == SyncStatus::Idle {
            Some(("Sync complete.", Tone::Success))
        } else {
            Some(("Sync didn’t finish. Try again.", Tone::Warning))
        }
    }
}

impl RootView {
    pub(super) fn sync_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let store = AppDatabaseStore::global(cx);
        let previous_success = store.read(cx).last_successful_sync();
        match store.update(cx, |store, cx| store.sync_now(cx)) {
            Ok(()) => {
                self.manual_sync = Some(ManualSync { previous_success });

                self.update_sync_feedback(window, cx);
            }
            Err(reason) => {
                toast::push(
                    window,
                    cx,
                    timed_toast("command.sync", reason).tone(Tone::Warning),
                );
            }
        }
    }

    pub(super) fn update_sync_feedback(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(attempt) = &self.manual_sync else {
            return;
        };
        let store = AppDatabaseStore::global(cx);
        let store = store.read(cx);
        let observation = SyncObservation {
            ready: store.is_ready(),
            signed_in: AuthSession::global(cx).is_signed_in(),
            busy: store.sync_busy(),
            status: store.sync_status(),
            confirmed_pull: store.last_successful_sync().is_some()
                && store.last_successful_sync() != attempt.previous_success,
            pending: store.pending_count(),
            blocked: store.blocked_count(),
        };
        if let Some((message, tone)) = observation.feedback() {
            self.manual_sync = None;
            toast::push(window, cx, timed_toast("command.sync", message).tone(tone));
        }
    }

    pub(super) fn open_configuration_folder(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = Settings::open_configuration_folder() {
            tracing::warn!(%error, "could not open configuration folder");
            toast::push(
                window,
                cx,
                timed_toast(
                    "command.configuration-folder",
                    "Couldn’t open the configuration folder.",
                )
                .tone(Tone::Warning),
            );
        }
    }

    pub(super) fn edit_selected_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let items = SelectionManager::selected_items(cx);
        let [item] = items.as_slice() else {
            toast::push(
                window,
                cx,
                timed_toast("command.edit-selection", "Select exactly one item to edit.")
                    .tone(Tone::Warning),
            );
            return;
        };
        if item.is_template() {
            self.open_saved_item_inspector(&OpenSavedItemInspector(item.id()), window, cx);
        } else {
            let scope = SelectionManager::global(cx).read(cx).scope();
            self.open_inspected_item(item.clone(), scope, window, cx);
        }
    }
}
