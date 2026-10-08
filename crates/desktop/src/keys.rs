use std::collections::{BTreeMap, BTreeSet};

use gpui::{Action, App, KeyBinding, Keystroke, Modifiers, SharedString, Unbind};
use gpui_kit::{
    controls::keymap_editor::{KeymapBinding, KeymapCommand},
    overlay::Command,
};

use crate::{
    app::{ShowAccountSettings, ShowSettings, ToggleSearch},
    item_manager::{DraftAsAction, DraftAsEvent, OpenDraftTypeMenu},
    selection::{
        CompleteSelected, CopySelected, CutSelected, DeleteSelected, DuplicateSelected,
        KEY_CONTEXT as ITEMS_KEY_CONTEXT, PasteItems, SelectAllItems, TogglePinnedSelected,
        ToggleQueuedSelected,
    },
    themes::ToggleThemeMode,
    views::{
        ChooseTheme, EditSelectedItem, GoToCalendar, GoToFocus, GoToHome, GoToNow, GoToQueue,
        GoToRoutines, GoToSavedItems, GoToTimeline, GoToUnqueued, MAIN_VIEW_KEY_CONTEXT, NextTab,
        OpenConfigurationFolder, PreviousTab, Redo, RefreshPipeline, StartEventCreator,
        StartItemCreator, StartMarkerCreator, StartRoutine, StartRoutineCreator,
        StartSignalCreator, SyncNow, ToggleLeftSidebar, ToggleRightSidebar, Undo, UseSavedAction,
        UseSavedEvent,
    },
};

pub const CMD: &str = if cfg!(target_os = "macos") {
    "cmd"
} else {
    "ctrl"
};

pub const CONTEXT_MENU_KEYS: [&str; 2] = ["menu", "shift-f10"];
pub const VIEW_SWITCH_KEY_CONTEXT: &str = "Items && !ContextMenu";

pub fn key<A: Action>(chord: &str, action: A, context: Option<&str>) -> KeyBinding {
    KeyBinding::new(&platform(chord), action, context)
}

