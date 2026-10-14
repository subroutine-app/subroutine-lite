use std::collections::HashMap;

use rusqlite::Transaction;
use serde::Serialize;
use subroutine_core::{
    Action, AllData, Event, Marker, ResourceKey, ResourceValue, SchedulePoint, checked_duration_end,
};
use subroutine_import::provider::{Imported, SyncItem};
use uuid::Uuid;

use crate::{LocalStoreError, Result, db::rows::upsert_rows};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Action,
    Event,
    Marker,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Action => "action",
            Self::Event => "event",
            Self::Marker => "marker",
        }
    }

    pub fn key(self, id: Uuid) -> ResourceKey {
        match self {
            Self::Action => ResourceKey::Action { id },
            Self::Event => ResourceKey::Event { id },
            Self::Marker => ResourceKey::Marker { id },
        }
    }
}

#[derive(Clone, Serialize)]
pub(super) enum Resource {
    Action(Action),
    Event(Event),
    Marker(Marker),
}

impl Resource {
    pub fn imported(item: SyncItem) -> Imported<Self> {
        match item {
            SyncItem::Action(Imported {
                external_id,
                external_version,
                item,
            }) => Imported {
                external_id,
                external_version,
                item: Self::Action(item),
            },
            SyncItem::Event(Imported {
                external_id,
                external_version,
                item,
            }) => Imported {
                external_id,
                external_version,
                item: Self::Event(item),
            },
            SyncItem::Marker(Imported {
                external_id,
                external_version,
                item,
            }) => Imported {
                external_id,
                external_version,
                item: Self::Marker(item),
            },
        }
    }

