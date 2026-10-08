use serde::{Deserialize, Serialize};
use uuid::Uuid;

use chrono::{DateTime, NaiveDate, Utc};

use crate::{
    Action, ActionTemplate, Event, EventTemplate, Marker, MarkerTemplate, Routine, Signal,
    SignalTemplate,
};

pub(crate) mod relative_duration_iso8601 {
    use chronoutil::RelativeDuration;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(d: &RelativeDuration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&d.format_to_iso8601())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<RelativeDuration, D::Error> {
        RelativeDuration::parse_from_iso8601(&String::deserialize(d)?)
            .map_err(serde::de::Error::custom)
    }
}

pub(crate) mod relative_duration_iso8601_opt {
    use chronoutil::RelativeDuration;
    use serde::{Deserialize, Deserializer, Serializer};

    use super::relative_duration_iso8601;

    pub fn serialize<S: Serializer>(d: &Option<RelativeDuration>, s: S) -> Result<S::Ok, S::Error> {
        match d {
            Some(d) => relative_duration_iso8601::serialize(d, s),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<RelativeDuration>, D::Error> {
        match Option::<String>::deserialize(d)? {
            Some(s) => RelativeDuration::parse_from_iso8601(&s)
                .map(Some)
                .map_err(serde::de::Error::custom),
            None => Ok(None),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountInfo {
    pub account_id: Uuid,
    pub dataset_id: Uuid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountProfile {
    pub display_name: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateAccountProfile {
    pub display_name: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AllData {
    #[serde(default)]
    pub dataset_id: Uuid,
    #[serde(default)]
    pub seq: i64,
    pub actions: Vec<Action>,
    pub events: Vec<Event>,
    pub routines: Vec<Routine>,
    pub markers: Vec<Marker>,
    #[serde(default)]
    pub signals: Vec<Signal>,
    pub action_templates: Vec<ActionTemplate>,
    pub event_templates: Vec<EventTemplate>,
    #[serde(default)]
    pub marker_templates: Vec<MarkerTemplate>,
    #[serde(default)]
    pub signal_templates: Vec<SignalTemplate>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DataDelta {
    #[serde(default)]
    pub dataset_id: Uuid,
    pub seq: i64,
    pub actions: Vec<Action>,
    pub events: Vec<Event>,
    pub routines: Vec<Routine>,
    pub routine_order: Vec<Uuid>,
    pub markers: Vec<Marker>,
    pub signals: Vec<Signal>,
    pub action_templates: Vec<ActionTemplate>,
    pub event_templates: Vec<EventTemplate>,
    pub marker_templates: Vec<MarkerTemplate>,
    pub signal_templates: Vec<SignalTemplate>,
    pub tombstones: Tombstones,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Tombstones {
    pub actions: Vec<Uuid>,
    pub events: Vec<Uuid>,
    pub routines: Vec<Uuid>,
    pub markers: Vec<Uuid>,
    pub signals: Vec<Uuid>,
    pub action_templates: Vec<Uuid>,
    pub event_templates: Vec<Uuid>,
    pub marker_templates: Vec<Uuid>,
    pub signal_templates: Vec<Uuid>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ConvertEventToMarker {
    pub date: NaiveDate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_end_date: Option<NaiveDate>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompleteResult {
    pub completed: Action,
    pub next: Option<Action>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationEntry {
    pub provider_id: String,
    pub external_id: String,
    pub external_version: Option<String>,
    pub item_type: String,
    pub internal_uuid: Uuid,
    #[serde(default)]
    pub ignored: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChangeEvent {
    ActionsChanged,
    RoutinesChanged,
    EventsChanged,
    MarkersChanged,
    SignalsChanged,
    ActionTemplatesChanged,
    EventTemplatesChanged,
    MarkerTemplatesChanged,
    SignalTemplatesChanged,

    PipelineChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeBatch {
    pub seq: i64,
    pub changes: Vec<ChangeEvent>,
    #[serde(default)]
    pub reset: bool,
}

impl ChangeBatch {
    pub fn committed(seq: i64, changes: Vec<ChangeEvent>) -> Self {
        Self {
            seq,
            changes,
            reset: false,
        }
    }

    pub fn reset(seq: i64) -> Self {
        Self {
            seq,
            changes: Vec::new(),
            reset: true,
        }
    }
}
