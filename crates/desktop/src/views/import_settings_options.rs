use gpui::prelude::FluentBuilder as _;
use gpui::{App, AppContext as _, Context, Entity, Window};
use gpui_kit::{
    controls::{
        select::{Select, SelectEvent, SelectOption},
        settings_row::SettingsRow,
        toggle::Switch,
    },
    foundation::{Disableable as _, Sizable as _},
};

use super::{ImportSettings, state};
use crate::{imports::status::IntegrationKind, settings::Settings};

#[derive(Clone, Copy)]
pub(super) enum WindowEdge {
    Past,
    Future,
}

impl WindowEdge {
    fn id(self) -> &'static str {
        match self {
            Self::Past => "past",
            Self::Future => "future",
        }
    }
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Past => "Import from the past",
            Self::Future => "Import upcoming",
        }
    }

    fn days(self, settings: &Settings, kind: IntegrationKind) -> u32 {
        match (kind, self) {
            #[cfg(target_os = "macos")]
            (IntegrationKind::EventKit, _) => settings.integrations.eventkit.future_days(),
            (IntegrationKind::GoogleCalendar, Self::Past) => {
                settings.integrations.google_calendar.past_days()
            }
            (IntegrationKind::GoogleCalendar, Self::Future) => {
                settings.integrations.google_calendar.future_days()
            }
            (IntegrationKind::Outlook, Self::Past) => settings.integrations.outlook.past_days(),
            (IntegrationKind::Outlook, Self::Future) => settings.integrations.outlook.future_days(),
            #[cfg(feature = "private-workiq")]
            (IntegrationKind::WorkIq, Self::Past) => settings.integrations.workiq.past_days(),
            #[cfg(feature = "private-workiq")]
            (IntegrationKind::WorkIq, Self::Future) => settings.integrations.workiq.future_days(),
            (IntegrationKind::Todoist, _) => 0,
        }
    }

    fn set(self, settings: &mut Settings, kind: IntegrationKind, days: u32) {
        match (kind, self) {
            #[cfg(target_os = "macos")]
            (IntegrationKind::EventKit, _) => settings.integrations.eventkit.set_future_days(days),
            (IntegrationKind::GoogleCalendar, Self::Past) => {
                settings.integrations.google_calendar.set_past_days(days)
            }
            (IntegrationKind::GoogleCalendar, Self::Future) => {
                settings.integrations.google_calendar.set_future_days(days)
            }
            (IntegrationKind::Outlook, Self::Past) => {
                settings.integrations.outlook.set_past_days(days)
            }
            (IntegrationKind::Outlook, Self::Future) => {
                settings.integrations.outlook.set_future_days(days)
            }
            #[cfg(feature = "private-workiq")]
            (IntegrationKind::WorkIq, Self::Past) => {
                settings.integrations.workiq.set_past_days(days)
            }
            #[cfg(feature = "private-workiq")]
            (IntegrationKind::WorkIq, Self::Future) => {
                settings.integrations.workiq.set_future_days(days)
            }
            (IntegrationKind::Todoist, _) => {}
        }
    }
}

pub(super) struct WindowSelect {
    pub kind: IntegrationKind,
    edge: WindowEdge,
    select: Entity<Select>,
}

impl WindowSelect {
    pub(super) fn new(
        kind: IntegrationKind,
        edge: WindowEdge,
        window: &mut Window,
        cx: &mut Context<ImportSettings>,
    ) -> Self {
        let days = edge.days(&Settings::global(cx), kind);
        let mut choices = vec![0, 7, 14, 30, 60, 90, 180, 365, days];
        #[cfg(target_os = "macos")]
        if kind == IntegrationKind::EventKit {
            choices.retain(|days| *days > 0);
        }
        choices.sort_unstable();
        choices.dedup();
        let select = cx.new(|cx| {
            Select::new(
                format!("settings.import.{}.{}", kind.provider_id(), edge.id()),
                window,
                cx,
            )
            .large()
            .name(edge.label())
            .options(
                choices
                    .into_iter()
                    .map(|days| SelectOption::new(days.to_string(), format!("{days} days"))),
            )
            .selected(days.to_string())
        });
        cx.subscribe(&select, move |_, _, event: &SelectEvent, cx| {
            if let SelectEvent::Selected(id) = event
                && let Ok(days) = id.parse()
                && !state::busy(cx, kind)
            {
                Settings::update(cx, |settings| edge.set(settings, kind, days));
            }
        })
        .detach();
        Self { kind, edge, select }
    }

    pub(super) fn row(&self, cx: &mut App) -> SettingsRow {
        let days = self.edge.days(&Settings::global(cx), self.kind);
        let disabled = state::busy(cx, self.kind);
        self.select.update(cx, |select, cx| {
            select.set_selected(Some(days.to_string().into()), cx);
            if select.is_disabled() != disabled {
                select.set_disabled(disabled, cx);
            }
        });
        SettingsRow::new(
            format!(
                "settings.import.{}.{}.row",
                self.kind.provider_id(),
                self.edge.id()
            ),
            self.edge.label(),
        )
        .search_terms(["Import", self.kind.label()])
        .select(self.select.clone())
    }
}

