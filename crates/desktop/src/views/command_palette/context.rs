
use gpui::App;
use subroutine_core::AnyItem;

use crate::{
    auth::AuthSession,
    keys::{ConfigurableCommand, PaletteCommandState},
    selection::{self, SelectionManager},
    settings::Settings,
    stores::AppDatabaseStore,
};

#[derive(Default)]
struct SelectionCounts {
    total: usize,
    incomplete: usize,
    queued: usize,
    unqueued: usize,
    pinned: usize,
    unpinned: usize,
}

impl SelectionCounts {
    fn from_items(items: &[AnyItem]) -> Self {
        let mut counts = Self {
            total: items.len(),
            ..Self::default()
        };
        for item in items {
            if let AnyItem::Action(action) = item {
                if action.is_completed() {
                    continue;
                }
                counts.incomplete += 1;
                if action.queued {
                    counts.queued += 1;
                } else {
                    counts.unqueued += 1;
                }
                if action.start.is_some() {
                    if action.pinned {
                        counts.pinned += 1;
                    } else {
                        counts.unpinned += 1;
                    }
                }
            }
        }
        counts
    }

    fn state(&self, command: ConfigurableCommand) -> PaletteCommandState {
        use ConfigurableCommand::*;
        let (count, requirement) = match command {
            EditSelectedItem => (self.total, "Select exactly one item to edit."),
            CompleteSelected => (self.incomplete, "Select an incomplete action to complete."),
            ToggleQueuedSelected => (
                if self.unqueued > 0 {
                    self.unqueued
                } else {
                    self.queued
                },
                "Select an incomplete action to queue or unqueue.",
            ),
            TogglePinnedSelected => (
                if self.unpinned > 0 {
                    self.unpinned
                } else {
                    self.pinned
                },
                "Select a scheduled, incomplete action to pin or unpin.",
            ),
            DeleteSelected | CopySelected | CutSelected | DuplicateSelected => {
                (self.total, "Select an item first.")
            }
            _ => return PaletteCommandState::default(),
        };
        PaletteCommandState {
            selection_count: count,
            unavailable: if command == EditSelectedItem {
                (count != 1).then_some(requirement)
            } else {
                (count == 0).then_some(requirement)
            },
        }
    }
}

pub(super) fn command_visible(command: ConfigurableCommand, cx: &App) -> bool {
    match command {
        ConfigurableCommand::SyncNow => {
            let auth = AuthSession::global(cx);
            auth.is_interactive_sign_in_configured()
                || auth.is_signed_in()
                || AppDatabaseStore::global(cx).read(cx).has_remote_workspace()
        }
        _ => true,
    }
}

pub(super) fn command_state(command: ConfigurableCommand, cx: &App) -> PaletteCommandState {
    let items = SelectionManager::selected_items(cx);
    state_for(command, &items, cx)
}

pub(super) fn command_states(cx: &App) -> Vec<(ConfigurableCommand, PaletteCommandState)> {
    let items = SelectionManager::selected_items(cx);
    ConfigurableCommand::ALL
        .into_iter()
        .map(|command| (command, state_for(command, &items, cx)))
        .collect()
}

fn state_for(command: ConfigurableCommand, items: &[AnyItem], cx: &App) -> PaletteCommandState {
    let mut state = SelectionCounts::from_items(items).state(command);
    let store = AppDatabaseStore::global(cx);
    let store = store.read(cx);
    use ConfigurableCommand::*;
    state.unavailable = match command {
        Undo => (!store.can_undo()).then_some("There is nothing to undo."),
        Redo => (!store.can_redo()).then_some("There is nothing to redo."),
        PasteItems => (!selection::can_paste_items(cx)).then_some("Copy or cut an item first."),
        SelectAllItems => (!selection::can_select_all_items(cx)).then_some("Open a list first."),
        SyncNow => store.sync_unavailable_reason(cx),
        OpenConfigurationFolder => Settings::configuration_folder()
            .is_none()
            .then_some("Configuration folder not found."),
        _ => state.unavailable,
    };
    state
}
