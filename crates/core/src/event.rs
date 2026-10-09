use chrono::{DateTime, Days, Duration, NaiveDate, Offset, TimeZone, Utc};
use chronoutil::RelativeDuration;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    CoreItem, ItemType, Marker, Recurrence, SchedulePoint, recurrence::occurrence_id,
    relative_duration_iso8601,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: Uuid,
    pub lineage_id: Uuid,
    pub template_id: Option<Uuid>,
    pub title: String,
    pub content: Option<String>,
    pub start: DateTime<Utc>,
    #[serde(with = "relative_duration_iso8601")]
    pub duration: RelativeDuration,
    pub recurrence: Option<Recurrence>,
    pub source_provider: Option<String>,
    pub source_external_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_busy: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub busy_override: Option<bool>,
}

impl Event {
    pub const MAX_PROJECTION_STEPS: usize = 4096;

    pub fn new(
        title: impl Into<String>,
        datetime: impl Into<DateTime<Utc>>,
        duration: impl Into<RelativeDuration>,
    ) -> Self {
        let id = Uuid::now_v7();
        Self {
            id,
            lineage_id: id,
            template_id: None,
            title: title.into(),
            content: None,
            start: datetime.into(),
            duration: duration.into(),
            recurrence: None,
            source_provider: None,
            source_external_id: None,
            source_busy: None,
            busy_override: None,
        }
    }

    pub fn blocks_time(&self) -> bool {
        self.busy_override.or(self.source_busy).unwrap_or(true)
    }

    pub fn preserve_missing_availability(&mut self, existing: Option<&Self>) {
        if let Some(existing) = existing {
            self.source_busy = self.source_busy.or(existing.source_busy);
            self.busy_override = self.busy_override.or(existing.busy_override);
        }
    }

    pub fn preserve_busy_override(&mut self, existing: Option<&Self>) {
        self.busy_override = existing.and_then(|event| event.busy_override);
    }

    pub fn with_lineage_id(mut self, lineage_id: Uuid) -> Self {
        self.lineage_id = lineage_id;
        self
    }

    pub fn with_template_id(mut self, template_id: Uuid) -> Self {
        self.template_id = Some(template_id);
        self
    }

    pub fn with_content(mut self, content: Option<impl Into<String>>) -> Self {
        self.content = content.map(|c| c.into());
        self
    }

    pub fn with_recurrence(mut self, recurrence: Option<Recurrence>) -> Self {
        self.recurrence = recurrence;
        self
    }

    pub fn with_duration(mut self, duration: RelativeDuration) -> Self {
        self.duration = duration;
        self
    }

    pub fn with_source_provider(mut self, source_provider: impl Into<String>) -> Self {
        self.source_provider = Some(source_provider.into());
        self
    }

    pub fn with_source_external_id(mut self, source_external_id: impl Into<String>) -> Self {
        self.source_external_id = Some(source_external_id.into());
        self
    }

