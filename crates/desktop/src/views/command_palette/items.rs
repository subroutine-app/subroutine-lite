use chrono::{DateTime, Utc};
use gpui::{Context, prelude::FluentBuilder as _};
use gpui_kit::{
    display::empty::{EmptyKind, EmptyState},
    overlay::Command,
};
use subroutine_core::{ActionTemplate, AnyItem, CoreItem, EventTemplate, Routine};
use uuid::Uuid;

use crate::{
    keys::ConfigurableCommand,
    stores::{AppDatabaseStore, StoreStatus, SyncStatus},
};

const EMPTY_ROUTINE: &str = "This routine has no steps yet.";
const INVALID_ROW: &str = "That item can’t be used here.";
const MISSING_ROW: &str = "That item no longer exists.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LibraryCommand {
    StartRoutine,
    UseSavedAction,
    UseSavedEvent,
}

impl LibraryCommand {
    pub(super) fn from_command(command: ConfigurableCommand) -> Option<Self> {
        match command {
            ConfigurableCommand::StartRoutine => Some(Self::StartRoutine),
            ConfigurableCommand::UseSavedAction => Some(Self::UseSavedAction),
            ConfigurableCommand::UseSavedEvent => Some(Self::UseSavedEvent),
            _ => None,
        }
    }

    pub(super) fn configurable_command(self) -> ConfigurableCommand {
        match self {
            Self::StartRoutine => ConfigurableCommand::StartRoutine,
            Self::UseSavedAction => ConfigurableCommand::UseSavedAction,
            Self::UseSavedEvent => ConfigurableCommand::UseSavedEvent,
        }
    }

    pub(super) fn title(self) -> &'static str {
        match self {
            Self::StartRoutine => "Start routine",
            Self::UseSavedAction => "Use saved action",
            Self::UseSavedEvent => "Use saved event",
        }
    }

    pub(super) fn placeholder(self) -> &'static str {
        match self {
            Self::StartRoutine => "Search routines…",
            Self::UseSavedAction => "Search saved actions…",
            Self::UseSavedEvent => "Search saved events…",
        }
    }

    pub(super) fn commands(self, store: &AppDatabaseStore) -> Vec<Command> {
        if !store.is_ready() {
            return Vec::new();
        }
        match self {
            Self::StartRoutine => store
                .routines()
                .iter()
                .filter_map(routine_command)
                .collect(),
            Self::UseSavedAction => store
                .action_templates()
                .iter()
                .map(|template| self.command(template))
                .collect(),
            Self::UseSavedEvent => store
                .event_templates()
                .iter()
                .map(|template| self.command(template))
                .collect(),
        }
    }

    pub(super) fn empty_state(self, store: &AppDatabaseStore, has_items: bool) -> EmptyState {
        let (kind, title, detail) = self.empty_copy(&store.status(), has_items);
        EmptyState::new(self.empty_id(), title)
            .kind(kind)
            .when_some(detail, |empty, detail| empty.detail(detail))
    }

    pub(super) fn invoke(
        self,
        row_id: &str,
        store: &mut AppDatabaseStore,
        cx: &mut Context<AppDatabaseStore>,
    ) -> Result<(), &'static str> {
        if let Some(reason) = not_ready_reason(&store.status()) {
            return Err(reason);
        }
        let accepted = match self {
            Self::StartRoutine => {
                let id = resolve_routine(row_id, store.routines())?;
                store.try_instantiate_routine(id, None, cx)
            }
            Self::UseSavedAction => {
                let item = build_action(row_id, &store.action_templates())?;
                store.try_create_items(vec![item], cx)
            }
            Self::UseSavedEvent => {
                let item = build_event(row_id, &store.event_templates(), Utc::now())?;
                store.try_create_items(vec![item], cx)
            }
        };
        acceptance_result(accepted)
    }

    fn prefix(self) -> &'static str {
        match self {
            Self::StartRoutine => "routine:",
            Self::UseSavedAction => "action-template:",
            Self::UseSavedEvent => "event-template:",
        }
    }

    fn row_id(self, id: Uuid) -> String {
        format!("{}{id}", self.prefix())
    }

    fn command(self, item: &impl CoreItem) -> Command {
        let title = if item.title().trim().is_empty() {
            match self {
                Self::StartRoutine => "Untitled routine",
                Self::UseSavedAction => "Untitled saved action",
                Self::UseSavedEvent => "Untitled saved event",
            }
        } else {
            item.title()
        };
        Command::new(self.row_id(item.id()), title.to_owned())
    }

    fn resolve<'a, T: CoreItem>(self, row_id: &str, items: &'a [T]) -> Result<&'a T, &'static str> {
        let id = row_id
            .strip_prefix(self.prefix())
            .and_then(|id| Uuid::parse_str(id).ok())
            .ok_or(INVALID_ROW)?;
        items.iter().find(|item| item.id() == id).ok_or(MISSING_ROW)
    }

    fn empty_id(self) -> &'static str {
        match self {
            Self::StartRoutine => "command-palette.start-routine.empty",
            Self::UseSavedAction => "command-palette.use-saved-action.empty",
            Self::UseSavedEvent => "command-palette.use-saved-event.empty",
        }
    }

    fn empty_copy(
        self,
        status: &StoreStatus,
        has_items: bool,
    ) -> (EmptyKind, &'static str, Option<&'static str>) {
        match status {
            StoreStatus::NotConfigured => (EmptyKind::Unavailable, "Library unavailable", None),
            StoreStatus::AuthenticationRequired => {
                (EmptyKind::Unauthorized, "Sign-in required", None)
            }
            StoreStatus::Error(_) => (EmptyKind::Failed, "Couldn’t load library", None),
            StoreStatus::Ready if has_items => (EmptyKind::Empty, "No matches", None),
            StoreStatus::Ready => match self {
                Self::StartRoutine => (
                    EmptyKind::Empty,
                    "No routines available",
                    Some("Create a routine with at least one step in Routines."),
                ),
                Self::UseSavedAction => (
                    EmptyKind::Empty,
                    "No saved actions yet",
                    Some("Right-click an action and choose Save for reuse."),
                ),
                Self::UseSavedEvent => (
                    EmptyKind::Empty,
                    "No saved events yet",
                    Some("Right-click an event and choose Save for reuse."),
                ),
            },
        }
    }
}

