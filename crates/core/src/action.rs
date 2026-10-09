use chrono::{DateTime, Local, NaiveTime, Utc};
use chronoutil::RelativeDuration;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    CoreItem, ItemType, Recurrence, SchedulePoint, recurrence::occurrence_id,
    relative_duration_iso8601_opt,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub id: Uuid,
    pub recurrence_id: Uuid,
    pub routine_id: Option<Uuid>,
    pub template_id: Option<Uuid>,
    pub title: String,
    pub content: Option<String>,
    pub queued: bool,
    pub pinned: bool,
    pub start: Option<SchedulePoint>,
    #[serde(with = "relative_duration_iso8601_opt")]
    pub duration: Option<RelativeDuration>,
    pub completion: Option<DateTime<Utc>>,
    pub recurrence: Option<Recurrence>,
    pub source_provider: Option<String>,
    pub source_external_id: Option<String>,
}

impl Action {
    pub fn new(title: impl Into<String>) -> Self {
        let id = Uuid::now_v7();
        Self {
            id,
            recurrence_id: id,
            routine_id: None,
            template_id: None,
            title: title.into(),
            content: None,
            queued: false,
            pinned: false,
            start: None,
            duration: None,
            completion: None,
            recurrence: None,
            source_provider: None,
            source_external_id: None,
        }
    }

    pub fn with_recurrence_id(mut self, lineage_id: Uuid) -> Self {
        self.recurrence_id = lineage_id;
        self
    }

    pub fn with_routine_id(mut self, routine_id: Uuid) -> Self {
        self.routine_id = Some(routine_id);
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

    pub fn with_queued(mut self, queued: bool) -> Self {
        self.queued = queued;
        self
    }

    pub fn with_pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }

    pub fn with_start(mut self, start: Option<SchedulePoint>) -> Self {
        self.start = start;
        self
    }

    pub fn with_duration(mut self, duration: Option<RelativeDuration>) -> Self {
        self.duration = duration;
        self
    }

    pub fn with_completion(mut self, completion: Option<DateTime<Utc>>) -> Self {
        self.completion = completion;
        self
    }

    pub fn with_recurrence(mut self, rule: Option<Recurrence>) -> Self {
        self.recurrence = rule;
        self
    }

    pub fn with_origin_routine(mut self, routine_id: Uuid) -> Self {
        self.routine_id = Some(routine_id);
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

    pub fn is_from_recurrence(&self) -> bool {
        self.recurrence_id != self.id
    }

    pub fn is_from_routine(&self) -> bool {
        self.routine_id.is_some()
    }

    pub fn is_from_template(&self) -> bool {
        self.template_id.is_some()
    }

    pub fn is_queued(&self) -> bool {
        self.queued
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned
    }

    pub fn is_scheduled(&self) -> bool {
        self.start.is_some()
    }

    pub fn is_overdue(&self, now: DateTime<Utc>) -> bool {
        match self.start.as_ref() {
            Some(SchedulePoint::DateTime(time)) => *time < now,
            Some(SchedulePoint::Date(date)) => *date < now.date_naive(),
            None => false,
        }
    }

    pub fn is_locked(&self, now: DateTime<Utc>) -> bool {
        self.is_scheduled() && self.pinned && !self.is_overdue(now)
    }

    pub fn is_completed(&self) -> bool {
        self.completion.is_some()
    }

    pub fn set_content(&mut self, content: impl Into<String>) {
        self.content = Some(content.into());
    }

    pub fn set_start(&mut self, start: Option<SchedulePoint>) {
        self.start = start;
    }

    pub fn set_duration(&mut self, duration: RelativeDuration) {
        self.duration = Some(duration);
    }

    pub fn set_completion(&mut self, completion: Option<DateTime<Utc>>) {
        self.completion = completion;
    }

    pub fn set_recurrence(&mut self, recurrence: Option<Recurrence>) {
        self.recurrence = recurrence;
    }

    pub fn set_lineage_id(&mut self, lineage_id: Uuid) {
        self.recurrence_id = lineage_id;
    }

    pub fn set_routine_id(&mut self, routine_id: Uuid) {
        self.routine_id = Some(routine_id);
    }

    pub fn set_template_id(&mut self, template_id: Uuid) {
        self.template_id = Some(template_id);
    }

    pub fn set_queued(&mut self, queued: bool) {
        self.queued = queued;
    }

    pub fn set_pinned(&mut self, pinned: bool) {
        self.pinned = pinned;
    }

    pub fn next_occurrence(&self) -> Option<Self> {
        let rule = self.recurrence?;
        let (next_start, next_recurrence) = rule.advance(self.start?)?;
        if let Some(duration) = self.duration {
            crate::checked_duration_end(next_start, duration).ok()?;
        }

        tracing::debug!(
            action_id = %self.id,
            title = %self.title,
            next_start = ?next_start,
            "generated next occurrence"
        );

        Some(Self {
            id: occurrence_id(self.recurrence_id, "action", next_start),
            recurrence_id: self.recurrence_id,
            routine_id: self.routine_id,
            template_id: self.template_id,
            title: self.title.clone(),
            content: self.content.clone(),
            start: Some(next_start),
            duration: self.duration,
            completion: None,
            recurrence: Some(next_recurrence),
            queued: self.queued,
            pinned: false,
            source_provider: self.source_provider.clone(),
            source_external_id: self.source_external_id.clone(),
        })
    }

    pub fn next_occurence(&self) -> Option<Self> {
        self.next_occurrence()
    }

    pub fn as_template(&self) -> ActionTemplate {
        let naive_time = self.start.and_then(|start| match start {
            SchedulePoint::DateTime(datetime) => Some(match self.recurrence {
                Some(recurrence) => recurrence.timezone.map_or_else(
                    || datetime.time(),
                    |timezone| datetime.with_timezone(&timezone).time(),
                ),
                None => datetime.with_timezone(&Local).time(),
            }),
            SchedulePoint::Date(_) => None,
        });
        ActionTemplate {
            id: Uuid::now_v7(),
            sort_order: i64::MAX,
            title: self.title.clone(),
            content: self.content.clone(),
            naive_time,
            duration: self.duration,
            recurrence: self.recurrence,
        }
    }
}

impl CoreItem for Action {
    fn id(&self) -> Uuid {
        self.id
    }