    pub fn end_time(&self) -> Result<DateTime<Utc>, &'static str> {
        crate::checked_duration_end(self.start.into(), self.duration).map(DateTime::<Utc>::from)
    }

    pub fn is_expired(&self, now: DateTime<Utc>) -> Result<bool, &'static str> {
        Ok(self.end_time()? < now)
    }

    fn last_occupied_instant(&self) -> Result<DateTime<Utc>, &'static str> {
        let end = self.end_time()?;
        if end == self.start {
            return Ok(self.start);
        }
        end.checked_sub_signed(Duration::nanoseconds(1))
            .ok_or("event end is outside the supported calendar range")
    }

    pub fn next_recurrence(&self) -> Option<Self> {
        let recurrence = self.recurrence?;
        let (next_time, next_recurrence) = recurrence.advance(self.start)?;
        crate::checked_duration_end(next_time, self.duration).ok()?;
        Some(Self {
            id: occurrence_id(self.lineage_id, "event", next_time),
            lineage_id: self.lineage_id,
            template_id: self.template_id,
            title: self.title.clone(),
            content: self.content.clone(),
            start: next_time.into(),
            duration: self.duration,
            recurrence: Some(next_recurrence),
            source_provider: self.source_provider.clone(),
            source_external_id: self.source_external_id.clone(),
            source_busy: self.source_busy,
            busy_override: self.busy_override,
        })
    }

    pub fn next_recurrence_after(&self, after: DateTime<Utc>) -> Option<Self> {
        let recurrence = self.recurrence?;
        let (mut cursor, mut recurrence) =
            recurrence.fast_forward_before(self.start, after.date_naive());

        for _ in 0..Self::MAX_PROJECTION_STEPS {
            let (next_time, next_recurrence) = recurrence.advance(cursor)?;
            if next_time.timestamp() <= cursor.timestamp() {
                return None;
            }
            cursor = next_time;
            recurrence = next_recurrence;
            let start = DateTime::<Utc>::from(next_time);
            if start > after {
                crate::checked_duration_end(next_time, self.duration).ok()?;
                return Some(Self {
                    id: occurrence_id(self.lineage_id, "event", next_time),
                    lineage_id: self.lineage_id,
                    template_id: self.template_id,
                    title: self.title.clone(),
                    content: self.content.clone(),
                    start,
                    duration: self.duration,
                    recurrence: Some(next_recurrence),
                    source_provider: self.source_provider.clone(),
                    source_external_id: self.source_external_id.clone(),
                    source_busy: self.source_busy,
                    busy_override: self.busy_override,
                });
            }
        }

        None
    }

    pub fn marker_date_range_in<Tz: TimeZone>(
        &self,
        timezone: &Tz,
    ) -> Result<(NaiveDate, NaiveDate), &'static str> {
        let local_date = |instant: DateTime<Utc>| {
            let offset = instant
                .with_timezone(timezone)
                .offset()
                .fix()
                .local_minus_utc();
            instant
                .naive_utc()
                .checked_add_signed(Duration::seconds(i64::from(offset)))
                .map(|local| local.date())
                .ok_or("event date is outside the supported calendar range in this timezone")
        };
        let date = local_date(self.start)?;
        let end_date = local_date(self.last_occupied_instant()?)?;
        if end_date < date {
            return Err("marker end date cannot precede its start date");
        }
        Ok((date, end_date))
    }

    pub fn to_marker_between(
        &self,
        date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<Marker, &'static str> {
        self.to_marker_between_with_id(date, end_date, Uuid::now_v7())
    }

    pub fn to_marker_between_with_id(
        &self,
        date: NaiveDate,
        end_date: NaiveDate,
        marker_id: Uuid,
    ) -> Result<Marker, &'static str> {
        self.end_time()?;
        if end_date < date {
            return Err("marker end date cannot precede its start date");
        }
        Ok(Marker {
            id: marker_id,
            lineage_id: marker_id,
            template_id: None,
            title: self.title.clone(),
            content: self.content.clone(),
            date,
            end_date: (end_date > date).then_some(end_date),
            recurrence: None,
            source_provider: None,
            source_external_id: None,
        })
    }

    pub fn to_marker_on(&self, date: NaiveDate) -> Result<Marker, &'static str> {
        self.to_marker_on_with_id(date, Uuid::now_v7())
    }

    pub fn to_marker_on_with_id(
        &self,
        date: NaiveDate,
        marker_id: Uuid,
    ) -> Result<Marker, &'static str> {
        let days = self
            .last_occupied_instant()?
            .signed_duration_since(self.start)
            .num_days();
        let days = u64::try_from(days).map_err(|_| "event duration cannot be negative")?;
        let end_date = date
            .checked_add_days(Days::new(days))
            .ok_or("marker end date is outside the supported calendar range")?;
        self.to_marker_between_with_id(date, end_date, marker_id)
    }

    pub fn as_template(&self) -> EventTemplate {
        EventTemplate {
            id: Uuid::now_v7(),
            lineage_id: self.lineage_id,
            sort_order: i64::MAX,
            title: self.title.clone(),
            content: self.content.clone(),
            duration: self.duration,
            recurrence: self.recurrence,
            source_provider: self.source_provider.clone(),
            source_external_id: self.source_external_id.clone(),
            source_busy: self.source_busy,
            busy_override: self.busy_override,
        }
    }
}

