use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
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

    pub fn end_time(&self) -> DateTime<Utc> {
        self.start + self.duration
    }

    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.end_time() < now
    }

    pub fn next_recurrence(&self) -> Option<Self> {
        let recurrence = self.recurrence?;
        let (next_time, next_recurrence) = recurrence.advance(self.start)?;
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

    pub fn marker_date_range_in<Tz: TimeZone>(&self, timezone: &Tz) -> (NaiveDate, NaiveDate) {
        let date = self.start.with_timezone(timezone).date_naive();
        let end = self.end_time();
        let last_occupied_instant = if end > self.start {
            end - Duration::nanoseconds(1)
        } else {
            self.start
        };
        let end_date = last_occupied_instant.with_timezone(timezone).date_naive();
        (date, end_date.max(date))
    }

    pub fn to_marker_between(&self, date: NaiveDate, end_date: NaiveDate) -> Marker {
        self.to_marker_between_with_id(date, end_date, Uuid::now_v7())
    }

    pub fn to_marker_between_with_id(
        &self,
        date: NaiveDate,
        end_date: NaiveDate,
        marker_id: Uuid,
    ) -> Marker {
        Marker {
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
        }
    }

    pub fn to_marker_on(&self, date: NaiveDate) -> Marker {
        self.to_marker_on_with_id(date, Uuid::now_v7())
    }

    pub fn to_marker_on_with_id(&self, date: NaiveDate, marker_id: Uuid) -> Marker {
        let elapsed = self.end_time() - self.start;
        let span_days = if elapsed <= Duration::zero() {
            1
        } else {
            let whole_days = elapsed.num_days();
            if elapsed > Duration::days(whole_days) {
                whole_days + 1
            } else {
                whole_days.max(1)
            }
        };
        self.to_marker_between_with_id(date, date + Duration::days(span_days - 1), marker_id)
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
