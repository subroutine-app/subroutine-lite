use chronoutil::RelativeDuration;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{CoreItem, ItemType, Recurrence, SchedulePoint, relative_duration_iso8601_opt};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineStep {
    pub title: String,
    #[serde(with = "relative_duration_iso8601_opt")]
    pub duration: Option<RelativeDuration>,
}

impl RoutineStep {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            duration: None,
        }
    }

    pub fn with_duration(mut self, duration: RelativeDuration) -> Self {
        self.duration = Some(duration);
        self
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn duration(&self) -> Option<RelativeDuration> {
        self.duration
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Routine {
    pub id: Uuid,
    #[serde(default)]
    pub recurrence_id: Uuid,
    pub title: String,
    pub content: Option<String>,
    pub target: Option<SchedulePoint>,
    pub steps: Vec<RoutineStep>,
    pub recurrence: Option<Recurrence>,
}

impl Routine {
    pub fn new(title: impl Into<String>) -> Self {
        let id = Uuid::now_v7();
        Self {
            id,
            recurrence_id: id,
            title: title.into(),
            content: None,
            target: None,
            steps: Vec::new(),
            recurrence: None,
        }
    }

    pub fn with_recurrence_id(mut self, recurrence_id: Uuid) -> Self {
        self.recurrence_id = recurrence_id;
        self
    }

    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = Some(content.into());
        self
    }

    pub fn with_target(mut self, target: impl Into<SchedulePoint>) -> Self {
        self.target = Some(target.into());
        self
    }

    pub fn with_steps(mut self, steps: Vec<RoutineStep>) -> Self {
        self.steps = steps;
        self
    }

    pub fn with_recurrence(mut self, recurrence: Recurrence) -> Self {
        self.recurrence = Some(recurrence);
        self
    }

    pub fn add_step(&mut self, step: RoutineStep) {
        self.steps.push(step);
    }

    pub fn insert_step(&mut self, index: usize, step: RoutineStep) {
        self.steps.insert(index, step);
    }

    pub fn steps(&self) -> &[RoutineStep] {
        &self.steps
    }
}

impl CoreItem for Routine {
    fn id(&self) -> Uuid {
        self.id
    }

    fn lineage_id(&self) -> Uuid {
        if self.recurrence_id.is_nil() {
            self.id
        } else {
            self.recurrence_id
        }
    }

    fn item_type(&self) -> ItemType {
        ItemType::Routine
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn content(&self) -> Option<String> {
        self.content.clone()
    }

    fn start(&self) -> Option<SchedulePoint> {
        self.target
    }

    fn duration(&self) -> Option<RelativeDuration> {
        let mut durations = self
            .steps()
            .iter()
            .filter_map(RoutineStep::duration)
            .peekable();
        durations.peek()?;
        crate::checked_duration_sum(durations).ok()
    }

    fn recurrence(&self) -> Option<Recurrence> {
        self.recurrence
    }
}