impl CoreItem for Event {
    fn id(&self) -> Uuid {
        self.id
    }

    fn lineage_id(&self) -> Uuid {
        self.lineage_id
    }

    fn item_type(&self) -> ItemType {
        ItemType::Event
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn content(&self) -> Option<String> {
        self.content.clone()
    }

    fn start(&self) -> Option<SchedulePoint> {
        Some(self.start.into())
    }

    fn duration(&self) -> Option<RelativeDuration> {
        Some(self.duration)
    }

    fn recurrence(&self) -> Option<Recurrence> {
        self.recurrence
    }
}

fn default_template_sort_order() -> i64 {
    i64::MAX
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventTemplate {
    pub id: Uuid,
    pub lineage_id: Uuid,
    #[serde(default = "default_template_sort_order")]
    pub sort_order: i64,
    pub title: String,
    pub content: Option<String>,
    #[serde(with = "relative_duration_iso8601")]
    pub duration: RelativeDuration,
    pub recurrence: Option<Recurrence>,
    pub source_provider: Option<String>,
    pub source_external_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_busy: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub busy_override: Option<bool>,
}

impl EventTemplate {
    pub fn preserve_missing_availability(&mut self, existing: Option<&Self>) {
        if let Some(existing) = existing {
            self.source_busy = self.source_busy.or(existing.source_busy);
            self.busy_override = self.busy_override.or(existing.busy_override);
        }
    }

    pub fn new(title: impl Into<String>, duration: RelativeDuration) -> Self {
        Self {
            id: Uuid::now_v7(),
            lineage_id: Uuid::now_v7(),
            sort_order: i64::MAX,
            title: title.into(),
            content: None,
            duration,
            recurrence: None,
            source_provider: None,
            source_external_id: None,
            source_busy: None,
            busy_override: None,
        }
    }

    pub fn with_lineage_id(mut self, lineage_id: Uuid) -> Self {
        self.lineage_id = lineage_id;
        self
    }

    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = Some(content.into());
        self
    }

    pub fn with_recurrence(mut self, recurrence: Recurrence) -> Self {
        self.recurrence = Some(recurrence);
        self
    }

    pub fn with_source_provider(mut self, source_provider: impl Into<String>) -> Self {
        self.source_provider = Some(source_provider.into());
        self
    }

    pub fn with_source_external_id(mut self, source_external_id: impl Into<String>) -> Self {
        self.source_external_id = Some(source_external_id.into());
        self
    }

    pub fn build(self, time: DateTime<Utc>) -> Event {
        Event {
            id: Uuid::now_v7(),
            lineage_id: self.lineage_id,
            template_id: Some(self.id),
            title: self.title,
            content: self.content,
            start: time,
            duration: self.duration,
            recurrence: self.recurrence,
            source_provider: self.source_provider,
            source_external_id: self.source_external_id,
            source_busy: self.source_busy,
            busy_override: self.busy_override,
        }
    }
}

impl CoreItem for EventTemplate {
    fn id(&self) -> Uuid {
        self.id
    }

    fn lineage_id(&self) -> Uuid {
        self.lineage_id
    }

    fn item_type(&self) -> ItemType {
        ItemType::EventTemplate
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn content(&self) -> Option<String> {
        self.content.clone()
    }

    fn start(&self) -> Option<SchedulePoint> {
        None
    }

    fn duration(&self) -> Option<RelativeDuration> {
        Some(self.duration)
    }

    fn recurrence(&self) -> Option<Recurrence> {
        self.recurrence
    }
}
