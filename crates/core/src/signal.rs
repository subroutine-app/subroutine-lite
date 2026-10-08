use chrono::{DateTime, Utc};
use chronoutil::RelativeDuration;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{CoreItem, ItemType, Recurrence, SchedulePoint, recurrence::occurrence_id};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    pub id: Uuid,
    pub lineage_id: Uuid,
    pub template_id: Option<Uuid>,
    pub title: String,
    pub content: Option<String>,
    pub datetime: DateTime<Utc>,
    pub recurrence: Option<Recurrence>,
}

impl Signal {
    pub const MAX_PROJECTION_STEPS: usize = 4096;

    pub fn new(title: impl Into<String>, datetime: impl Into<DateTime<Utc>>) -> Self {
        let id = Uuid::now_v7();
        Self {
            id,
            lineage_id: id,
            template_id: None,
            title: title.into(),
            content: None,
            datetime: datetime.into(),
            recurrence: None,
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

    pub fn with_content(mut self, content: Option<impl Into<String>>) -> Self {
        self.content = content.map(|c| c.into());
        self
    }

    pub fn with_recurrence(mut self, recurrence: Recurrence) -> Self {
        self.recurrence = Some(recurrence);
        self
    }

    pub fn is_from_template(&self) -> bool {
        self.template_id.is_some()
    }

    pub fn next_recurrence(&self) -> Option<Self> {
        let recurrence = self.recurrence?;
        let (next_time, next_recurrence) = recurrence.advance(self.datetime)?;
        Some(Self {
            id: occurrence_id(self.lineage_id, "signal", next_time),
            lineage_id: self.lineage_id,
            template_id: self.template_id,
            title: self.title.clone(),
            content: self.content.clone(),
            datetime: next_time.into(),
            recurrence: Some(next_recurrence),
        })
    }

    pub fn as_template(&self) -> SignalTemplate {
        SignalTemplate {
            id: Uuid::now_v7(),
            lineage_id: self.lineage_id,
            title: self.title.clone(),
            content: self.content.clone(),
            recurrence: self.recurrence,
        }
    }

    pub fn occurrence_at(&self, datetime: DateTime<Utc>) -> Self {
        Self {
            id: occurrence_id(self.lineage_id, "signal", datetime.into()),
            lineage_id: self.lineage_id,
            template_id: self.template_id,
            title: self.title.clone(),
            content: self.content.clone(),
            datetime,
            recurrence: self.recurrence,
        }
    }

    pub fn projections_between(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Vec<Self> {
        let Some(mut recurrence) = self.recurrence else {
            return Vec::new();
        };
        let (cursor, fast_forwarded) =
            recurrence.fast_forward_before(self.datetime, start.date_naive());
        let mut cursor = DateTime::<Utc>::from(cursor);
        recurrence = fast_forwarded;

        let mut occurrences = Vec::new();
        for _ in 0..Self::MAX_PROJECTION_STEPS {
            let Some((next_point, next_recurrence)) = recurrence.advance(cursor) else {
                break;
            };
            let next = DateTime::<Utc>::from(next_point);
            if next <= cursor {
                break;
            }
            cursor = next;
            recurrence = next_recurrence;
            if cursor > end {
                break;
            }
            if cursor >= start {
                let mut occurrence = self.occurrence_at(cursor);
                occurrence.recurrence = Some(recurrence);
                occurrences.push(occurrence);
            }
        }
        occurrences
    }
}

impl CoreItem for Signal {
    fn id(&self) -> Uuid {
        self.id
    }

    fn lineage_id(&self) -> Uuid {
        self.lineage_id
    }

    fn item_type(&self) -> ItemType {
        ItemType::Signal
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn content(&self) -> Option<String> {
        self.content.clone()
    }

    fn start(&self) -> Option<SchedulePoint> {
        Some(self.datetime.into())
    }

    fn duration(&self) -> Option<RelativeDuration> {
        None
    }

    fn end(&self) -> Option<SchedulePoint> {
        None
    }

    fn recurrence(&self) -> Option<Recurrence> {
        self.recurrence
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalTemplate {
    pub id: Uuid,
    pub lineage_id: Uuid,
    pub title: String,
    pub content: Option<String>,
    pub recurrence: Option<Recurrence>,
}

impl SignalTemplate {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            id: Uuid::now_v7(),
            lineage_id: Uuid::now_v7(),
            title: title.into(),
            content: None,
            recurrence: None,
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

    pub fn build(self, datetime: DateTime<Utc>) -> Signal {
        Signal {
            id: Uuid::now_v7(),
            lineage_id: self.lineage_id,
            template_id: Some(self.id),
            title: self.title,
            content: self.content,
            datetime,
            recurrence: self.recurrence,
        }
    }
}

impl CoreItem for SignalTemplate {
    fn id(&self) -> Uuid {
        self.id
    }

    fn lineage_id(&self) -> Uuid {
        self.lineage_id
    }

    fn item_type(&self) -> ItemType {
        ItemType::Signal
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
