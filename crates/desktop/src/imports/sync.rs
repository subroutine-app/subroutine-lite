use std::{collections::HashSet, time::Duration};

use gpui::{App, AppContext as _, AsyncApp, BorrowAppContext as _, Global};
use subroutine_import::provider::{Provider, SyncOutcome};

use super::{
    credentials::{self, IntegrationCredential, OAuthProvider},
    status::{IntegrationKind, IntegrationState, IntegrationStatuses},
};
use crate::{
    settings::{GlobalSettings, IntegrationSettings, Settings},
    stores::AppDatabaseStore,
};

const INTERVAL: Duration = Duration::from_secs(5 * 60);
const KINDS: &[IntegrationKind] = &[
    #[cfg(target_os = "macos")]
    IntegrationKind::EventKit,
    IntegrationKind::GoogleCalendar,
    IntegrationKind::Outlook,
    IntegrationKind::Todoist,
    #[cfg(feature = "private-workiq")]
    IntegrationKind::WorkIq,
];

#[derive(Clone, PartialEq, Eq)]
struct ImportScope {
    workspace: u64,
    ready: bool,
    settings: IntegrationSettings,
}

impl ImportScope {
    fn read(cx: &App) -> Self {
        let store = AppDatabaseStore::global(cx);
        let store = store.read(cx);
        Self {
            workspace: store.workspace_generation(),
            ready: store.is_ready(),
            settings: Settings::global(cx).integrations,
        }
    }

    fn enabled(&self, kind: IntegrationKind) -> bool {
        match kind {
            #[cfg(target_os = "macos")]
            IntegrationKind::EventKit => self.settings.eventkit.imports_anything(),
            IntegrationKind::GoogleCalendar => self.settings.google_calendar.imports_anything(),
            IntegrationKind::Outlook => self.settings.outlook.imports_anything(),
            IntegrationKind::Todoist => self.settings.todoist.enabled,
            #[cfg(feature = "private-workiq")]
            IntegrationKind::WorkIq => self.settings.workiq.imports_anything(),
        }
    }

    fn matches(&self, other: &Self, kind: IntegrationKind) -> bool {
        self.workspace == other.workspace
            && self.ready == other.ready
            && match kind {
                #[cfg(target_os = "macos")]
                IntegrationKind::EventKit => self.settings.eventkit == other.settings.eventkit,
                IntegrationKind::GoogleCalendar => {
                    self.settings.google_calendar == other.settings.google_calendar
                }
                IntegrationKind::Outlook => self.settings.outlook == other.settings.outlook,
                IntegrationKind::Todoist => self.settings.todoist == other.settings.todoist,
                #[cfg(feature = "private-workiq")]
                IntegrationKind::WorkIq => self.settings.workiq == other.settings.workiq,
            }
    }
}

#[derive(Default)]
struct ImportJobs {
    observed: Option<ImportScope>,
    running: HashSet<IntegrationKind>,
}
impl Global for ImportJobs {}

pub fn start(cx: &mut App) {
    cx.set_global(ImportJobs::default());
    IntegrationStatuses::initialize_global(cx);
    cx.observe_global::<GlobalSettings>(|cx| cx.defer(|cx| refresh(cx, false)))
        .detach();
    let store = AppDatabaseStore::global(cx);
    cx.observe(&store, |_, cx| cx.defer(|cx| refresh(cx, false)))
        .detach();
    cx.defer(|cx| refresh(cx, false));
    cx.spawn(async move |cx: &mut AsyncApp| {
        loop {
            cx.background_executor().timer(INTERVAL).await;
            cx.update(sync_now);
        }
    })
    .detach();
}

pub fn sync_now(cx: &mut App) {
    refresh(cx, true);
}

pub fn is_running(kind: IntegrationKind, cx: &App) -> bool {
    cx.try_global::<ImportJobs>()
        .is_some_and(|jobs| jobs.running.contains(&kind))
}

fn refresh(cx: &mut App, force: bool) {
    let scope = ImportScope::read(cx);
    let previous =
        cx.update_global::<ImportJobs, _>(|jobs, _| jobs.observed.replace(scope.clone()));
    for &kind in KINDS {
        if force
            || previous
                .as_ref()
                .is_none_or(|previous| !scope.matches(previous, kind))
        {
            run(kind, scope.clone(), cx);
        }
    }
}

