use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    Action, ActionTemplate, Event, EventTemplate, Marker, MarkerTemplate, Routine, Signal,
    SignalTemplate,
};

pub const MUTATION_PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutationRequest {
    pub protocol_version: u16,
    pub dataset_id: Uuid,
    pub mutation_id: Uuid,
    pub client_id: Uuid,
    pub base_seq: i64,
    pub operation: MutationOperation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MutationOperation {
    UpsertAction {
        action: Box<Action>,
    },
    UpsertActions {
        actions: Vec<Action>,
    },
    UpsertResources {
        resources: Vec<ResourceValue>,
    },

    SetEventBusyOverride {
        event_id: Uuid,
        busy_override: Option<bool>,
    },
    DeleteResources {
        resources: Vec<ResourceKey>,
    },
    ReorderRoutines {
        routine_ids: Vec<Uuid>,
    },
    CompleteAction {
        action_id: Uuid,
        completed_at: DateTime<Utc>,
    },
    DeleteAction {
        action_id: Uuid,
    },
    ConvertEventToMarker {
        event_id: Uuid,
        marker_id: Uuid,
        date: NaiveDate,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        local_end_date: Option<NaiveDate>,
    },
}

impl MutationOperation {
    pub fn name(&self) -> &'static str {
        match self {
            Self::UpsertAction { .. } => "upsert_action",
            Self::UpsertActions { .. } => "upsert_actions",
            Self::UpsertResources { .. } => "upsert_resources",

            Self::SetEventBusyOverride { .. } => "set_event_busy_override",
            Self::DeleteResources { .. } => "delete_resources",
            Self::ReorderRoutines { .. } => "reorder_routines",
            Self::CompleteAction { .. } => "complete_action",
            Self::DeleteAction { .. } => "delete_action",
            Self::ConvertEventToMarker { .. } => "convert_event_to_marker",
        }
    }
}

impl MutationRequest {
    pub fn is_compatibility_rejection(&self, error: &ApiErrorBody) -> bool {
        if error.mutation_id.is_some_and(|id| id != self.mutation_id)
            || error
                .current_dataset_id
                .is_some_and(|id| id != self.dataset_id)
        {
            return false;
        }
        match error.error {
            ApiErrorCode::UnsupportedProtocol => true,
            ApiErrorCode::ValidationFailed => {
                let message = error
                    .message
                    .strip_prefix("Failed to deserialize the JSON body into the target type: ")
                    .unwrap_or(&error.message);
                message.starts_with(&format!(
                    "operation.type: unknown variant `{}`, expected ",
                    self.operation.name()
                ))
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientMutation {
    pub request: MutationRequest,
    pub optimistic_patch: OptimisticPatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResourceKey {
    Action { id: Uuid },
    Event { id: Uuid },
    Routine { id: Uuid },
    Marker { id: Uuid },
    Signal { id: Uuid },
    ActionTemplate { id: Uuid },
    EventTemplate { id: Uuid },
    MarkerTemplate { id: Uuid },
    SignalTemplate { id: Uuid },
}

impl ResourceKey {
    pub fn id(self) -> Uuid {
        match self {
            Self::Action { id }
            | Self::Event { id }
            | Self::Routine { id }
            | Self::Marker { id }
            | Self::Signal { id }
            | Self::ActionTemplate { id }
            | Self::EventTemplate { id }
            | Self::MarkerTemplate { id }
            | Self::SignalTemplate { id } => id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ResourceValue {
    Action(Action),
    Event(Event),
    Routine(Routine),
    Marker(Marker),
    Signal(Signal),
    ActionTemplate(ActionTemplate),
    EventTemplate(EventTemplate),
    MarkerTemplate(MarkerTemplate),
    SignalTemplate(SignalTemplate),
}

impl ResourceValue {
    pub fn key(&self) -> ResourceKey {
        match self {
            Self::Action(value) => ResourceKey::Action { id: value.id },
            Self::Event(value) => ResourceKey::Event { id: value.id },
            Self::Routine(value) => ResourceKey::Routine { id: value.id },
            Self::Marker(value) => ResourceKey::Marker { id: value.id },
            Self::Signal(value) => ResourceKey::Signal { id: value.id },
            Self::ActionTemplate(value) => ResourceKey::ActionTemplate { id: value.id },
            Self::EventTemplate(value) => ResourceKey::EventTemplate { id: value.id },
            Self::MarkerTemplate(value) => ResourceKey::MarkerTemplate { id: value.id },
            Self::SignalTemplate(value) => ResourceKey::SignalTemplate { id: value.id },
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OptimisticPatch {
    pub writes: Vec<ResourceValue>,
    pub deletes: Vec<ResourceKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routine_order: Option<Vec<Uuid>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationEffect {
    Applied,
    NoOp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MutationResult {
    ActionUpserted {
        action: Box<Action>,
    },
    ActionsUpserted {
        actions: Vec<Action>,
    },
    ResourcesUpserted {
        resources: Vec<ResourceValue>,
    },
    ResourcesDeleted {
        resources: Vec<ResourceKey>,
    },
    RoutinesReordered {
        routine_ids: Vec<Uuid>,
    },
    ActionCompleted {
        completed: Box<Action>,
        next: Option<Box<Action>>,
    },
    ActionDeleted {
        action_id: Uuid,
    },
    EventConvertedToMarker {
        event_id: Uuid,
        marker: Box<crate::Marker>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutationReceipt {
    pub protocol_version: u16,
    pub dataset_id: Uuid,
    pub mutation_id: Uuid,
    pub client_id: Uuid,
    pub base_seq: i64,
    pub commit_seq: i64,
    pub effect: MutationEffect,
    pub replayed: bool,
    pub result: MutationResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorCode {
    MissingToken,
    InvalidToken,
    InsufficientScope,
    WrongTenant,
    AuthenticationUnavailable,
    UnsupportedProtocol,
    ValidationFailed,
    ResourceNotFound,
    DatasetMismatch,
    StaleBase,
    MutationIdReuse,
    DomainConflict,
    TransientFailure,
    InternalFailure,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiErrorBody {
    pub error: ApiErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mutation_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<ResourceKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_seq: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_dataset_id: Option<Uuid>,
    pub retryable: bool,
}