pub fn platform(chord: &str) -> String {
    let Some(rest) = chord.strip_prefix("cmd-") else {
        debug_assert!(
            !chord.contains("cmd"),
            "write the command modifier first: {chord}"
        );
        return chord.to_string();
    };
    debug_assert!(
        !rest.contains("cmd") && !rest.contains("ctrl"),
        "a chord cannot name both commands, and cmd goes first: {chord}"
    );
    format!("{CMD}-{rest}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConfigurableCommand {
    ShowSettings,
    SyncNow,

    OpenConfigurationFolder,
    ShowAccountSettings,
    SearchAllItems,
    NewItem,
    NewEvent,
    NewRoutine,
    NewMarker,
    NewSignal,
    StartRoutine,
    UseSavedAction,
    UseSavedEvent,
    DraftAsAction,
    DraftAsEvent,
    OpenDraftTypeMenu,
    ToggleThemeMode,
    ChooseTheme,
    GoToHome,
    GoToTimeline,
    GoToCalendar,
    GoToQueue,
    GoToFocus,
    GoToUnqueued,
    GoToRoutines,
    GoToSavedItems,
    ToggleLeftSidebar,
    ToggleRightSidebar,
    Undo,
    Redo,
    NextTab,
    PreviousTab,
    Refresh,
    GoToNow,
    EditSelectedItem,
    CompleteSelected,
    ToggleQueuedSelected,
    TogglePinnedSelected,
    DeleteSelected,
    CopySelected,
    CutSelected,
    PasteItems,
    DuplicateSelected,
    SelectAllItems,
}

impl ConfigurableCommand {
    pub const ALL: [Self; 44] = [
        Self::ShowSettings,
        Self::SyncNow,
        Self::OpenConfigurationFolder,
        Self::ShowAccountSettings,
        Self::SearchAllItems,
        Self::NewItem,
        Self::NewEvent,
        Self::NewRoutine,
        Self::NewMarker,
        Self::NewSignal,
        Self::StartRoutine,
        Self::UseSavedAction,
        Self::UseSavedEvent,
        Self::DraftAsAction,
        Self::DraftAsEvent,
        Self::OpenDraftTypeMenu,
        Self::ToggleThemeMode,
        Self::ChooseTheme,
        Self::GoToHome,
        Self::GoToTimeline,
        Self::GoToCalendar,
        Self::GoToQueue,
        Self::GoToFocus,
        Self::GoToUnqueued,
        Self::GoToRoutines,
        Self::GoToSavedItems,
        Self::ToggleLeftSidebar,
        Self::ToggleRightSidebar,
        Self::Undo,
        Self::Redo,
        Self::NextTab,
        Self::PreviousTab,
        Self::Refresh,
        Self::GoToNow,
        Self::EditSelectedItem,
        Self::CompleteSelected,
        Self::ToggleQueuedSelected,
        Self::TogglePinnedSelected,
        Self::DeleteSelected,
        Self::CopySelected,
        Self::CutSelected,
        Self::PasteItems,
        Self::DuplicateSelected,
        Self::SelectAllItems,
    ];

    pub const fn id(self) -> &'static str {
        match self {
            Self::ShowSettings => "application.show-settings",
            Self::SyncNow => "application.sync-now",

            Self::OpenConfigurationFolder => "application.open-configuration-folder",
            Self::ShowAccountSettings => "application.show-account-settings",
            Self::SearchAllItems => "application.search-all-items",
            Self::NewItem => "items.new-item",
            Self::NewEvent => "items.new-event",
            Self::NewRoutine => "items.new-routine",
            Self::NewMarker => "items.new-marker",
            Self::NewSignal => "items.new-signal",
            Self::StartRoutine => "items.start-routine",
            Self::UseSavedAction => "items.use-saved-action",
            Self::UseSavedEvent => "items.use-saved-event",
            Self::DraftAsAction => "draft.as-action",
            Self::DraftAsEvent => "draft.as-event",
            Self::OpenDraftTypeMenu => "draft.choose-type",
            Self::ToggleThemeMode => "appearance.toggle-light-dark",
            Self::ChooseTheme => "appearance.choose-theme",
            Self::GoToHome => "navigation.go-to-home",
            Self::GoToTimeline => "navigation.go-to-timeline",
            Self::GoToCalendar => "navigation.go-to-calendar",
            Self::GoToQueue => "navigation.go-to-queue",
            Self::GoToFocus => "navigation.go-to-focus",
            Self::GoToUnqueued => "navigation.go-to-unqueued",
            Self::GoToRoutines => "navigation.go-to-routines",
            Self::GoToSavedItems => "navigation.go-to-saved-items",
            Self::ToggleLeftSidebar => "view.toggle-left-sidebar",
            Self::ToggleRightSidebar => "view.toggle-right-sidebar",
            Self::Undo => "edit.undo",
            Self::Redo => "edit.redo",
            Self::NextTab => "navigation.next-tab",
            Self::PreviousTab => "navigation.previous-tab",
            Self::Refresh => "navigation.refresh",
            Self::GoToNow => "navigation.go-to-now",
            Self::EditSelectedItem => "selection.edit",
            Self::CompleteSelected => "selection.complete",
            Self::ToggleQueuedSelected => "selection.toggle-queued",
            Self::TogglePinnedSelected => "selection.toggle-pinned",
            Self::DeleteSelected => "selection.delete",
            Self::CopySelected => "selection.copy",
            Self::CutSelected => "selection.cut",
            Self::PasteItems => "selection.paste",
            Self::DuplicateSelected => "selection.duplicate",
            Self::SelectAllItems => "selection.select-all",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::ShowSettings => "Open settings…",
            Self::SyncNow => "Sync now",

            Self::OpenConfigurationFolder => "Open configuration folder",
            Self::ShowAccountSettings => "Open account settings…",
            Self::SearchAllItems => "Search all items…",
            Self::NewItem => "New action…",
            Self::NewEvent => "New event…",
            Self::NewRoutine => "New routine…",
            Self::NewMarker => "New marker…",
            Self::NewSignal => "New signal…",
            Self::StartRoutine => "Start routine…",
            Self::UseSavedAction => "Use saved action…",
            Self::UseSavedEvent => "Use saved event…",
            Self::DraftAsAction => "Make draft an action",
            Self::DraftAsEvent => "Make draft an event",
            Self::OpenDraftTypeMenu => "Choose draft item type…",
            Self::ToggleThemeMode => "Toggle light/dark mode",
            Self::ChooseTheme => "Choose theme…",
            Self::GoToHome => "Go to Home",
            Self::GoToTimeline => "Go to Timeline",
            Self::GoToCalendar => "Go to Calendar",
            Self::GoToQueue => "Go to Queue",
            Self::GoToFocus => "Go to Focus",
            Self::GoToUnqueued => "Go to Unqueued",
            Self::GoToRoutines => "Go to Routines",
            Self::GoToSavedItems => "Go to Saved Items",
            Self::ToggleLeftSidebar => "Toggle navigation sidebar",
            Self::ToggleRightSidebar => "Toggle item editor",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::NextTab => "Next view",
            Self::PreviousTab => "Previous view",
            Self::Refresh => "Refresh current view",
            Self::GoToNow => "Go to now",
            Self::EditSelectedItem => "Edit selected item…",
            Self::CompleteSelected => "Complete selected items",
            Self::ToggleQueuedSelected => "Queue or unqueue selected items",
            Self::TogglePinnedSelected => "Pin or unpin selected items",
            Self::DeleteSelected => "Delete selected items",
            Self::CopySelected => "Copy selected items",
            Self::CutSelected => "Cut selected items",
            Self::PasteItems => "Paste items",
            Self::DuplicateSelected => "Duplicate selected items",
            Self::SelectAllItems => "Select all items",
        }
    }

    fn palette_label(self, selection_count: usize) -> SharedString {
        if selection_count == 0 {
            return self.label().into();
        }
        let verb = match self {
            Self::CompleteSelected => "Complete",
            Self::ToggleQueuedSelected => "Queue or unqueue",
            Self::TogglePinnedSelected => "Pin or unpin",
            Self::DeleteSelected => "Delete",
            Self::CopySelected => "Copy",
            Self::CutSelected => "Cut",
            Self::DuplicateSelected => "Duplicate",
            _ => return self.label().into(),
        };
        let noun = if selection_count == 1 {
            "item"
        } else {
            "items"
        };
        format!("{verb} {selection_count} selected {noun}").into()
    }
    const fn context(self) -> Option<&'static str> {
        match self {
            Self::ShowSettings
            | Self::ShowAccountSettings
            | Self::SearchAllItems
            | Self::ToggleThemeMode => None,
            Self::Refresh | Self::GoToNow => Some(MAIN_VIEW_KEY_CONTEXT),
            Self::NextTab | Self::PreviousTab => Some(VIEW_SWITCH_KEY_CONTEXT),
            Self::DraftAsAction | Self::DraftAsEvent | Self::OpenDraftTypeMenu => {
                Some("ItemDraft && !ContextMenu")
            }
            _ => Some(ITEMS_KEY_CONTEXT),
        }
    }

    const fn context_label(self) -> &'static str {
        match self {
            Self::ShowSettings
            | Self::SyncNow
            | Self::OpenConfigurationFolder
            | Self::ShowAccountSettings
            | Self::SearchAllItems
            | Self::ToggleThemeMode
            | Self::ChooseTheme => "Application",
            Self::GoToHome
            | Self::GoToTimeline
            | Self::GoToCalendar
            | Self::GoToQueue
            | Self::GoToFocus
            | Self::GoToUnqueued
            | Self::GoToRoutines
            | Self::GoToSavedItems
            | Self::NextTab
            | Self::PreviousTab => "Navigation",
            Self::Refresh | Self::GoToNow => "Main views",
            Self::DraftAsAction | Self::DraftAsEvent | Self::OpenDraftTypeMenu => "Item drafts",
            _ => "Items",
        }
    }

    const fn defaults(self) -> &'static [&'static str] {
        match self {
            Self::ShowSettings => &["cmd-,"],
            Self::SearchAllItems => &["cmd-shift-f"],
            Self::NewItem => &["cmd-n"],
            Self::NewEvent => &["cmd-shift-n"],
            Self::NewRoutine => &["cmd-alt-n"],
            Self::DraftAsAction => &["cmd-alt-1"],
            Self::DraftAsEvent => &["cmd-alt-2"],
            Self::OpenDraftTypeMenu => &["alt-down"],
            Self::SyncNow
            | Self::OpenConfigurationFolder
            | Self::ShowAccountSettings
            | Self::EditSelectedItem
            | Self::NewMarker
            | Self::NewSignal
            | Self::StartRoutine
            | Self::UseSavedAction
            | Self::UseSavedEvent
            | Self::ToggleThemeMode
            | Self::ChooseTheme
            | Self::GoToHome
            | Self::GoToTimeline
            | Self::GoToCalendar
            | Self::GoToQueue
            | Self::GoToFocus
            | Self::GoToUnqueued
            | Self::GoToRoutines
            | Self::GoToSavedItems => &[],
            Self::ToggleLeftSidebar => &["cmd-b"],
            Self::ToggleRightSidebar => &["cmd-alt-b"],
            Self::Undo => &["cmd-z"],
            Self::Redo => &["cmd-shift-z"],
            Self::NextTab => &["ctrl-tab"],
            Self::PreviousTab => &["ctrl-shift-tab"],
            Self::Refresh => &["cmd-r"],
            Self::GoToNow => &["cmd-)"],
            Self::CompleteSelected => &["cmd-enter"],
            Self::ToggleQueuedSelected => &["cmd-shift-enter"],
            Self::TogglePinnedSelected => &["cmd-p"],
            Self::DeleteSelected => &["delete", "backspace"],
            Self::CopySelected => &["cmd-c"],
            Self::CutSelected => &["cmd-x"],
            Self::PasteItems => &["cmd-v"],
            Self::DuplicateSelected => &["cmd-d"],
            Self::SelectAllItems => &["cmd-a"],
        }
    }

    const fn keywords(self) -> &'static [&'static str] {
        match self {
            Self::ShowSettings => &["preferences", "configuration"],
            Self::SyncNow => &["synchronize", "sync", "refresh"],

            Self::OpenConfigurationFolder => &["config", "directory", "files"],
            Self::ShowAccountSettings => &["settings", "account", "profile"],
            Self::SearchAllItems => &["find", "filter"],
            Self::NewItem
            | Self::NewEvent
            | Self::NewRoutine
            | Self::NewMarker
            | Self::NewSignal => &["create", "add"],
            Self::StartRoutine => &["run", "begin", "library"],
            Self::UseSavedAction | Self::UseSavedEvent => &["create", "template", "library"],
            Self::DraftAsAction | Self::DraftAsEvent | Self::OpenDraftTypeMenu => {
                &["draft", "type", "action", "event"]
            }
            Self::ToggleThemeMode | Self::ChooseTheme => &["theme", "appearance", "light", "dark"],
            Self::GoToHome
            | Self::GoToTimeline
            | Self::GoToCalendar
            | Self::GoToQueue
            | Self::GoToFocus
            | Self::GoToUnqueued
            | Self::GoToRoutines
            | Self::GoToSavedItems => &["navigate", "view", "switch"],
            Self::ToggleLeftSidebar => &["navigation", "sidebar", "rail"],
            Self::ToggleRightSidebar | Self::EditSelectedItem => &["editor", "details", "item"],
            Self::NextTab | Self::PreviousTab => &["tab", "view", "switch"],
            Self::Refresh => &["reload", "sync"],
            Self::GoToNow => &["today", "current", "first"],
            _ => &["selection", "items", "edit"],
        }
    }

    pub(crate) fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.id() == id)
    }

    pub(crate) const fn appears_in_palette(self) -> bool {
        !matches!(
            self,
            Self::ToggleRightSidebar
                | Self::DraftAsAction
                | Self::DraftAsEvent
                | Self::OpenDraftTypeMenu
        )
    }

    pub(crate) const fn targets_main_view(self) -> bool {
        matches!(self, Self::Refresh | Self::GoToNow)
    }

    pub(crate) fn action(self) -> Box<dyn Action> {
        match self {
            Self::ShowSettings => Box::new(ShowSettings),
            Self::SyncNow => Box::new(SyncNow),

            Self::OpenConfigurationFolder => Box::new(OpenConfigurationFolder),
            Self::ShowAccountSettings => Box::new(ShowAccountSettings),
            Self::SearchAllItems => Box::new(ToggleSearch),
            Self::NewItem => Box::new(StartItemCreator),
            Self::NewEvent => Box::new(StartEventCreator),
            Self::NewRoutine => Box::new(StartRoutineCreator),
            Self::NewMarker => Box::new(StartMarkerCreator),
            Self::NewSignal => Box::new(StartSignalCreator),
            Self::StartRoutine => Box::new(StartRoutine),
            Self::UseSavedAction => Box::new(UseSavedAction),
            Self::UseSavedEvent => Box::new(UseSavedEvent),
            Self::DraftAsAction => Box::new(DraftAsAction),
            Self::DraftAsEvent => Box::new(DraftAsEvent),
            Self::OpenDraftTypeMenu => Box::new(OpenDraftTypeMenu),
            Self::ToggleThemeMode => Box::new(ToggleThemeMode),
            Self::ChooseTheme => Box::new(ChooseTheme),
            Self::GoToHome => Box::new(GoToHome),
            Self::GoToTimeline => Box::new(GoToTimeline),
            Self::GoToCalendar => Box::new(GoToCalendar),
            Self::GoToQueue => Box::new(GoToQueue),
            Self::GoToFocus => Box::new(GoToFocus),
            Self::GoToUnqueued => Box::new(GoToUnqueued),
            Self::GoToRoutines => Box::new(GoToRoutines),
            Self::GoToSavedItems => Box::new(GoToSavedItems),
            Self::ToggleLeftSidebar => Box::new(ToggleLeftSidebar),
            Self::ToggleRightSidebar => Box::new(ToggleRightSidebar),
            Self::Undo => Box::new(Undo),
            Self::Redo => Box::new(Redo),
            Self::NextTab => Box::new(NextTab),
            Self::PreviousTab => Box::new(PreviousTab),
            Self::Refresh => Box::new(RefreshPipeline),
            Self::GoToNow => Box::new(GoToNow),
            Self::EditSelectedItem => Box::new(EditSelectedItem),
            Self::CompleteSelected => Box::new(CompleteSelected),
            Self::ToggleQueuedSelected => Box::new(ToggleQueuedSelected),
            Self::TogglePinnedSelected => Box::new(TogglePinnedSelected),
            Self::DeleteSelected => Box::new(DeleteSelected),
            Self::CopySelected => Box::new(CopySelected),
            Self::CutSelected => Box::new(CutSelected),
            Self::PasteItems => Box::new(PasteItems),
            Self::DuplicateSelected => Box::new(DuplicateSelected),
            Self::SelectAllItems => Box::new(SelectAllItems),
        }
    }

    fn action_name(self) -> &'static str {
        match self {
            Self::ShowSettings => ShowSettings::name_for_type(),
            Self::SyncNow => SyncNow::name_for_type(),

            Self::OpenConfigurationFolder => OpenConfigurationFolder::name_for_type(),
            Self::ShowAccountSettings => ShowAccountSettings::name_for_type(),
            Self::SearchAllItems => ToggleSearch::name_for_type(),
            Self::NewItem => StartItemCreator::name_for_type(),
            Self::NewEvent => StartEventCreator::name_for_type(),
            Self::NewRoutine => StartRoutineCreator::name_for_type(),
            Self::NewMarker => StartMarkerCreator::name_for_type(),
            Self::NewSignal => StartSignalCreator::name_for_type(),
            Self::StartRoutine => StartRoutine::name_for_type(),
            Self::UseSavedAction => UseSavedAction::name_for_type(),
            Self::UseSavedEvent => UseSavedEvent::name_for_type(),
            Self::DraftAsAction => DraftAsAction::name_for_type(),
            Self::DraftAsEvent => DraftAsEvent::name_for_type(),
            Self::OpenDraftTypeMenu => OpenDraftTypeMenu::name_for_type(),
            Self::ToggleThemeMode => ToggleThemeMode::name_for_type(),
            Self::ChooseTheme => ChooseTheme::name_for_type(),
            Self::GoToHome => GoToHome::name_for_type(),
            Self::GoToTimeline => GoToTimeline::name_for_type(),
            Self::GoToCalendar => GoToCalendar::name_for_type(),
            Self::GoToQueue => GoToQueue::name_for_type(),
            Self::GoToFocus => GoToFocus::name_for_type(),
            Self::GoToUnqueued => GoToUnqueued::name_for_type(),
            Self::GoToRoutines => GoToRoutines::name_for_type(),
            Self::GoToSavedItems => GoToSavedItems::name_for_type(),
            Self::ToggleLeftSidebar => ToggleLeftSidebar::name_for_type(),
            Self::ToggleRightSidebar => ToggleRightSidebar::name_for_type(),
            Self::Undo => Undo::name_for_type(),
            Self::Redo => Redo::name_for_type(),
            Self::NextTab => NextTab::name_for_type(),
            Self::PreviousTab => PreviousTab::name_for_type(),
            Self::Refresh => RefreshPipeline::name_for_type(),
            Self::GoToNow => GoToNow::name_for_type(),
            Self::EditSelectedItem => EditSelectedItem::name_for_type(),
            Self::CompleteSelected => CompleteSelected::name_for_type(),
            Self::ToggleQueuedSelected => ToggleQueuedSelected::name_for_type(),
            Self::TogglePinnedSelected => TogglePinnedSelected::name_for_type(),
            Self::DeleteSelected => DeleteSelected::name_for_type(),
            Self::CopySelected => CopySelected::name_for_type(),
            Self::CutSelected => CutSelected::name_for_type(),
            Self::PasteItems => PasteItems::name_for_type(),
            Self::DuplicateSelected => DuplicateSelected::name_for_type(),
            Self::SelectAllItems => SelectAllItems::name_for_type(),
        }
    }

    fn binding(self, chord: &str) -> KeyBinding {
        match self {
            Self::ShowSettings => key(chord, ShowSettings, self.context()),
            Self::SyncNow => key(chord, SyncNow, self.context()),

            Self::OpenConfigurationFolder => key(chord, OpenConfigurationFolder, self.context()),
            Self::ShowAccountSettings => key(chord, ShowAccountSettings, self.context()),
            Self::SearchAllItems => key(chord, ToggleSearch, self.context()),
            Self::NewItem => key(chord, StartItemCreator, self.context()),
            Self::NewEvent => key(chord, StartEventCreator, self.context()),
            Self::NewRoutine => key(chord, StartRoutineCreator, self.context()),
            Self::NewMarker => key(chord, StartMarkerCreator, self.context()),
            Self::NewSignal => key(chord, StartSignalCreator, self.context()),
            Self::StartRoutine => key(chord, StartRoutine, self.context()),
            Self::UseSavedAction => key(chord, UseSavedAction, self.context()),
            Self::UseSavedEvent => key(chord, UseSavedEvent, self.context()),
            Self::DraftAsAction => key(chord, DraftAsAction, self.context()),
            Self::DraftAsEvent => key(chord, DraftAsEvent, self.context()),
            Self::OpenDraftTypeMenu => key(chord, OpenDraftTypeMenu, self.context()),
            Self::ToggleThemeMode => key(chord, ToggleThemeMode, self.context()),
            Self::ChooseTheme => key(chord, ChooseTheme, self.context()),
            Self::GoToHome => key(chord, GoToHome, self.context()),
            Self::GoToTimeline => key(chord, GoToTimeline, self.context()),
            Self::GoToCalendar => key(chord, GoToCalendar, self.context()),
            Self::GoToQueue => key(chord, GoToQueue, self.context()),
            Self::GoToFocus => key(chord, GoToFocus, self.context()),
            Self::GoToUnqueued => key(chord, GoToUnqueued, self.context()),
            Self::GoToRoutines => key(chord, GoToRoutines, self.context()),
            Self::GoToSavedItems => key(chord, GoToSavedItems, self.context()),
            Self::ToggleLeftSidebar => key(chord, ToggleLeftSidebar, self.context()),
            Self::ToggleRightSidebar => key(chord, ToggleRightSidebar, self.context()),
            Self::Undo => key(chord, Undo, self.context()),
            Self::Redo => key(chord, Redo, self.context()),
            Self::NextTab => key(chord, NextTab, self.context()),
            Self::PreviousTab => key(chord, PreviousTab, self.context()),
            Self::Refresh => key(chord, RefreshPipeline, self.context()),
            Self::GoToNow => key(chord, GoToNow, self.context()),
            Self::EditSelectedItem => key(chord, EditSelectedItem, self.context()),
            Self::CompleteSelected => key(chord, CompleteSelected, self.context()),
            Self::ToggleQueuedSelected => key(chord, ToggleQueuedSelected, self.context()),
            Self::TogglePinnedSelected => key(chord, TogglePinnedSelected, self.context()),
            Self::DeleteSelected => key(chord, DeleteSelected, self.context()),
            Self::CopySelected => key(chord, CopySelected, self.context()),
            Self::CutSelected => key(chord, CutSelected, self.context()),
            Self::PasteItems => key(chord, PasteItems, self.context()),
            Self::DuplicateSelected => key(chord, DuplicateSelected, self.context()),
            Self::SelectAllItems => key(chord, SelectAllItems, self.context()),
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct PaletteCommandState {
    pub selection_count: usize,
    pub unavailable: Option<&'static str>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeymapConfig {
    overrides: BTreeMap<String, Vec<String>>,
}

impl KeymapConfig {
    pub fn from_overrides(overrides: BTreeMap<String, Vec<String>>) -> Result<Self, String> {
        let mut validated = BTreeMap::new();
        for (id, bindings) in overrides {
            let command = ConfigurableCommand::from_id(&id)
                .ok_or_else(|| format!("unknown configurable command {id}"))?;
            let mut unique = BTreeSet::new();
            let mut normalized = Vec::new();
            for binding in bindings {
                let binding = normalize_binding(&binding)?;
                if unique.insert(binding.clone()) {
                    normalized.push(binding);
                }
            }
            validated.insert(command.id().to_owned(), normalized);
        }
        Ok(Self {
            overrides: validated,
        })
    }

    pub fn overrides(&self) -> &BTreeMap<String, Vec<String>> {
        &self.overrides
    }

    pub fn add(&mut self, command_id: &str, binding: &str) -> Result<(), String> {
        let command = ConfigurableCommand::from_id(command_id)
            .ok_or_else(|| format!("unknown configurable command {command_id}"))?;
        let binding = normalize_binding(binding)?;
        let mut effective = self.effective(command);
        if !effective.contains(&binding) {
            effective.push(binding);
            self.overrides.insert(command.id().to_owned(), effective);
        }
        Ok(())
    }

    pub fn replace(
        &mut self,
        command_id: &str,
        binding_id: &str,
        binding: &str,
    ) -> Result<(), String> {
        let command = ConfigurableCommand::from_id(command_id)
            .ok_or_else(|| format!("unknown configurable command {command_id}"))?;
        let binding = normalize_binding(binding)?;
        let mut effective = self.effective(command);
        let index = effective
            .iter()
            .position(|binding| binding == binding_id)
            .ok_or_else(|| {
                format!(
                    "This shortcut for {} has changed. Try again.",
                    command.label()
                )
            })?;
        if effective[index] == binding {
            return Ok(());
        }
        if effective.contains(&binding) {
            effective.remove(index);
        } else {
            effective[index] = binding;
        }
        self.overrides.insert(command.id().to_owned(), effective);
        Ok(())
    }

    pub fn remove(&mut self, command_id: &str, binding_id: &str) -> Result<(), String> {
        let command = ConfigurableCommand::from_id(command_id)
            .ok_or_else(|| format!("unknown configurable command {command_id}"))?;
        let mut effective = self.effective(command);
        effective.retain(|binding| binding != binding_id);
        self.overrides.insert(command.id().to_owned(), effective);
        Ok(())
    }

    pub fn reset(&mut self, command_id: &str) -> Result<(), String> {
        let command = ConfigurableCommand::from_id(command_id)
            .ok_or_else(|| format!("unknown configurable command {command_id}"))?;
        self.overrides.remove(command.id());
        Ok(())
    }

    fn effective(&self, command: ConfigurableCommand) -> Vec<String> {
        self.overrides
            .get(command.id())
            .cloned()
            .unwrap_or_else(|| {
                command
                    .defaults()
                    .iter()
                    .map(|binding| platform(binding))
                    .collect()
            })
    }

    pub(crate) fn palette_commands(
        &self,
        recent: &[String],
        state: impl Fn(ConfigurableCommand) -> PaletteCommandState,
    ) -> Vec<Command> {
        let mut seen = BTreeSet::new();
        recent
            .iter()
            .filter_map(|id| ConfigurableCommand::from_id(id))
            .map(|command| (command, "Recently used"))
            .chain(
                ConfigurableCommand::ALL
                    .into_iter()
                    .map(|command| (command, command.context_label())),
            )
            .filter(|(command, _)| command.appears_in_palette() && seen.insert(*command))
            .filter_map(|(command, section)| {
                let state = state(command);
                if state.unavailable.is_some() {
                    return None;
                }
                let mut item =
                    Command::new(command.id(), command.palette_label(state.selection_count))
                        .section(section);
                if let Some(binding) = self.effective(command).first() {
                    item = item.shortcut(binding.clone());
                }
                Some(item)
            })
            .collect()
    }

    pub fn editor_commands(&self) -> Vec<KeymapCommand> {
        let effective: BTreeMap<_, _> = ConfigurableCommand::ALL
            .into_iter()
            .map(|command| (command, self.effective(command)))
            .collect();
        let mut owners: BTreeMap<String, Vec<ConfigurableCommand>> = BTreeMap::new();
        for (command, bindings) in &effective {
            for binding in bindings {
                owners.entry(binding.clone()).or_default().push(*command);
            }
        }

        ConfigurableCommand::ALL
            .into_iter()
            .map(|command| {
                let custom = self.overrides.contains_key(command.id());
                let bindings = effective[&command].iter().map(|binding| {
                    let others: Vec<_> = owners[binding]
                        .iter()
                        .copied()
                        .filter(|owner| *owner != command)
                        .map(ConfigurableCommand::label)
                        .collect();
                    let item = KeymapBinding::new(binding.clone(), binding.clone());
                    let item = if custom {
                        item.provenance("Custom")
                    } else {
                        item
                    };
                    if others.is_empty() {
                        item
                    } else {
                        item.conflict(format!("Also assigned to {}", others.join(", ")))
                    }
                });
                KeymapCommand::new(command.id(), command.label())
                    .context(command.context_label())
                    .defaults(command.defaults().iter().map(|binding| platform(binding)))
                    .bindings(bindings)
                    .searchable(
                        format!("{} {}", command.label(), effective[&command].join(" ")),
                        command.keywords().iter().copied(),
                    )
            })
            .collect()
    }
}

pub(crate) fn init_draft_shortcuts(cx: &mut App) {
    for command in [
        ConfigurableCommand::DraftAsAction,
        ConfigurableCommand::DraftAsEvent,
        ConfigurableCommand::OpenDraftTypeMenu,
    ] {
        cx.bind_keys(
            command
                .defaults()
                .iter()
                .map(|chord| command.binding(chord)),
        );
    }
}

pub fn apply_keymap_transition(cx: &mut App, previous: &KeymapConfig, next: &KeymapConfig) {
    let changes = keymap_transition(previous, next);
    if !changes.is_empty() {
        cx.bind_keys(changes);
    }
}

fn keymap_transition(previous: &KeymapConfig, next: &KeymapConfig) -> Vec<KeyBinding> {
    let mut changes = Vec::new();
    for command in ConfigurableCommand::ALL {
        let old = previous.effective(command);
        let new = next.effective(command);
        if old == new {
            continue;
        }
        for binding in old {
            changes.push(KeyBinding::new(
                &binding,
                Unbind(SharedString::from(command.action_name())),
                command.context(),
            ));
        }
        changes.extend(new.iter().map(|binding| command.binding(binding)));
    }
    changes
}

fn normalize_binding(binding: &str) -> Result<String, String> {
    let binding = binding.trim();
    if binding.is_empty() {
        return Err("a keybinding cannot be empty".into());
    }
    binding
        .split_whitespace()
        .map(|stroke| {
            Keystroke::parse(stroke)
                .map(|stroke| stroke.unparse())
                .map_err(|error| format!("invalid keybinding {binding}: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|strokes| strokes.join(" "))
}

pub fn force_click_modifier(modifiers: &Modifiers) -> bool {
    force_click_modifier_for(cfg!(target_os = "macos"), modifiers)
}

fn force_click_modifier_for(is_macos: bool, modifiers: &Modifiers) -> bool {
    !is_macos && modifiers.control
}
