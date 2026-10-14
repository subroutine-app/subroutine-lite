use std::collections::HashMap;

use chrono::{DateTime, Utc};
use gpui::{App, BorrowAppContext as _, Global};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntegrationKind {
    #[cfg(target_os = "macos")]
    EventKit,
    GoogleCalendar,
    Outlook,
    Todoist,
    #[cfg(feature = "private-workiq")]
    WorkIq,
}

impl IntegrationKind {
    pub const fn provider_id(self) -> &'static str {
        match self {
            #[cfg(target_os = "macos")]
            Self::EventKit => "eventkit",
            Self::GoogleCalendar => "google_calendar",
            Self::Outlook => "outlook",
            Self::Todoist => "todoist",
            #[cfg(feature = "private-workiq")]
            Self::WorkIq => "workiq",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            #[cfg(target_os = "macos")]
            Self::EventKit => "Apple Calendar & Reminders",
            Self::GoogleCalendar => "Google Calendar",
            Self::Outlook => "Outlook / Microsoft 365",
            Self::Todoist => "Todoist",
            #[cfg(feature = "private-workiq")]
            Self::WorkIq => "Microsoft 365 via WorkIQ",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum IntegrationState {
    #[default]
    Inactive,
    Syncing,
    Live,
    Error(String),
}

#[derive(Debug, Clone, Default)]
pub struct IntegrationStatus {
    pub state: IntegrationState,
    pub last_sync: Option<DateTime<Utc>>,
    pub last_imported: usize,
}

#[derive(Debug, Default)]
pub struct IntegrationStatuses(HashMap<IntegrationKind, IntegrationStatus>);

impl Global for IntegrationStatuses {}

impl IntegrationStatuses {
    pub fn initialize_global(cx: &mut App) {
        cx.set_global(Self::default());
    }

    pub fn get(cx: &App, kind: IntegrationKind) -> IntegrationStatus {
        cx.try_global::<Self>()
            .and_then(|statuses| statuses.0.get(&kind).cloned())
            .unwrap_or_default()
    }

    pub fn set(cx: &mut App, kind: IntegrationKind, state: IntegrationState) {
        if cx.try_global::<Self>().is_none() {
            Self::initialize_global(cx);
        }
        cx.update_global::<Self, ()>(|statuses, _| {
            let entry = statuses.0.entry(kind).or_default();
            entry.state = state;
        });
        cx.refresh_windows();
    }

    pub fn succeeded(cx: &mut App, kind: IntegrationKind, imported: usize) {
        if cx.try_global::<Self>().is_none() {
            Self::initialize_global(cx);
        }
        cx.update_global::<Self, ()>(|statuses, _| {
            let entry = statuses.0.entry(kind).or_default();
            entry.state = IntegrationState::Live;
            entry.last_sync = Some(Utc::now());
            entry.last_imported = imported;
        });
        cx.refresh_windows();
    }
}
