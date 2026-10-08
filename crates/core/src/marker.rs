use chrono::{Duration, NaiveDate};
use chronoutil::RelativeDuration;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{CoreItem, ItemType, Recurrence, SchedulePoint, recurrence::occurrence_id};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marker {
    pub id: Uuid,
    pub lineage_id: Uuid,
    pub template_id: Option<Uuid>,
    pub title: String,
    pub content: Option<String>,
    pub date: NaiveDate,
    pub end_date: Option<NaiveDate>,
    pub recurrence: Option<Recurrence>,
    pub source_provider: Option<String>,
    pub source_external_id: Option<String>,
}

impl Marker {
    pub const MAX_PROJECTION_STEPS: usize = 4096;

    pub fn new(title: impl Into<String>, date: NaiveDate) -> Self {
        let id = Uuid::now_v7();
        Self {
            id,
            lineage_id: id,
            template_id: None,
            title: title.into(),
            content: None,
            date,
            end_date: None,
            recurrence: None,
            source_provider: None,
            source_external_id: None,
        }
    }

    pub fn with_lineage_id(mut self, lineage_id: Uuid) -> Self {
        self.lineage_id = lineage_id;
        self
    }

    pub fn with_template_id(mut self, template_id: Uuid) -> Self {
        self.template_id = Some(template_id);
        self
    }

    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = Some(content.into());
        self
    }

    pub fn with_date(mut self, date: NaiveDate) -> Self {
        self.date = date;
        self
    }

    pub fn with_end_date(mut self, end_date: NaiveDate) -> Self {
        self.end_date = Some(end_date);
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

    pub fn set_date(&mut self, date: NaiveDate) {
        self.date = date;
    }

    pub fn set_end_date(&mut self, end_date: Option<NaiveDate>) {
        self.end_date = end_date;
    }

    pub fn is_multi_day(&self) -> bool {
        self.end_date.is_some()
    }

    pub fn is_from_template(&self) -> bool {
        self.template_id.is_some()
    }

    pub fn days_until(&self, today: NaiveDate) -> i64 {
        (self.date - today).num_days()
    }

    pub fn duration(&self) -> RelativeDuration {
        match self.end_date {
            Some(end) => end - self.date + RelativeDuration::days(1),
            None => RelativeDuration::days(1),
        }
    }

    pub fn next_recurrence(&self) -> Option<Self> {
        let recurrence = self.recurrence?;
        let (next_point, next_recurrence) = recurrence.advance(self.date)?;
        let next = next_point.date_naive();
        let end = self.end_date.map(|end| next + (end - self.date));
        Some(Self {
            id: occurrence_id(self.lineage_id, "marker", next_point),
            lineage_id: self.lineage_id,
            template_id: self.template_id,
            title: self.title.clone(),
            content: self.content.clone(),
            date: next,
            end_date: end,
            recurrence: Some(next_recurrence),
            source_provider: self.source_provider.clone(),
            source_external_id: self.source_external_id.clone(),
        })
    }

    pub fn occurrence_on(&self, date: NaiveDate) -> Self {
        Self {
            id: occurrence_id(self.lineage_id, "marker", date.into()),
            lineage_id: self.lineage_id,
            template_id: self.template_id,
            title: self.title.clone(),
            content: self.content.clone(),
            date,
            end_date: self.end_date.map(|end| date + (end - self.date)),
            recurrence: self.recurrence,
            source_provider: self.source_provider.clone(),
            source_external_id: self.source_external_id.clone(),
        }
    }

    pub fn projections_between(&self, start: NaiveDate, end: NaiveDate) -> Vec<Self> {
        let Some(mut recurrence) = self.recurrence else {
            return Vec::new();
        };
        let lookback_days = self
            .end_date
            .map(|end| (end - self.date).num_days().max(0))
            .unwrap_or(0);
        let target = start
            .checked_sub_signed(chrono::Duration::days(lookback_days))
            .unwrap_or(start);
        let (cursor, fast_forwarded) = recurrence.fast_forward_before(self.date, target);
        let mut cursor = cursor.date_naive();
        recurrence = fast_forwarded;

        let mut occurrences = Vec::new();
        for _ in 0..Self::MAX_PROJECTION_STEPS {
            let Some((next_point, next_recurrence)) = recurrence.advance(cursor) else {
                break;
            };
            let next = next_point.date_naive();
            if next <= cursor {
                break;
            }
            cursor = next;
            recurrence = next_recurrence;
            if cursor > end {
                break;
            }
            let mut occurrence = self.occurrence_on(cursor);
            occurrence.recurrence = Some(recurrence);
            if occurrence.end_date.unwrap_or(cursor) >= start {
                occurrences.push(occurrence);
            }
        }
        occurrences
    }

    pub fn covers(&self, date: NaiveDate) -> bool {
        self.date <= date && date <= self.end_date.unwrap_or(self.date)
    }

    pub fn as_template(&self) -> MarkerTemplate {
        let span_days = match self.end_date {
            Some(end) => (end - self.date).num_days() as u32 + 1,
            None => 1,
        };
        MarkerTemplate {
            id: Uuid::now_v7(),
            lineage_id: self.lineage_id,
            title: self.title.clone(),
            content: self.content.clone(),
            span_days,
            recurrence: self.recurrence,
            source_provider: self.source_provider.clone(),
            source_external_id: self.source_external_id.clone(),
        }
    }
}

impl CoreItem for Marker {
    fn id(&self) -> Uuid {
        self.id
    }

    fn lineage_id(&self) -> Uuid {
        self.lineage_id
    }

    fn item_type(&self) -> ItemType {
        ItemType::Marker
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn content(&self) -> Option<String> {
        self.content.clone()
    }

    fn start(&self) -> Option<SchedulePoint> {
        Some(self.date.into())
    }

    fn duration(&self) -> Option<RelativeDuration> {
        Some(self.duration())
    }

    fn recurrence(&self) -> Option<Recurrence> {
        self.recurrence
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkerTemplate {
    pub id: Uuid,
    pub lineage_id: Uuid,
    pub title: String,
    pub content: Option<String>,
    pub span_days: u32,
    pub recurrence: Option<Recurrence>,
    pub source_provider: Option<String>,
    pub source_external_id: Option<String>,
}

impl MarkerTemplate {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            id: Uuid::now_v7(),
            lineage_id: Uuid::now_v7(),
            title: title.into(),
            content: None,
            span_days: 1,
            recurrence: None,
            source_provider: None,
            source_external_id: None,
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

    pub fn with_span_days(mut self, span_days: u32) -> Self {
        self.span_days = span_days.max(1);
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

    pub fn build(self, date: NaiveDate) -> Marker {
        let end_date =
            (self.span_days > 1).then(|| date + Duration::days(i64::from(self.span_days) - 1));
        Marker {
            id: Uuid::now_v7(),
            lineage_id: self.lineage_id,
            template_id: Some(self.id),
            title: self.title,
            content: self.content,
            date,
            end_date,
            recurrence: self.recurrence,
            source_provider: self.source_provider,
            source_external_id: self.source_external_id,
        }
    }
}

impl CoreItem for MarkerTemplate {
    fn id(&self) -> Uuid {
        self.id
    }

    fn lineage_id(&self) -> Uuid {
        self.lineage_id
    }

    fn item_type(&self) -> ItemType {
        ItemType::Marker
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
}