    pub fn same_payload(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Action(Action {
                    id: _,
                    recurrence_id: _,
                    routine_id,
                    template_id,
                    title,
                    content,
                    queued,
                    pinned,
                    start,
                    duration,
                    completion,
                    recurrence,
                    source_provider,
                    source_external_id,
                }),
                Self::Action(other),
            ) => {
                routine_id == &other.routine_id
                    && template_id == &other.template_id
                    && title == &other.title
                    && content == &other.content
                    && queued == &other.queued
                    && pinned == &other.pinned
                    && start == &other.start
                    && duration == &other.duration
                    && completion == &other.completion
                    && recurrence == &other.recurrence
                    && source_provider == &other.source_provider
                    && source_external_id == &other.source_external_id
            }
            (
                Self::Event(Event {
                    id: _,
                    lineage_id: _,
                    template_id,
                    title,
                    content,
                    start,
                    duration,
                    recurrence,
                    source_provider,
                    source_external_id,
                    source_busy,
                    busy_override,
                }),
                Self::Event(other),
            ) => {
                template_id == &other.template_id
                    && title == &other.title
                    && content == &other.content
                    && start == &other.start
                    && duration == &other.duration
                    && recurrence == &other.recurrence
                    && source_provider == &other.source_provider
                    && source_external_id == &other.source_external_id
                    && source_busy == &other.source_busy
                    && busy_override == &other.busy_override
            }
            (
                Self::Marker(Marker {
                    id: _,
                    lineage_id: _,
                    template_id,
                    title,
                    content,
                    date,
                    end_date,
                    recurrence,
                    source_provider,
                    source_external_id,
                }),
                Self::Marker(other),
            ) => {
                template_id == &other.template_id
                    && title == &other.title
                    && content == &other.content
                    && date == &other.date
                    && end_date == &other.end_date
                    && recurrence == &other.recurrence
                    && source_provider == &other.source_provider
                    && source_external_id == &other.source_external_id
            }
            _ => false,
        }
    }

    pub fn kind(&self) -> Kind {
        match self {
            Self::Action(_) => Kind::Action,
            Self::Event(_) => Kind::Event,
            Self::Marker(_) => Kind::Marker,
        }
    }

    pub fn id(&self) -> Uuid {
        match self {
            Self::Action(value) => value.id,
            Self::Event(value) => value.id,
            Self::Marker(value) => value.id,
        }
    }

    fn lineage_id(&self) -> Uuid {
        match self {
            Self::Action(value) => value.recurrence_id,
            Self::Event(value) => value.lineage_id,
            Self::Marker(value) => value.lineage_id,
        }
    }

    fn source(&self) -> (Option<&str>, Option<&str>) {
        match self {
            Self::Action(value) => (
                value.source_provider.as_deref(),
                value.source_external_id.as_deref(),
            ),
            Self::Event(value) => (
                value.source_provider.as_deref(),
                value.source_external_id.as_deref(),
            ),
            Self::Marker(value) => (
                value.source_provider.as_deref(),
                value.source_external_id.as_deref(),
            ),
        }
    }

    pub fn reconcile(
        &mut self,
        id: Uuid,
        provider: &str,
        external_id: &str,
        previous: Option<&Self>,
        preserve_completion: bool,
    ) {
        let lineage_id = previous.map_or(id, Self::lineage_id);
        match self {
            Self::Action(value) => {
                value.id = id;
                value.recurrence_id = lineage_id;
                value.source_provider = Some(provider.into());
                value.source_external_id = Some(external_id.into());
                if preserve_completion && let Some(Self::Action(old)) = previous {
                    value.completion = old.completion.or(value.completion);
                }
            }
            Self::Event(value) => {
                value.id = id;
                value.lineage_id = lineage_id;
                value.source_provider = Some(provider.into());
                value.source_external_id = Some(external_id.into());
                if let Some(Self::Event(old)) = previous {
                    value.preserve_missing_availability(Some(old));
                    value.preserve_busy_override(Some(old));
                }
            }
            Self::Marker(value) => {
                value.id = id;
                value.lineage_id = lineage_id;
                value.source_provider = Some(provider.into());
                value.source_external_id = Some(external_id.into());
            }
        }
    }

    pub fn backfill_availability(&self, previous: Option<&Self>) -> Option<Self> {
        let (Self::Event(incoming), Some(Self::Event(previous))) = (self, previous) else {
            return None;
        };
        if previous.source_busy.is_some() {
            return None;
        }
        let source_busy = incoming.source_busy?;
        let mut event = previous.clone();
        event.source_busy = Some(source_busy);
        Some(Self::Event(event))
    }

    pub fn validate_timing(&self) -> Result<()> {
        let result = match self {
            Self::Action(action) => action.duration.map_or(Ok(()), |duration| {
                checked_duration_end(action.start.unwrap_or_else(SchedulePoint::now), duration)
                    .map(|_| ())
            }),
            Self::Event(event) => event.end_time().map(|_| ()),
            Self::Marker(marker) => {
                if marker.end_date.is_some_and(|end| end < marker.date) {
                    Err("the end date must not be before the start date")
                } else {
                    Ok(())
                }
            }
        };
        result.map_err(|error| {
            LocalStoreError::InvalidMutation(format!(
                "provider {} {}: {error}",
                self.kind().name(),
                self.id()
            ))
        })
    }

    pub fn upsert(&self, tx: &Transaction<'_>) -> Result<()> {
        match self {
            Self::Action(value) => {
                upsert_rows(tx, "action", std::slice::from_ref(value), |row| row.id)
            }
            Self::Event(value) => {
                upsert_rows(tx, "event", std::slice::from_ref(value), |row| row.id)
            }
            Self::Marker(value) => {
                upsert_rows(tx, "marker", std::slice::from_ref(value), |row| row.id)
            }
        }
    }

    pub fn into_value(self) -> ResourceValue {
        match self {
            Self::Action(value) => ResourceValue::Action(value),
            Self::Event(value) => ResourceValue::Event(value),
            Self::Marker(value) => ResourceValue::Marker(value),
        }
    }
}

pub(super) struct Resources {
    by_key: HashMap<ResourceKey, Resource>,
    by_source: HashMap<String, Vec<ResourceKey>>,
}

impl Resources {
    pub fn new(data: AllData, provider: &str) -> Self {
        let mut by_key = HashMap::new();
        let mut by_source = HashMap::<String, Vec<ResourceKey>>::new();
        for resource in data
            .actions
            .into_iter()
            .map(Resource::Action)
            .chain(data.events.into_iter().map(Resource::Event))
            .chain(data.markers.into_iter().map(Resource::Marker))
        {
            let key = resource.kind().key(resource.id());
            if let (Some(source), Some(external_id)) = resource.source()
                && source == provider
            {
                by_source.entry(external_id.into()).or_default().push(key);
            }
            by_key.insert(key, resource);
        }
        Self { by_key, by_source }
    }

    pub fn get(&self, kind: Kind, id: Uuid) -> Option<&Resource> {
        self.by_key.get(&kind.key(id))
    }

    pub fn adopt(&self, external_id: &str) -> Result<Option<&Resource>> {
        match self.by_source.get(external_id).map(Vec::as_slice) {
            None | Some([]) => Ok(None),
            Some([key]) => Ok(self.by_key.get(key)),
            Some(_) => Err(LocalStoreError::InvalidMutation(format!(
                "multiple existing resources have provider identity {external_id}; import left unchanged"
            ))),
        }
    }
}