pub(super) fn warning(store: &AppDatabaseStore) -> Option<&'static str> {
    sync_warning(&store.status(), &store.sync_status())
}

fn acceptance_result(accepted: bool) -> Result<(), &'static str> {
    if accepted {
        Ok(())
    } else {
        Err("Couldn’t add this item. Try again.")
    }
}

fn not_ready_reason(status: &StoreStatus) -> Option<&'static str> {
    match status {
        StoreStatus::Ready => None,
        StoreStatus::NotConfigured => Some("Your data isn’t available right now."),
        StoreStatus::AuthenticationRequired => Some("Sign in to see your library."),
        StoreStatus::Error(_) => Some("Couldn’t load your library. Try again in a moment."),
    }
}

fn sync_warning(status: &StoreStatus, sync: &SyncStatus) -> Option<&'static str> {
    if not_ready_reason(status).is_some() {
        return None;
    }
    match sync {
        SyncStatus::Idle | SyncStatus::Syncing => None,
        SyncStatus::Offline => Some("Offline — recent changes may be missing."),
        SyncStatus::AuthenticationRequired => {
            Some("Sign in to sync — recent changes may be missing.")
        }
    }
}

fn routine_command(routine: &Routine) -> Option<Command> {
    (!routine.steps.is_empty()).then(|| LibraryCommand::StartRoutine.command(routine))
}

fn resolve_routine(row_id: &str, routines: &[Routine]) -> Result<Uuid, &'static str> {
    let routine = LibraryCommand::StartRoutine.resolve(row_id, routines)?;
    if routine.steps.is_empty() {
        return Err(EMPTY_ROUTINE);
    }
    Ok(routine.id)
}

fn build_action(row_id: &str, templates: &[ActionTemplate]) -> Result<AnyItem, &'static str> {
    let template = LibraryCommand::UseSavedAction.resolve(row_id, templates)?;
    Ok(AnyItem::Action(template.clone().build()))
}

fn build_event(
    row_id: &str,
    templates: &[EventTemplate],
    now: DateTime<Utc>,
) -> Result<AnyItem, &'static str> {
    let template = LibraryCommand::UseSavedEvent.resolve(row_id, templates)?;
    Ok(AnyItem::Event(template.clone().build(now)))
}