#[derive(Clone, Copy)]
pub(super) enum Channel {
    Calendar,
    Tasks,
    AcceptedOnly,
}

impl Channel {
    fn id(self) -> &'static str {
        match self {
            Self::Calendar => "calendar",
            Self::Tasks => "tasks",
            Self::AcceptedOnly => "accepted-only",
        }
    }

    fn get(self, settings: &Settings, kind: IntegrationKind) -> bool {
        let imports = &settings.integrations;
        match (kind, self) {
            #[cfg(target_os = "macos")]
            (IntegrationKind::EventKit, Self::Calendar) => imports.eventkit.events,
            #[cfg(target_os = "macos")]
            (IntegrationKind::EventKit, Self::Tasks) => imports.eventkit.reminders,
            #[cfg(target_os = "macos")]
            (IntegrationKind::EventKit, Self::AcceptedOnly) => {
                imports.eventkit.accepted_events_only
            }
            (IntegrationKind::GoogleCalendar, Self::AcceptedOnly) => {
                imports.google_calendar.accepted_events_only
            }
            (IntegrationKind::Outlook, Self::Calendar) => imports.outlook.calendar,
            (IntegrationKind::Outlook, Self::Tasks) => imports.outlook.flagged_mail,
            (IntegrationKind::Outlook, Self::AcceptedOnly) => imports.outlook.accepted_events_only,
            #[cfg(feature = "private-workiq")]
            (IntegrationKind::WorkIq, Self::Calendar) => imports.workiq.calendar,
            #[cfg(feature = "private-workiq")]
            (IntegrationKind::WorkIq, Self::Tasks) => imports.workiq.flagged_mail,
            #[cfg(feature = "private-workiq")]
            (IntegrationKind::WorkIq, Self::AcceptedOnly) => imports.workiq.accepted_events_only,
            _ => false,
        }
    }

    fn set(self, settings: &mut Settings, kind: IntegrationKind, value: bool) {
        let imports = &mut settings.integrations;
        match (kind, self) {
            #[cfg(target_os = "macos")]
            (IntegrationKind::EventKit, Self::Calendar) => imports.eventkit.events = value,
            #[cfg(target_os = "macos")]
            (IntegrationKind::EventKit, Self::Tasks) => imports.eventkit.reminders = value,
            #[cfg(target_os = "macos")]
            (IntegrationKind::EventKit, Self::AcceptedOnly) => {
                imports.eventkit.accepted_events_only = value
            }
            (IntegrationKind::GoogleCalendar, Self::AcceptedOnly) => {
                imports.google_calendar.accepted_events_only = value
            }
            (IntegrationKind::Outlook, Self::Calendar) => imports.outlook.calendar = value,
            (IntegrationKind::Outlook, Self::Tasks) => imports.outlook.flagged_mail = value,
            (IntegrationKind::Outlook, Self::AcceptedOnly) => {
                imports.outlook.accepted_events_only = value
            }
            #[cfg(feature = "private-workiq")]
            (IntegrationKind::WorkIq, Self::Calendar) => imports.workiq.calendar = value,
            #[cfg(feature = "private-workiq")]
            (IntegrationKind::WorkIq, Self::Tasks) => imports.workiq.flagged_mail = value,
            #[cfg(feature = "private-workiq")]
            (IntegrationKind::WorkIq, Self::AcceptedOnly) => {
                imports.workiq.accepted_events_only = value
            }
            _ => {}
        }
    }

    pub(super) fn row(self, kind: IntegrationKind, cx: &App) -> SettingsRow {
        let (label, info) = match self {
            Self::Calendar => (
                "Calendar",
                "Timed entries become events; all-day entries become markers.",
            ),
            #[cfg(target_os = "macos")]
            Self::Tasks if kind == IntegrationKind::EventKit => (
                "Reminders",
                "Reminders become actions with the same due date.",
            ),
            Self::Tasks => (
                "Flagged mail",
                "Flagged messages become actions. Reading imports never changes your mailbox.",
            ),
            Self::AcceptedOnly => (
                "Accepted events only",
                "Import events you organized or accepted. Other invitations are excluded from import.",
            ),
        };
        let id = format!("settings.import.{}.{}", kind.provider_id(), self.id());
        let disabled = state::busy(cx, kind);
        SettingsRow::new(format!("{id}.row"), label)
            .info(info)
            .search_terms(["Import", kind.label()])
            .switch(
                Switch::new(id)
                    .named(label)
                    .large()
                    .on(self.get(&Settings::global(cx), kind))
                    .disabled(disabled)
                    .when(!disabled, |switch| {
                        switch.on_change(move |value, _, cx| {
                            if !state::busy(cx, kind) {
                                Settings::update(cx, |settings| self.set(settings, kind, value));
                            }
                        })
                    }),
            )
    }
}