    fn lineage_id(&self) -> Uuid {
        self.recurrence_id
    }

    fn item_type(&self) -> ItemType {
        ItemType::Action
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn content(&self) -> Option<String> {
        self.content.clone()
    }

    fn start(&self) -> Option<SchedulePoint> {
        self.start
    }

    fn duration(&self) -> Option<RelativeDuration> {
        self.duration
    }

    fn recurrence(&self) -> Option<Recurrence> {
        self.recurrence
    }
}

fn default_template_sort_order() -> i64 {
    i64::MAX
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionTemplate {
    pub id: Uuid,
    #[serde(default = "default_template_sort_order")]
    pub sort_order: i64,
    pub title: String,
    pub content: Option<String>,
    pub naive_time: Option<NaiveTime>,
    #[serde(with = "relative_duration_iso8601_opt")]
    pub duration: Option<RelativeDuration>,
    pub recurrence: Option<Recurrence>,
}

impl ActionTemplate {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            id: Uuid::now_v7(),
            sort_order: i64::MAX,
            title: title.into(),
            content: None,
            naive_time: None,
            duration: None,
            recurrence: None,
        }
    }

    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = Some(content.into());
        self
    }

    pub fn with_duration(mut self, duration: RelativeDuration) -> Self {
        self.duration = Some(duration);
        self
    }

    pub fn with_recurrence(mut self, recurrence: Recurrence) -> Self {
        self.recurrence = Some(recurrence);
        self
    }

    pub fn with_time(mut self, time: Option<NaiveTime>) -> Self {
        self.naive_time = time;
        self
    }

    pub fn build(self) -> Action {
        let template_id = self.id;
        Action::new(self.title)
            .with_template_id(template_id)
            .with_content(self.content)
            .with_duration(self.duration)
            .with_recurrence(self.recurrence)
    }

    pub fn build_scheduled(self, start: SchedulePoint) -> Result<Action, &'static str> {
        let start = match (start, self.naive_time) {
            (SchedulePoint::Date(date), Some(time)) => {
                let local = date.and_time(time);
                let datetime = match self.recurrence {
                    Some(recurrence) => match recurrence.timezone {
                        Some(timezone) => local
                            .and_local_timezone(timezone)
                            .earliest()
                            .map(|datetime| datetime.to_utc()),
                        None => Some(local.and_utc()),
                    },
                    None => local
                        .and_local_timezone(Local)
                        .earliest()
                        .map(|datetime| datetime.to_utc()),
                }
                .ok_or("this time does not exist in the schedule timezone")?;
                SchedulePoint::DateTime(datetime)
            }
            _ => start,
        };
        if let Some(duration) = self.duration {
            crate::checked_duration_end(start, duration)?;
        }
        Ok(self.build().with_start(Some(start)).with_queued(true))
    }
}

impl CoreItem for ActionTemplate {
    fn id(&self) -> Uuid {
        self.id
    }

    fn lineage_id(&self) -> Uuid {
        self.id
    }

    fn item_type(&self) -> ItemType {
        ItemType::ActionTemplate
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
        self.duration
    }

    fn recurrence(&self) -> Option<Recurrence> {
        self.recurrence
    }
}
