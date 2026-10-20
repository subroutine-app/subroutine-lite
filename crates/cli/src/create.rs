use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Local};
use parser::{BuildTarget, BuiltEntity, EntityKind};
use serde_json::Value;
use subroutine_core::{Action, Event, Marker, ResourceValue, Routine, SchedulePoint, Signal, ops};

use crate::{
    args::{AddCommand, ParseKind},
    commands::Plan,
    resources,
};

pub fn add(command: AddCommand, mut snapshot: ops::Snapshot) -> Result<Plan> {
    let resource = match command {
        AddCommand::Action {
            text,
            start,
            duration,
            queue,
            pin,
        } => {
            let mut action = Action::new(text.title)
                .with_content(text.content)
                .with_start(start)
                .with_duration(duration)
                .with_queued(matches!(start, Some(SchedulePoint::DateTime(_))));
            if queue {
                let id = action.id;
                resources::validate(&ResourceValue::Action(action.clone()), snapshot.settings)?;
                snapshot.actions.push(action);
                let mut outcome = ops::actions::queue(&snapshot, id).map_err(Plan::op_error)?;
                action = outcome
                    .value
                    .iter()
                    .find(|action| action.id == id)
                    .context("queue result is missing the new action")?
                    .clone();
                if pin {
                    action.pinned = true;
                    outcome.changes.put(action.clone());
                }
                return Plan::outcome(ops::Outcome::new(action, outcome.changes));
            }
            ensure!(
                !pin || action.queued,
                "pinning requires --queue or a timed --start"
            );
            action.pinned = pin;
            ResourceValue::Action(action)
        }
        AddCommand::Event {
            text,
            start,
            duration,
            free,
        } => {
            let mut event = Event::new(text.title, start, duration).with_content(text.content);
            if free {
                event.busy_override = Some(false);
            }
            ResourceValue::Event(event)
        }
        AddCommand::Marker {
            text,
            date,
            end_date,
        } => {
            let mut marker = Marker::new(text.title, date);
            marker.content = text.content;
            marker.end_date = end_date;
            ResourceValue::Marker(marker)
        }
        AddCommand::Signal { text, at } => {
            ResourceValue::Signal(Signal::new(text.title, at).with_content(text.content))
        }
        AddCommand::Routine {
            title,
            content,
            step,
            target,
        } => {
            let steps = step
                .iter()
                .map(
                    |text| match parse(ParseKind::RoutineStep, text, snapshot.now)? {
                        BuiltEntity::RoutineStep(step) => Ok(step),
                        _ => anyhow::bail!("parser did not return a routine step"),
                    },
                )
                .collect::<Result<Vec<_>>>()?;
            let mut routine = Routine::new(title).with_steps(steps);
            routine.content = content;
            routine.target = target;
            ResourceValue::Routine(routine)
        }
    };
    Plan::write(resource)
}

pub fn parsed_value(kind: ParseKind, text: &str, now: DateTime<Local>) -> Result<Value> {
    let entity = parse(kind, text, now)?;
    let value = match entity {
        BuiltEntity::Action(value) => ResourceValue::Action(value),
        BuiltEntity::Event(value) => ResourceValue::Event(value),
        BuiltEntity::Marker(value) => ResourceValue::Marker(value),
        BuiltEntity::Signal(value) => ResourceValue::Signal(value),
        BuiltEntity::ActionTemplate(value) => ResourceValue::ActionTemplate(value),
        BuiltEntity::EventTemplate(value) => ResourceValue::EventTemplate(value),
        BuiltEntity::RoutineStep(step) => return Ok(serde_json::to_value(step)?),
    };
    resources::validate(&value, ops::Settings::default())?;
    Plan::resource_value(&value)
}

fn parse(kind: ParseKind, text: &str, now: DateTime<Local>) -> Result<BuiltEntity> {
    let (kind, target) = match kind {
        ParseKind::Action => (EntityKind::Action, BuildTarget::Action),
        ParseKind::Event => (EntityKind::Event, BuildTarget::Event),
        ParseKind::Marker => (EntityKind::Marker, BuildTarget::Marker),
        ParseKind::Signal => (EntityKind::Signal, BuildTarget::Signal),
        ParseKind::ActionTemplate => (EntityKind::ActionTemplate, BuildTarget::ActionTemplate),
        ParseKind::EventTemplate => (EntityKind::EventTemplate, BuildTarget::EventTemplate),
        ParseKind::RoutineStep => (EntityKind::RoutineStep, BuildTarget::RoutineStep),
    };
    let draft = parser::parse_with_context(
        text,
        kind,
        now,
        ops::Settings::default().schedule.granularity,
    )?;
    for warning in &draft.warnings {
        eprintln!("warning: {warning}");
    }
    Ok(parser::build_entity(&draft, target)?)
}
