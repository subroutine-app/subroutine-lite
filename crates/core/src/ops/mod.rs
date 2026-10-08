
pub mod actions;
pub mod pipeline;
pub mod recurrence;
pub mod routines;

mod change;
mod item;
mod settings;

pub use change::{Changes, Delete, Outcome, Write, put};
pub use item::{
    Identify, Templated, convert_event_to_marker, convert_event_to_marker_with_id, create,
    save_as_template, validate_update,
};
pub use settings::Settings;

use chrono::{DateTime, Local, Utc};
use uuid::Uuid;

use crate::{Action, Event, Marker, PipelineContext, Routine, Signal};

#[derive(Debug)]
pub enum OpError {
    NotFound { kind: &'static str, id: Uuid },
    Rejected(String),
}

impl OpError {
    pub fn not_found(kind: &'static str, id: Uuid) -> Self {
        Self::NotFound { kind, id }
    }

    pub fn rejected(reason: impl Into<String>) -> Self {
        Self::Rejected(reason.into())
    }
}

impl std::fmt::Display for OpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound { kind, id } => write!(f, "{kind} {id} not found"),
            Self::Rejected(reason) => f.write_str(reason),
        }
    }
}

pub type OpResult<T> = Result<T, OpError>;

pub struct Snapshot {
    pub now: DateTime<Local>,
    pub settings: Settings,
    pub actions: Vec<Action>,
    pub events: Vec<Event>,
    pub routines: Vec<Routine>,
    pub markers: Vec<Marker>,
    pub signals: Vec<Signal>,
}

impl Snapshot {
    pub fn new(
        now: DateTime<Local>,
        settings: Settings,
        actions: Vec<Action>,
        events: Vec<Event>,
        routines: Vec<Routine>,
        markers: Vec<Marker>,
        signals: Vec<Signal>,
    ) -> Self {
        Self {
            now,
            settings,
            actions: actions.into_iter().filter(|a| !a.is_completed()).collect(),
            events,
            routines,
            markers,
            signals,
        }
    }

    pub fn now_utc(&self) -> DateTime<Utc> {
        self.now.with_timezone(&Utc)
    }

    pub fn context(&self) -> PipelineContext<'_> {
        self.context_with(&self.actions)
    }

    pub fn context_with<'a>(&'a self, actions: &'a [Action]) -> PipelineContext<'a> {
        PipelineContext::new(
            self.now,
            self.settings.schedule,
            actions,
            &self.events,
            &self.routines,
            &self.markers,
            &self.signals,
        )
    }

    pub fn action(&self, id: Uuid) -> OpResult<&Action> {
        self.actions
            .iter()
            .find(|a| a.id == id)
            .ok_or_else(|| OpError::not_found("action", id))
    }
}