fn run(kind: IntegrationKind, scope: ImportScope, cx: &mut App) {
    if cx.global::<ImportJobs>().running.contains(&kind) {
        return;
    }
    if !scope.enabled(kind) {
        IntegrationStatuses::set(cx, kind, IntegrationState::Inactive);
        return;
    }
    if !scope.ready {
        IntegrationStatuses::set(
            cx,
            kind,
            IntegrationState::Error(
                "The local workspace is not ready for imports. Retry opening it in Settings."
                    .into(),
            ),
        );
        return;
    }
    cx.update_global::<ImportJobs, _>(|jobs, _| {
        jobs.running.insert(kind);
    });
    IntegrationStatuses::set(cx, kind, IntegrationState::Syncing);
    cx.spawn(async move |cx: &mut AsyncApp| {
        let settings = scope.settings.clone();
        let outcome = cx
            .background_spawn(async move {
                // EventKit managers are !Send; their entire lifetime stays in this poll.
                #[cfg(target_os = "macos")]
                {
                    objc2::rc::autoreleasepool(|_| fetch(kind, settings))
                }
                #[cfg(not(target_os = "macos"))]
                {
                    fetch(kind, settings)
                }
            })
            .await;
        let import = cx.update(|cx| {
            if !scope.matches(&ImportScope::read(cx), kind) {
                return None;
            }
            Some(match outcome {
                Ok(outcome) => AppDatabaseStore::global(cx)
                    .update(cx, |store, cx| store.apply_provider_import(outcome, cx)),
                Err(error) => gpui::Task::ready(Err(error)),
            })
        });
        let result = match import {
            Some(task) => Some(task.await),
            None => None,
        };
        cx.update(|cx| {
            cx.update_global::<ImportJobs, _>(|jobs, _| {
                jobs.running.remove(&kind);
            });
            let current = ImportScope::read(cx);
            if !scope.matches(&current, kind) {
                // Never attach a completed fetch to a different account or import policy.
                IntegrationStatuses::set(cx, kind, IntegrationState::Inactive);
                run(kind, current, cx);
                return;
            }
            let Some(result) = result else {
                run(kind, current, cx);
                return;
            };
            match result {
                Ok(count) => IntegrationStatuses::succeeded(cx, kind, count),
                Err(error) => {
                    tracing::warn!(provider = kind.provider_id(), %error, "import failed");
                    IntegrationStatuses::set(cx, kind, IntegrationState::Error(error));
                }
            }
        });
    })
    .detach();
}

fn fetch(kind: IntegrationKind, settings: IntegrationSettings) -> Result<SyncOutcome, String> {
    use subroutine_import::providers;

    let mut provider: Box<dyn Provider> = match kind {
        #[cfg(target_os = "macos")]
        IntegrationKind::EventKit => {
            super::apple::availability()?;
            let options = settings.eventkit;
            let mut provider = providers::eventkit::EventKitProvider::new()
                .with_future_days(i64::from(options.future_days()));
            if options.accepted_events_only {
                provider = provider.accepted_events_only();
            }
            if !options.reminders {
                provider = provider.events_only();
            }
            if !options.events {
                provider = provider.reminders_only();
            }
            Box::new(provider)
        }
        IntegrationKind::GoogleCalendar => {
            require_credential(IntegrationCredential::Google, "Google Calendar")?;
            let token = credentials::access_token_blocking(OAuthProvider::google())?;
            let options = settings.google_calendar;
            let mut provider = providers::google_calendar::GoogleCalendarProvider::new(token)
                .with_window(
                    i64::from(options.past_days()),
                    i64::from(options.future_days()),
                )
                .with_calendars(options.selected_calendars());
            if options.accepted_events_only {
                provider = provider.accepted_events_only();
            }
            Box::new(provider)
        }
        IntegrationKind::Outlook => {
            require_credential(IntegrationCredential::Microsoft, "Outlook / Microsoft 365")?;
            let token = credentials::access_token_blocking(OAuthProvider::microsoft())?;
            let options = settings.outlook;
            let mut provider = providers::outlook::OutlookProvider::new(token)
                .with_window(
                    i64::from(options.past_days()),
                    i64::from(options.future_days()),
                )
                .with_calendars(options.selected_calendars());
            if options.accepted_events_only {
                provider = provider.accepted_events_only();
            }
            if !options.flagged_mail {
                provider = provider.calendar_only();
            }
            if !options.imports_calendar() {
                provider = provider.flagged_mail_only();
            }
            Box::new(provider)
        }
        IntegrationKind::Todoist => {
            let token = credentials::todoist_token()?
                .ok_or("Connect Todoist in Settings before enabling import.")?;
            Box::new(providers::todoist::TodoistProvider::new(token))
        }
        #[cfg(feature = "private-workiq")]
        IntegrationKind::WorkIq => {
            let options = settings.workiq;
            let mut provider = providers::workiq::WorkIqProvider::new()
                .map_err(|error| format!("{error:#}"))?
                .with_window(
                    i64::from(options.past_days()),
                    i64::from(options.future_days()),
                );
            if options.accepted_events_only {
                provider = provider.accepted_events_only();
            }
            if !options.flagged_mail {
                provider = provider.calendar_only();
            }
            if !options.calendar {
                provider = provider.flagged_mail_only();
            }
            Box::new(provider)
        }
    };
    provider
        .ensure_authorized()
        .map_err(|error| format!("{error:#}"))?;
    provider.sync().map_err(|error| format!("{error:#}"))
}

fn require_credential(credential: IntegrationCredential, label: &str) -> Result<(), String> {
    if credential.is_stored()? {
        Ok(())
    } else {
        Err(format!(
            "Connect {label} in Settings before enabling import."
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fetch_belongs_to_its_workspace_and_provider_policy() {
        let mut scope = ImportScope {
            workspace: 4,
            ready: true,
            settings: IntegrationSettings::default(),
        };
        scope.settings.todoist.enabled = true;
        let mut changed = scope.clone();
        changed.workspace += 1;
        assert!(!scope.matches(&changed, IntegrationKind::Todoist));
        changed = scope.clone();
        changed.settings.todoist.enabled = false;
        assert!(!scope.matches(&changed, IntegrationKind::Todoist));
        changed = scope.clone();
        changed.settings.outlook.enabled = true;
        assert!(scope.matches(&changed, IntegrationKind::Todoist));
        changed.ready = false;
        assert!(!scope.matches(&changed, IntegrationKind::Todoist));
    }
}
