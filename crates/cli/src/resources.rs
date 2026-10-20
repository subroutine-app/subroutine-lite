use std::collections::HashMap;

use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Days, Duration, Offset, Utc};
use chronoutil::RelativeDuration;
use serde_json::Value;
use subroutine_core::{
    AllData, CoreItem, OptimisticPatch, Recurrence, ResourceKey, ResourceValue, SchedulePoint,
    checked_duration_end, checked_duration_sum, ops,
};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Kind {
    Action,
    Event,
    Marker,
    Signal,
    Routine,
    ActionTemplate,
    EventTemplate,
    MarkerTemplate,
    SignalTemplate,
}

impl Kind {
    pub fn key(self, id: Uuid) -> ResourceKey {
        match self {
            Self::Action => ResourceKey::Action { id },
            Self::Event => ResourceKey::Event { id },
            Self::Marker => ResourceKey::Marker { id },
            Self::Signal => ResourceKey::Signal { id },
            Self::Routine => ResourceKey::Routine { id },
            Self::ActionTemplate => ResourceKey::ActionTemplate { id },
            Self::EventTemplate => ResourceKey::EventTemplate { id },
            Self::MarkerTemplate => ResourceKey::MarkerTemplate { id },
            Self::SignalTemplate => ResourceKey::SignalTemplate { id },
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Action => "action",
            Self::Event => "event",
            Self::Marker => "marker",
            Self::Signal => "signal",
            Self::Routine => "routine",
            Self::ActionTemplate => "action-template",
            Self::EventTemplate => "event-template",
            Self::MarkerTemplate => "marker-template",
            Self::SignalTemplate => "signal-template",
        }
    }
}

pub fn list(data: &AllData, kind: Option<Kind>) -> Vec<ResourceValue> {
    match kind {
        Some(Kind::Action) => data
            .actions
            .iter()
            .cloned()
            .map(ResourceValue::Action)
            .collect(),
        Some(Kind::Event) => data
            .events
            .iter()
            .cloned()
            .map(ResourceValue::Event)
            .collect(),
        Some(Kind::Marker) => data
            .markers
            .iter()
            .cloned()
            .map(ResourceValue::Marker)
            .collect(),
        Some(Kind::Signal) => data
            .signals
            .iter()
            .cloned()
            .map(ResourceValue::Signal)
            .collect(),
        Some(Kind::Routine) => data
            .routines
            .iter()
            .cloned()
            .map(ResourceValue::Routine)
            .collect(),
        Some(Kind::ActionTemplate) => data
            .action_templates
            .iter()
            .cloned()
            .map(ResourceValue::ActionTemplate)
            .collect(),
        Some(Kind::EventTemplate) => data
            .event_templates
            .iter()
            .cloned()
            .map(ResourceValue::EventTemplate)
            .collect(),
        Some(Kind::MarkerTemplate) => data
            .marker_templates
            .iter()
            .cloned()
            .map(ResourceValue::MarkerTemplate)
            .collect(),
        Some(Kind::SignalTemplate) => data
            .signal_templates
            .iter()
            .cloned()
            .map(ResourceValue::SignalTemplate)
            .collect(),
        None => <Kind as clap::ValueEnum>::value_variants()
            .iter()
            .flat_map(|kind| list(data, Some(*kind)))
            .collect(),
    }
}

pub fn get(data: &AllData, kind: Kind, id: Uuid) -> Result<ResourceValue> {
    match kind {
        Kind::Action => find(&data.actions, id).map(ResourceValue::Action),
        Kind::Event => find(&data.events, id).map(ResourceValue::Event),
        Kind::Marker => find(&data.markers, id).map(ResourceValue::Marker),
        Kind::Signal => find(&data.signals, id).map(ResourceValue::Signal),
        Kind::Routine => find(&data.routines, id).map(ResourceValue::Routine),
        Kind::ActionTemplate => find(&data.action_templates, id).map(ResourceValue::ActionTemplate),
        Kind::EventTemplate => find(&data.event_templates, id).map(ResourceValue::EventTemplate),
        Kind::MarkerTemplate => find(&data.marker_templates, id).map(ResourceValue::MarkerTemplate),
        Kind::SignalTemplate => find(&data.signal_templates, id).map(ResourceValue::SignalTemplate),
    }
    .with_context(|| format!("{} {id} not found", kind.name()))
}

fn find<T: CoreItem + Clone>(items: &[T], id: Uuid) -> Option<T> {
    items.iter().find(|item| item.id() == id).cloned()
}

