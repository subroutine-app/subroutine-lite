use std::num::NonZeroU32;

use serde::Serialize;
use subroutine_core::{
    Action, ActionTemplate, Event, EventTemplate, Marker, Recurrence, RecurrenceRule,
    RecurrenceUnit, RoutineStep, SchedulePoint, Signal,
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
    #[error("invalid duration: {0}")]
    InvalidDuration(String),
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
    validate_draft(draft)?;
    Ok(match target {
        BuildTarget::Action => BuiltEntity::Action(build_action(draft)?),
        BuildTarget::ActionTemplate => BuiltEntity::ActionTemplate(build_action_template(draft)?),
        BuildTarget::Event => BuiltEntity::Event(build_event(draft)?),
        BuildTarget::EventTemplate => BuiltEntity::EventTemplate(build_event_template(draft)?),
        BuildTarget::Signal => BuiltEntity::Signal(build_signal(draft)?),
        BuildTarget::Marker => BuiltEntity::Marker(build_marker(draft)?),
        BuildTarget::RoutineStep => BuiltEntity::RoutineStep(build_routine_step(draft)),
    })
}

fn build_action(draft: &ParseDraft) -> Result<Action, ParserBuildError> {
    let action = Action::new(&draft.title)
        .with_content(draft.content.clone())
        .with_duration(draft.duration.map(Into::into))
        .with_recurrence(recurrence(draft)?);

    Ok(match draft.when {
        Some(WhenSpec::DateTime(dt)) => action
            .with_queued(true)
            .with_start(Some(SchedulePoint::DateTime(dt))),
        Some(WhenSpec::NaiveDate(date)) => action.with_start(Some(SchedulePoint::Date(date))),
        None => action,
    })
}

fn build_event(draft: &ParseDraft) -> Result<Event, ParserBuildError> {
    let start = required_datetime(draft)?;
    let duration = draft.duration.ok_or(ParserBuildError::MissingDuration)?;

    Ok(Event::new(&draft.title, start, duration)
        .with_content(draft.content.clone())
        .with_recurrence(recurrence(draft)?))
}

pub fn build_action_template(draft: &ParseDraft) -> Result<ActionTemplate, ParserBuildError> {
    validate_draft(draft)?;
    Ok(ActionTemplate {
        content: draft.content.clone(),
        naive_time: draft.naive_time,
        duration: draft.duration.map(Into::into),
        recurrence: recurrence(draft)?,
        ..ActionTemplate::new(&draft.title)
    })
}

pub fn build_event_template(draft: &ParseDraft) -> Result<EventTemplate, ParserBuildError> {
    validate_draft(draft)?;
    let duration = draft.duration.ok_or(ParserBuildError::MissingDuration)?;

    Ok(EventTemplate {
        content: draft.content.clone(),
        recurrence: recurrence(draft)?,
        ..EventTemplate::new(&draft.title, duration.into())
    })
}

fn build_signal(draft: &ParseDraft) -> Result<Signal, ParserBuildError> {
    let datetime = required_datetime(draft)?;

    let mut signal = Signal::new(&draft.title, datetime).with_content(draft.content.clone());
    if let Some(recurrence) = recurrence(draft)? {
        signal = signal.with_recurrence(recurrence);
    }
    Ok(signal)
}

fn build_marker(draft: &ParseDraft) -> Result<Marker, ParserBuildError> {
    let when = draft.when.as_ref().ok_or(ParserBuildError::MissingTime)?;

    let mut marker = Marker::new(&draft.title, when.date());
    marker.content = draft.content.clone();
    marker.recurrence = recurrence(draft)?;
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

fn validate_draft(draft: &ParseDraft) -> Result<(), ParserBuildError> {
    recurrence(draft)?;
    if let Some(duration) = draft.duration {
        if duration < chrono::Duration::zero() {
            return Err(ParserBuildError::InvalidDuration(
                "duration cannot be negative".into(),
            ));
        }
        let fits = match draft.when {
            Some(WhenSpec::DateTime(start)) => start.checked_add_signed(duration).is_some(),
            Some(WhenSpec::NaiveDate(start)) => start.checked_add_signed(duration).is_some(),
            None => true,
        };
        if !fits {
            return Err(ParserBuildError::InvalidDuration(
                "end date is out of range".into(),
            ));
        }
    }
    Ok(())
}

fn recurrence(draft: &ParseDraft) -> Result<Option<Recurrence>, ParserBuildError> {
    if draft.recurrence_remaining == Some(0) {
        return Err(ParserBuildError::InvalidRecurrence(
            "occurrence count must be greater than zero".into(),
        ));
    }
    Ok(
        recurrence_to_rule(draft.recurrence.as_ref())?.map(|recurrence| {
            recurrence
                .with_end_date(draft.recurrence_end_date)
                .with_remaining(draft.recurrence_remaining)
        }),
    )
}

pub fn recurrence_to_rule(
    spec: Option<&RecurrenceSpec>,
) -> Result<Option<Recurrence>, ParserBuildError> {
    let Some(spec) = spec else {
        return Ok(None);
    };
    let rule = match spec {
        RecurrenceSpec::EveryDays(n) => relative_rule(RecurrenceUnit::Days, *n)?,
        RecurrenceSpec::EveryWeeks(n) => relative_rule(RecurrenceUnit::Weeks, *n)?,
        RecurrenceSpec::EveryMonths(n) => relative_rule(RecurrenceUnit::Months, *n)?,
        RecurrenceSpec::EveryYears(n) => relative_rule(RecurrenceUnit::Years, *n)?,
        RecurrenceSpec::OnMonthDay(day) => {
            if !(1..=31).contains(day) {
                return Err(ParserBuildError::InvalidRecurrence(
                    "month day must be between 1 and 31".into(),
                ));
            }
            RecurrenceRule::MonthlyDay(*day)
        }
        RecurrenceSpec::OnWeekdays(days) => {
            if days.is_empty() || days.0 & !0x7f != 0 {
                return Err(ParserBuildError::InvalidRecurrence(
                    "select at least one valid weekday".into(),
                ));
            }
            RecurrenceRule::WeeklyDays(days.iter().collect())
        }
    };
    Recurrence::in_local_timezone(rule)
        .map(Some)
        .map_err(|reason| ParserBuildError::InvalidRecurrence(reason.into()))
}

fn relative_rule(unit: RecurrenceUnit, n: i64) -> Result<RecurrenceRule, ParserBuildError> {
    let interval = u32::try_from(n)
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or_else(|| {
            ParserBuildError::InvalidRecurrence("interval must be between 1 and 4294967295".into())
        })?;
    Ok(RecurrenceRule::Relative { unit, interval })
}
