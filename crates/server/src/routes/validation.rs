use chrono::{DateTime, Utc};
use chronoutil::RelativeDuration;
use subroutine_core::{
    Action, Event, ResourceValue, Routine, RoutineStep, SchedulePoint, checked_duration_end,
    checked_duration_sum,
};

use crate::ops::Settings;

pub(crate) fn duration(
    start: SchedulePoint,
    duration: RelativeDuration,
) -> Result<SchedulePoint, String> {
    checked_duration_end(start, duration).map_err(str::to_owned)
}

pub(crate) fn action(action: &Action, settings: Settings) -> Result<(), String> {
    duration(
        action.start.unwrap_or_else(SchedulePoint::now),
        action
            .duration
            .unwrap_or(settings.schedule.default_action_duration),
    )
    .map(|_| ())
    .map_err(|error| format!("action {}: {error}", action.id))
}

pub(crate) fn actions<'a>(
    actions: impl IntoIterator<Item = &'a Action>,
    settings: Settings,
) -> Result<(), String> {
    actions
        .into_iter()
        .try_for_each(|value| action(value, settings))
}

pub(crate) fn event(event: &Event) -> Result<(), String> {
    duration(event.start.into(), event.duration)
        .map(|_| ())
        .map_err(|error| format!("event {}: {error}", event.id))
}

pub(crate) fn steps(
    steps: &[RoutineStep],
    start: SchedulePoint,
    default_duration: RelativeDuration,
) -> Result<(), String> {
    let durations = || {
        steps
            .iter()
            .map(|step| step.duration.unwrap_or(default_duration))
    };
    let total = checked_duration_sum(durations()).map_err(str::to_owned)?;
    duration(start, total)?;
    durations().try_fold(
        SchedulePoint::DateTime(DateTime::<Utc>::from(start)),
        duration,
    )?;
    Ok(())
}

pub(crate) fn routine(routine: &Routine, settings: Settings) -> Result<(), String> {
    steps(
        &routine.steps,
        routine.target.unwrap_or_else(SchedulePoint::now),
        settings.default_step_duration,
    )
    .map_err(|error| format!("routine {}: {error}", routine.id))
}

pub(crate) fn resource(resource: &ResourceValue, settings: Settings) -> Result<(), String> {
    match resource {
        ResourceValue::Action(value) => action(value, settings),
        ResourceValue::Event(value) => event(value),
        ResourceValue::Routine(value) => routine(value, settings),
        ResourceValue::ActionTemplate(value) => {
            if let Some(value) = value.duration {
                duration(SchedulePoint::now(), value)?;
            }
            Ok(())
        }
        ResourceValue::EventTemplate(value) => {
            duration(SchedulePoint::now(), value.duration).map(|_| ())
        }
        ResourceValue::Marker(_)
        | ResourceValue::Signal(_)
        | ResourceValue::MarkerTemplate(_)
        | ResourceValue::SignalTemplate(_) => Ok(()),
    }
}