pub fn decode(kind: Kind, value: Value) -> Result<ResourceValue> {
    ensure!(value.is_object(), "expected a {} object", kind.name());
    let resource = match kind {
        Kind::Action => serde_json::from_value(value.clone()).map(ResourceValue::Action),
        Kind::Event => serde_json::from_value(value.clone()).map(ResourceValue::Event),
        Kind::Marker => serde_json::from_value(value.clone()).map(ResourceValue::Marker),
        Kind::Signal => serde_json::from_value(value.clone()).map(ResourceValue::Signal),
        Kind::Routine => serde_json::from_value(value.clone()).map(ResourceValue::Routine),
        Kind::ActionTemplate => {
            serde_json::from_value(value.clone()).map(ResourceValue::ActionTemplate)
        }
        Kind::EventTemplate => {
            serde_json::from_value(value.clone()).map(ResourceValue::EventTemplate)
        }
        Kind::MarkerTemplate => {
            serde_json::from_value(value.clone()).map(ResourceValue::MarkerTemplate)
        }
        Kind::SignalTemplate => {
            serde_json::from_value(value.clone()).map(ResourceValue::SignalTemplate)
        }
    }
    .with_context(|| format!("invalid {}", kind.name()))?;
    let mut serialized = serde_json::to_value(&resource)?;
    let fields = &mut serialized["value"];
    if matches!(kind, Kind::Event | Kind::EventTemplate) {
        fields["source_busy"] = Value::Null;
        fields["busy_override"] = Value::Null;
    }
    if fields["recurrence"].is_object() {
        fields["recurrence"]["timezone"] = Value::Null;
    }
    check_keys(&value, fields, "$")?;
    Ok(resource)
}

