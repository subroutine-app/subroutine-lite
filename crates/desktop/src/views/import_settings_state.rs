use std::collections::HashMap;

use gpui::{App, AppContext as _, BorrowAppContext as _, Global, Task};
use subroutine_import::provider::CalendarSource;

use crate::{
    imports::{
        credentials::{self, IntegrationCredential, OAuthProvider},
        status::{IntegrationKind, IntegrationState, IntegrationStatuses},
        sync,
    },
    settings::Settings,
};

pub(super) const KINDS: &[IntegrationKind] = &[
    #[cfg(target_os = "macos")]
    IntegrationKind::EventKit,
    IntegrationKind::GoogleCalendar,
    IntegrationKind::Outlook,
    IntegrationKind::Todoist,
    #[cfg(feature = "private-workiq")]
    IntegrationKind::WorkIq,
];

#[derive(Clone, Default, PartialEq, Eq)]
pub(super) enum Connection {
    #[default]
    Unknown,
    Disconnected,
    Stored,
    Environment,
    Error(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Operation {
    WaitingToCheck,
    Check,
    Connect,
    Disconnect,
    Calendars,
}

impl Operation {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::WaitingToCheck => "Waiting for import to finish…",
            Self::Check => "Checking connection…",
            Self::Connect => "Connecting…",
            Self::Disconnect => "Disconnecting…",
            Self::Calendars => "Loading calendars…",
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct ProviderState {
    pub connection: Connection,
    pub operation: Option<Operation>,
    pub feedback: Option<Result<String, String>>,
    pub calendars: Option<Vec<CalendarSource>>,
}

impl ProviderState {
    pub(super) fn can_disable(&self) -> bool {
        !matches!(
            self.operation,
            Some(Operation::Connect | Operation::Disconnect)
        )
    }

    pub(super) fn operation_in_progress(&self) -> bool {
        matches!(
            self.operation,
            Some(
                Operation::Check
                    | Operation::Connect
                    | Operation::Disconnect
                    | Operation::Calendars
            )
        )
    }

    fn queue_check(&mut self) {
        if self.operation.is_none() {
            self.operation = Some(Operation::WaitingToCheck);
        }
    }

    fn begin_check(&mut self, status: &IntegrationState, running: bool) -> bool {
        if self.operation != Some(Operation::WaitingToCheck)
            || running
            || *status == IntegrationState::Syncing
        {
            return false;
        }
        self.operation = Some(Operation::Check);
        self.feedback = None;
        true
    }
}

#[derive(Default)]
struct ImportConnections(HashMap<IntegrationKind, ProviderState>);
impl Global for ImportConnections {}

pub(super) fn get(cx: &App, kind: IntegrationKind) -> ProviderState {
    cx.try_global::<ImportConnections>()
        .and_then(|states| states.0.get(&kind))
        .cloned()
        .unwrap_or_default()
}

fn update<R>(cx: &mut App, kind: IntegrationKind, f: impl FnOnce(&mut ProviderState) -> R) -> R {
    if cx.try_global::<ImportConnections>().is_none() {
        cx.set_global(ImportConnections::default());
        cx.observe_global::<IntegrationStatuses>(resume_checks)
            .detach();
    }
    let result =
        cx.update_global::<ImportConnections, _>(|states, _| f(states.0.entry(kind).or_default()));
    cx.refresh_windows();
    result
}

pub(super) fn sync_in_flight(cx: &App, kind: IntegrationKind) -> bool {
    sync::is_running(kind, cx)
        || IntegrationStatuses::get(cx, kind).state == IntegrationState::Syncing
}

pub(super) fn busy(cx: &App, kind: IntegrationKind) -> bool {
    get(cx, kind).operation.is_some() || sync_in_flight(cx, kind)
}

pub(super) fn any_operation_in_progress(cx: &App) -> bool {
    KINDS
        .iter()
        .any(|kind| get(cx, *kind).operation_in_progress())
}

pub(super) fn credential(kind: IntegrationKind) -> Option<IntegrationCredential> {
    match kind {
        IntegrationKind::GoogleCalendar => Some(IntegrationCredential::Google),
        IntegrationKind::Outlook => Some(IntegrationCredential::Microsoft),
        IntegrationKind::Todoist => Some(IntegrationCredential::Todoist),
        #[cfg(target_os = "macos")]
        IntegrationKind::EventKit => None,
        #[cfg(feature = "private-workiq")]
        IntegrationKind::WorkIq => None,
    }
}

pub(super) fn registration(kind: IntegrationKind) -> Option<OAuthProvider> {
    match kind {
        IntegrationKind::GoogleCalendar => OAuthProvider::google(),
        IntegrationKind::Outlook => OAuthProvider::microsoft(),
        _ => None,
    }
}

fn connection(kind: IntegrationKind) -> Result<Connection, String> {
    let credential = credential(kind).ok_or("This provider manages its own connection.")?;
    if credential.is_stored()? {
        return Ok(Connection::Stored);
    }
    if kind == IntegrationKind::Todoist && credentials::todoist_token_is_from_environment()? {
        return Ok(Connection::Environment);
    }
    Ok(Connection::Disconnected)
}

enum Outcome {
    Connection(Connection),
    Calendars(Vec<CalendarSource>),
}

fn run(
    kind: IntegrationKind,
    operation: Operation,
    work: impl FnOnce() -> Result<Outcome, String> + Send + 'static,
    cx: &mut App,
) -> Option<Task<bool>> {
    if busy(cx, kind) {
        return None;
    }
    update(cx, kind, |state| {
        state.operation = Some(operation);
        state.feedback = None;
    });
    if operation == Operation::Disconnect {
        Settings::update(cx, |settings| set_enabled(settings, kind, false));
        IntegrationStatuses::set(cx, kind, IntegrationState::Inactive);
    }
    Some(execute(kind, operation, work, cx))
}

fn execute(
    kind: IntegrationKind,
    operation: Operation,
    work: impl FnOnce() -> Result<Outcome, String> + Send + 'static,
    cx: &mut App,
) -> Task<bool> {
    cx.spawn(async move |cx| {
        let result = cx.background_spawn(async move { work() }).await;
        let succeeded = result.is_ok();
        cx.update(|cx| {
            if succeeded && matches!(operation, Operation::Connect | Operation::Disconnect) {
                Settings::update(cx, |settings| match kind {
                    IntegrationKind::GoogleCalendar => settings.integrations.google_calendar.reset_calendars(),
                    IntegrationKind::Outlook => settings.integrations.outlook.reset_calendars(),
                    _ => {}
                });
                IntegrationStatuses::set(cx, kind, IntegrationState::Inactive);
            }
            update(cx, kind, |state| {
                state.operation = None;
                match result {
                    Ok(Outcome::Connection(connection)) => {
                        state.connection = connection;
                        if matches!(operation, Operation::Connect | Operation::Disconnect) {
                            state.calendars = None;
                        }
                        state.feedback = match operation {
                            Operation::Connect => Some(Ok("Connected. Choose what to import, then enable import.".into())),
                            Operation::Disconnect => Some(Ok(if state.connection == Connection::Environment {
                                "Saved token removed. TODOIST_API_KEY is still available; import is off. Existing items are kept."
                            } else {
                                "Disconnected. Existing imported items are kept."
                            }.into())),
                            _ => None,
                        };
                    }
                    Ok(Outcome::Calendars(calendars)) => state.calendars = Some(calendars),
                    Err(error) => {
                        if operation == Operation::Check { state.connection = Connection::Error(error.clone()); }
                        state.feedback = Some(Err(error));
                    }
                }
            });
        });
        succeeded
    })
}

pub(super) fn check(kind: IntegrationKind, cx: &mut App) {
    if credential(kind).is_none() || get(cx, kind).operation.is_some() {
        return;
    }
    update(cx, kind, ProviderState::queue_check);
    resume_checks(cx);
}

fn resume_checks(cx: &mut App) {
    for &kind in KINDS {
        if get(cx, kind).operation != Some(Operation::WaitingToCheck) {
            continue;
        }
        let status = IntegrationStatuses::get(cx, kind).state;
        let running = sync::is_running(kind, cx);
        if update(cx, kind, |state| state.begin_check(&status, running)) {
            execute(
                kind,
                Operation::Check,
                move || connection(kind).map(Outcome::Connection),
                cx,
            )
            .detach();
        }
    }
}

pub(super) fn connect(
    kind: IntegrationKind,
    token: Option<String>,
    cx: &mut App,
) -> Option<Task<bool>> {
    if busy(cx, kind) {
        return None;
    }
    Settings::update(cx, |settings| set_enabled(settings, kind, false));
    let provider = registration(kind);
    run(
        kind,
        Operation::Connect,
        move || {
            if kind == IntegrationKind::Todoist {
                credentials::connect_todoist_blocking(token.as_deref().unwrap_or_default())?;
            } else {
                credentials::connect_blocking(provider)?;
            }
            Ok(Outcome::Connection(Connection::Stored))
        },
        cx,
    )
}

pub(super) fn disconnect(kind: IntegrationKind, cx: &mut App) {
    let Some(credential) = credential(kind) else {
        return;
    };
    if let Some(task) = run(
        kind,
        Operation::Disconnect,
        move || {
            credentials::disconnect(credential)?;
            connection(kind).map(Outcome::Connection)
        },
        cx,
    ) {
        task.detach();
    }
}

pub(super) fn load_calendars(kind: IntegrationKind, cx: &mut App) {
    if let Some(task) = run(
        kind,
        Operation::Calendars,
        move || {
            match kind {
                IntegrationKind::GoogleCalendar => credentials::google_calendars(),
                IntegrationKind::Outlook => credentials::outlook_calendars(),
                _ => Err("This provider has no calendar picker.".into()),
            }
            .map(Outcome::Calendars)
        },
        cx,
    ) {
        task.detach();
    }
}

pub(super) fn enabled(settings: &Settings, kind: IntegrationKind) -> bool {
    match kind {
        #[cfg(target_os = "macos")]
        IntegrationKind::EventKit => settings.integrations.eventkit.enabled,
        IntegrationKind::GoogleCalendar => settings.integrations.google_calendar.enabled,
        IntegrationKind::Outlook => settings.integrations.outlook.enabled,
        IntegrationKind::Todoist => settings.integrations.todoist.enabled,
        #[cfg(feature = "private-workiq")]
        IntegrationKind::WorkIq => settings.integrations.workiq.enabled,
    }
}

pub(super) fn set_enabled(settings: &mut Settings, kind: IntegrationKind, enabled: bool) {
    match kind {
        #[cfg(target_os = "macos")]
        IntegrationKind::EventKit => settings.integrations.eventkit.enabled = enabled,
        IntegrationKind::GoogleCalendar => settings.integrations.google_calendar.enabled = enabled,
        IntegrationKind::Outlook => settings.integrations.outlook.enabled = enabled,
        IntegrationKind::Todoist => settings.integrations.todoist.enabled = enabled,
        #[cfg(feature = "private-workiq")]
        IntegrationKind::WorkIq => settings.integrations.workiq.enabled = enabled,
    }
}

pub(super) fn imports_anything(settings: &Settings, kind: IntegrationKind) -> bool {
    match kind {
        #[cfg(target_os = "macos")]
        IntegrationKind::EventKit => settings.integrations.eventkit.imports_anything(),
        IntegrationKind::GoogleCalendar => settings.integrations.google_calendar.imports_anything(),
        IntegrationKind::Outlook => settings.integrations.outlook.imports_anything(),
        IntegrationKind::Todoist => settings.integrations.todoist.enabled,
        #[cfg(feature = "private-workiq")]
        IntegrationKind::WorkIq => settings.integrations.workiq.imports_anything(),
    }
}
