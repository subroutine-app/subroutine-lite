use chrono::{DateTime, Utc};
use chronoutil::RelativeDuration;
use uuid::Uuid;

use crate::{Action, Routine, RoutineStep, SchedulePoint};

use super::{Changes, Outcome, Snapshot, put};

pub fn create(mut routine: Routine) -> Outcome<Routine> {
    routine.id = Uuid::now_v7();
    routine.recurrence_id = routine.id;
    put(routine)
}

pub fn instantiate(
    snapshot: &Snapshot,
    routine: &Routine,
    start: Option<DateTime<Utc>>,
) -> Outcome<Vec<Action>> {
    let context = snapshot.context();
    let step_duration = |step: &RoutineStep| {
        step.duration
            .unwrap_or(snapshot.settings.default_step_duration)
    };

    let mut cursor = match start {
        Some(start) => context.quantize_ceil(start),
        None => context.next_slot(
            routine
                .steps
                .first()
                .map(step_duration)
                .unwrap_or_else(RelativeDuration::zero),
        ),
    };

    let mut actions = Vec::with_capacity(routine.steps.len());
    for step in &routine.steps {
        let duration = step_duration(step);

        actions.push(
            Action::new(step.title.clone())
                .with_routine_id(routine.id)
                .with_duration(Some(duration))
                .with_queued(true)
                .with_start(Some(SchedulePoint::DateTime(cursor))),
        );

        cursor = cursor + duration;
    }

    let mut changes = Changes::default();
    changes.put_all(actions.clone());
    Outcome::new(actions, changes.rescheduled())
}