fn check_keys(input: &Value, serialized: &Value, path: &str) -> Result<()> {
    match input {
        Value::Object(fields) => {
            let known = serialized
                .as_object()
                .with_context(|| format!("unexpected object at {path}"))?;
            for (key, value) in fields {
                let path = format!("{path}.{key}");
                let field = known
                    .get(key)
                    .with_context(|| format!("unknown field {path}"))?;
                check_keys(value, field, &path)?;
            }
        }
        Value::Array(values) => {
            let known = serialized
                .as_array()
                .with_context(|| format!("unexpected array at {path}"))?;
            for (index, (value, field)) in values.iter().zip(known).enumerate() {
                check_keys(value, field, &format!("{path}[{index}]"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn validate(resource: &ResourceValue, settings: ops::Settings) -> Result<()> {
    let item = core_item(resource);
    ensure!(!item.id().is_nil(), "resource id must not be nil");
    ensure!(!item.lineage_id().is_nil(), "lineage id must not be nil");
    ensure!(!item.title().trim().is_empty(), "title must not be blank");
    let references = match resource {
        ResourceValue::Action(value) => [value.routine_id, value.template_id],
        ResourceValue::Event(value) => [None, value.template_id],
        ResourceValue::Marker(value) => [None, value.template_id],
        ResourceValue::Signal(value) => [None, value.template_id],
        ResourceValue::Routine(_)
        | ResourceValue::ActionTemplate(_)
        | ResourceValue::EventTemplate(_)
        | ResourceValue::MarkerTemplate(_)
        | ResourceValue::SignalTemplate(_) => [None, None],
    };
    ensure!(
        references.into_iter().flatten().all(|id| !id.is_nil()),
        "referenced ids must not be nil"
    );
    let anchor = item.start().unwrap_or_else(SchedulePoint::now);
    let recurrence = match resource {
        ResourceValue::Action(value) => {
            duration(
                anchor,
                value
                    .duration
                    .unwrap_or(settings.schedule.default_action_duration),
            )?;
            value.recurrence
        }
        ResourceValue::Event(value) => {
            duration(anchor, value.duration)?;
            value.recurrence
        }
        ResourceValue::Marker(value) => {
            ensure!(
                value.end_date.is_none_or(|end| end >= value.date),
                "marker end date cannot precede its start date"
            );
            value.recurrence
        }
        ResourceValue::Signal(value) => value.recurrence,
        ResourceValue::Routine(value) => {
            ensure!(
                value.steps.iter().all(|step| !step.title.trim().is_empty()),
                "routine step titles must not be blank"
            );
            let durations = || {
                value
                    .steps
                    .iter()
                    .map(|step| step.duration.unwrap_or(settings.default_step_duration))
            };
            let total = checked_duration_sum(durations()).map_err(anyhow::Error::msg)?;
            duration(anchor, total)?;
            durations().try_fold(
                SchedulePoint::DateTime(DateTime::<Utc>::from(anchor)),
                duration,
            )?;
            value.recurrence
        }
        ResourceValue::ActionTemplate(value) => {
            if let Some(value) = value.duration {
                duration(anchor, value)?;
            }
            value.recurrence
        }
        ResourceValue::EventTemplate(value) => {
            duration(anchor, value.duration)?;
            value.recurrence
        }
        ResourceValue::MarkerTemplate(value) => {
            ensure!(value.span_days > 0, "marker span_days must be positive");
            ensure!(
                SchedulePoint::today()
                    .checked_add_days(Days::new(u64::from(value.span_days) - 1))
                    .is_some(),
                "marker span is outside the supported calendar range"
            );
            value.recurrence
        }
        ResourceValue::SignalTemplate(value) => value.recurrence,
    };
    if let Some(recurrence) = recurrence {
        validate_recurrence(recurrence, item.start())?;
    }
    Ok(())
}

fn duration(start: SchedulePoint, value: RelativeDuration) -> Result<SchedulePoint> {
    checked_duration_end(start, value).map_err(anyhow::Error::msg)
}

fn validate_recurrence(recurrence: Recurrence, start: Option<SchedulePoint>) -> Result<()> {
    serde_json::from_value::<Recurrence>(serde_json::to_value(recurrence)?)
        .context("invalid recurrence")?;
    let Some(start) = start else {
        return Ok(());
    };
    let date = match (start, recurrence.timezone) {
        (SchedulePoint::DateTime(datetime), Some(timezone)) => {
            let offset = datetime
                .with_timezone(&timezone)
                .offset()
                .fix()
                .local_minus_utc();
            datetime
                .naive_utc()
                .checked_add_signed(Duration::seconds(i64::from(offset)))
                .context(
                    "recurrence start is outside the supported calendar range in its timezone",
                )?
                .date()
        }
        _ => start.date_naive(),
    };
    ensure!(
        recurrence.end_date.is_none_or(|end| end >= date),
        "recurrence end date cannot precede its start date"
    );
    Ok(())
}

pub fn from_changes(changes: ops::Changes) -> OptimisticPatch {
    let mut writes = Vec::new();
    let mut positions = HashMap::new();
    for write in changes.writes() {
        let value = match write {
            ops::Write::Action(value) => ResourceValue::Action(value.clone()),
            ops::Write::Event(value) => ResourceValue::Event(value.clone()),
            ops::Write::Marker(value) => ResourceValue::Marker(value.clone()),
            ops::Write::Signal(value) => ResourceValue::Signal(value.clone()),
            ops::Write::Routine(value) => ResourceValue::Routine(value.clone()),
            ops::Write::ActionTemplate(value) => ResourceValue::ActionTemplate(value.clone()),
            ops::Write::EventTemplate(value) => ResourceValue::EventTemplate(value.clone()),
            ops::Write::MarkerTemplate(value) => ResourceValue::MarkerTemplate(value.clone()),
            ops::Write::SignalTemplate(value) => ResourceValue::SignalTemplate(value.clone()),
        };
        if let Some(&index) = positions.get(&value.key()) {
            writes[index] = value;
        } else {
            positions.insert(value.key(), writes.len());
            writes.push(value);
        }
    }
    let deletes = changes
        .deletes()
        .iter()
        .map(|delete| match *delete {
            ops::Delete::Action(id) => ResourceKey::Action { id },
            ops::Delete::Event(id) => ResourceKey::Event { id },
            ops::Delete::Marker(id) => ResourceKey::Marker { id },
            ops::Delete::Signal(id) => ResourceKey::Signal { id },
            ops::Delete::Routine(id) => ResourceKey::Routine { id },
            ops::Delete::ActionTemplate(id) => ResourceKey::ActionTemplate { id },
            ops::Delete::EventTemplate(id) => ResourceKey::EventTemplate { id },
            ops::Delete::MarkerTemplate(id) => ResourceKey::MarkerTemplate { id },
            ops::Delete::SignalTemplate(id) => ResourceKey::SignalTemplate { id },
        })
        .collect();
    OptimisticPatch {
        writes,
        deletes,
        routine_order: None,
    }
}

pub fn title(resource: &ResourceValue) -> &str {
    core_item(resource).title()
}

pub fn start(resource: &ResourceValue) -> Option<SchedulePoint> {
    core_item(resource).start()
}

pub fn kind(resource: &ResourceValue) -> Kind {
    match resource {
        ResourceValue::Action(_) => Kind::Action,
        ResourceValue::Event(_) => Kind::Event,
        ResourceValue::Marker(_) => Kind::Marker,
        ResourceValue::Signal(_) => Kind::Signal,
        ResourceValue::Routine(_) => Kind::Routine,
        ResourceValue::ActionTemplate(_) => Kind::ActionTemplate,
        ResourceValue::EventTemplate(_) => Kind::EventTemplate,
        ResourceValue::MarkerTemplate(_) => Kind::MarkerTemplate,
        ResourceValue::SignalTemplate(_) => Kind::SignalTemplate,
    }
}

fn core_item(resource: &ResourceValue) -> &dyn CoreItem {
    match resource {
        ResourceValue::Action(value) => value,
        ResourceValue::Event(value) => value,
        ResourceValue::Marker(value) => value,
        ResourceValue::Signal(value) => value,
        ResourceValue::Routine(value) => value,
        ResourceValue::ActionTemplate(value) => value,
        ResourceValue::EventTemplate(value) => value,
        ResourceValue::MarkerTemplate(value) => value,
        ResourceValue::SignalTemplate(value) => value,
    }
}
