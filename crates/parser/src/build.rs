use serde::Serialize;
use subroutine_core::{
    Action, ActionTemplate, Event, EventTemplate, Marker, Recurrence, RecurrenceRule, RoutineStep,
    SchedulePoint, Signal,
};

use crate::ast::{ParseDraft, RecurrenceSpec, WhenSpec};

#[derive(Debug, Clone, thiserror::Error)]
pub enum ParserBuildError {
    #[error("missing time")]
    MissingTime,
    #[error("invalid time: {0}")]
    InvalidTime(String),
    #[error("missing duration")]
    MissingDuration,
    #[error("invalid recurrence: {0}")]
    InvalidRecurrence(String),
}

#[derive(Debug, Clone, Copy)]
pub enum BuildTarget {
    Action,
    ActionTemplate,
    Event,
    EventTemplate,
    Signal,
    Marker,
    RoutineStep,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "value")]
pub enum BuiltEntity {
    Action(Action),
    ActionTemplate(ActionTemplate),
    Event(Event),
    EventTemplate(EventTemplate),
    Signal(Signal),
    Marker(Marker),
    RoutineStep(RoutineStep),
}

pub fn build_entity(
    draft: &ParseDraft,
    target: BuildTarget,
) -> Result<BuiltEntity, ParserBuildError> {
    Ok(match target {
        BuildTarget::Action => BuiltEntity::Action(build_action(draft)),
        BuildTarget::ActionTemplate => BuiltEntity::ActionTemplate(build_action_template(draft)),
        BuildTarget::Event => BuiltEntity::Event(build_event(draft)?),
        BuildTarget::EventTemplate => BuiltEntity::EventTemplate(build_event_template(draft)?),
        BuildTarget::Signal => BuiltEntity::Signal(build_signal(draft)?),
        BuildTarget::Marker => BuiltEntity::Marker(build_marker(draft)?),
        BuildTarget::RoutineStep => BuiltEntity::RoutineStep(build_routine_step(draft)),
    })
}

fn build_action(draft: &ParseDraft) -> Action {
    let action = Action::new(&draft.title)
        .with_content(draft.content.clone())
        .with_duration(draft.duration.map(Into::into))
        .with_recurrence(recurrence(draft));

    match draft.when {
        Some(WhenSpec::DateTime(dt)) => action
            .with_queued(true)
            .with_start(Some(SchedulePoint::DateTime(dt))),
        Some(WhenSpec::NaiveDate(date)) => action.with_start(Some(SchedulePoint::Date(date))),
        None => action,
    }
}

fn build_event(draft: &ParseDraft) -> Result<Event, ParserBuildError> {
    let start = required_datetime(draft)?;
    let duration = draft.duration.ok_or(ParserBuildError::MissingDuration)?;

    Ok(Event::new(&draft.title, start, duration)
        .with_content(draft.content.clone())
        .with_recurrence(recurrence(draft)))
}

pub fn build_action_template(draft: &ParseDraft) -> ActionTemplate {
    ActionTemplate {
        content: draft.content.clone(),
        naive_time: draft.naive_time,
        duration: draft.duration.map(Into::into),
        recurrence: recurrence(draft),
        ..ActionTemplate::new(&draft.title)
    }
}

pub fn build_event_template(draft: &ParseDraft) -> Result<EventTemplate, ParserBuildError> {
    let duration = draft.duration.ok_or(ParserBuildError::MissingDuration)?;

    Ok(EventTemplate {
        content: draft.content.clone(),
        recurrence: recurrence(draft),
        ..EventTemplate::new(&draft.title, duration.into())
    })
}

fn build_signal(draft: &ParseDraft) -> Result<Signal, ParserBuildError> {
    let datetime = required_datetime(draft)?;

    let mut signal = Signal::new(&draft.title, datetime).with_content(draft.content.clone());
    if let Some(recurrence) = recurrence(draft) {
        signal = signal.with_recurrence(recurrence);
    }
    Ok(signal)
}

fn build_marker(draft: &ParseDraft) -> Result<Marker, ParserBuildError> {
    let when = draft.when.as_ref().ok_or(ParserBuildError::MissingTime)?;

    let mut marker = Marker::new(&draft.title, when.date());
    marker.content = draft.content.clone();
    marker.recurrence = recurrence(draft);
    Ok(marker)
}

fn build_routine_step(draft: &ParseDraft) -> RoutineStep {
    let mut step = RoutineStep::new(&draft.title);
    if let Some(duration) = draft.duration {
        step = step.with_duration(duration.into());
    }
    step
}

fn required_datetime(
    draft: &ParseDraft,
) -> Result<chrono::DateTime<chrono::Utc>, ParserBuildError> {
    match draft.when {
        Some(WhenSpec::DateTime(dt)) => Ok(dt),
        Some(WhenSpec::NaiveDate(_)) => Err(ParserBuildError::InvalidTime(
            "include a time of day".to_string(),
        )),
        None => Err(ParserBuildError::MissingTime),
    }
}

fn recurrence(draft: &ParseDraft) -> Option<Recurrence> {
    recurrence_to_rule(draft.recurrence.as_ref()).map(|recurrence| {
        recurrence
            .with_end_date(draft.recurrence_end_date)
            .with_remaining(draft.recurrence_remaining)
    })
}

pub fn recurrence_to_rule(spec: Option<&RecurrenceSpec>) -> Option<Recurrence> {
    let rule = match spec? {
        RecurrenceSpec::EveryDays(n) => RecurrenceRule::days(positive(*n)?),
        RecurrenceSpec::EveryWeeks(n) => RecurrenceRule::weeks(positive(*n)?),
        RecurrenceSpec::EveryMonths(n) => RecurrenceRule::months(positive(*n)?),
        RecurrenceSpec::EveryYears(n) => RecurrenceRule::years(positive(*n)?),
        RecurrenceSpec::OnMonthDay(day) => RecurrenceRule::MonthlyDay(*day),
        RecurrenceSpec::OnWeekdays(days) => {
            let days: chrono::WeekdaySet = days.iter().collect();
            if days.is_empty() {
                return None;
            }
            RecurrenceRule::WeeklyDays(days)
        }
    };
    Some(Recurrence::new(rule))
}

fn positive(n: i64) -> Option<u32> {
    u32::try_from(n).ok().filter(|n| *n > 0)
}
