use serde::{Deserialize, Serialize};
use uuid::Uuid;

use chrono::{DateTime, Duration, Months, NaiveDate, Utc};
use chronoutil::RelativeDuration;

use crate::{
    Action, ActionTemplate, Event, EventTemplate, Marker, MarkerTemplate, Routine, SchedulePoint,
    Signal, SignalTemplate,
};

pub fn parse_iso8601_duration(input: &str) -> Result<RelativeDuration, String> {
    DurationParts::parse(input)
        .map(DurationParts::relative)
        .map_err(|reason| format!("invalid ISO-8601 duration {input:?}: {reason}"))
}

pub fn checked_duration_end(
    start: SchedulePoint,
    duration: RelativeDuration,
) -> Result<SchedulePoint, &'static str> {
    DurationParts::parse(&duration.format_to_iso8601())?.end(start)
}

pub fn duration_lookback(duration: RelativeDuration) -> Result<Duration, &'static str> {
    let parts = DurationParts::parse(&duration.format_to_iso8601())?;
    Duration::try_days(i64::from(parts.months) * 31)
        .and_then(|calendar| calendar.checked_add(&parts.fixed))
        .ok_or("duration is out of range")
}

pub fn checked_duration_sum(
    durations: impl IntoIterator<Item = RelativeDuration>,
) -> Result<RelativeDuration, &'static str> {
    let total = durations.into_iter().try_fold(
        DurationParts {
            months: 0,
            fixed: Duration::zero(),
        },
        |total, duration| -> Result<DurationParts, &'static str> {
            let part = DurationParts::parse(&duration.format_to_iso8601())?;
            Ok(DurationParts {
                months: total
                    .months
                    .checked_add(part.months)
                    .ok_or("total months are out of range")?,
                fixed: total
                    .fixed
                    .checked_add(&part.fixed)
                    .ok_or("total duration is out of range")?,
            })
        },
    )?;
    total.end(SchedulePoint::DateTime(DateTime::<Utc>::MIN_UTC))?;
    Ok(total.relative())
}

struct DurationParts {
    months: i32,
    fixed: Duration,
}

impl DurationParts {
    fn parse(input: &str) -> Result<Self, &'static str> {
        if input.contains('-') {
            return Err("duration cannot contain negative components");
        }
        let input = input
            .strip_prefix('P')
            .ok_or("duration must start with P")?;
        let (mut date, mut time) = input.split_once('T').unwrap_or((input, ""));
        let years = duration_component(&mut date, 'Y')?;
        let months = duration_component(&mut date, 'M')?;
        let weeks = duration_component(&mut date, 'W')?;
        let days = duration_component(&mut date, 'D')?;
        let hours = duration_component(&mut time, 'H')?;
        let minutes = duration_component(&mut time, 'M')?;
        let (seconds, nanos) = if let Some(seconds) = time.strip_suffix('S') {
            let (whole, fraction) = seconds.split_once(['.', ',']).unwrap_or((seconds, ""));
            if !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err("fractional seconds must contain only digits");
            }
            if fraction.bytes().skip(9).any(|byte| byte != b'0') {
                return Err("fractional seconds must be exactly representable in nanoseconds");
            }
            let nanos = fraction
                .bytes()
                .take(9)
                .chain(std::iter::repeat(b'0'))
                .take(9)
                .fold(0u32, |value, digit| value * 10 + u32::from(digit - b'0'));
            time = "";
            (duration_number(whole)?, nanos)
        } else {
            (0, 0)
        };
        if !date.is_empty() || !time.is_empty() {
            return Err("invalid duration component or component order");
        }
        let months = years
            .checked_mul(12)
            .and_then(|years| years.checked_add(months))
            .and_then(|months| i32::try_from(months).ok())
            .ok_or("calendar duration is out of range")?;
        let seconds = weeks
            .checked_mul(7)
            .and_then(|weeks| weeks.checked_add(days))
            .and_then(|days| days.checked_mul(24))
            .and_then(|days| days.checked_add(hours))
            .and_then(|hours| hours.checked_mul(60))
            .and_then(|hours| hours.checked_add(minutes))
            .and_then(|minutes| minutes.checked_mul(60))
            .and_then(|minutes| minutes.checked_add(seconds))
            .and_then(|seconds| i64::try_from(seconds).ok())
            .ok_or("fixed duration is out of range")?;
        let fixed = Duration::new(seconds, nanos).ok_or("fixed duration is out of range")?;
        let parts = Self { months, fixed };
        parts.end(SchedulePoint::DateTime(DateTime::<Utc>::MIN_UTC))?;
        Ok(parts)
    }

    fn relative(self) -> RelativeDuration {
        RelativeDuration::months(self.months).with_duration(self.fixed)
    }

    fn end(&self, start: SchedulePoint) -> Result<SchedulePoint, &'static str> {
        let months =
            Months::new(u32::try_from(self.months).map_err(|_| "duration cannot be negative")?);
        let end = match start {
            SchedulePoint::DateTime(start) => start
                .checked_add_months(months)
                .and_then(|start| start.checked_add_signed(self.fixed))
                .map(SchedulePoint::DateTime),
            SchedulePoint::Date(start) => start
                .checked_add_months(months)
                .and_then(|start| start.checked_add_signed(self.fixed))
                .map(SchedulePoint::Date),
        };
        end.ok_or("duration end is outside the supported calendar range")
    }
}

fn duration_component(input: &mut &str, suffix: char) -> Result<u64, &'static str> {
    let Some((number, rest)) = input.split_once(suffix) else {
        return Ok(0);
    };
    let value = duration_number(number)?;
    *input = rest;
    Ok(value)
}

fn duration_number(input: &str) -> Result<u64, &'static str> {
    let input = input.strip_prefix('+').unwrap_or(input);
    if input.is_empty() || !input.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("duration components must be nonnegative whole numbers");
    }
    input
        .parse()
        .map_err(|_| "duration component is out of range")
}

pub(crate) mod relative_duration_iso8601 {
    use chronoutil::RelativeDuration;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(d: &RelativeDuration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&d.format_to_iso8601())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<RelativeDuration, D::Error> {
        super::parse_iso8601_duration(&String::deserialize(d)?).map_err(serde::de::Error::custom)
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
            Some(s) => super::parse_iso8601_duration(&s)
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
