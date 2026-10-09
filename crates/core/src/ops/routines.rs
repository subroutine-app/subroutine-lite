use chrono::{DateTime, Utc};
use chronoutil::RelativeDuration;
use uuid::Uuid;

use crate::{
    Action, Routine, RoutineStep, SchedulePoint,
    schedule::{duration_end, find_free_slot, scheduled_time},
};

use super::{Changes, OpError, OpResult, Outcome, Snapshot, put};

pub fn create(mut routine: Routine) -> Outcome<Routine> {
    routine.id = Uuid::now_v7();
    routine.recurrence_id = routine.id;
    put(routine)
}

pub fn instantiate(
    snapshot: &Snapshot,
    routine: &Routine,
    start: Option<DateTime<Utc>>,
) -> OpResult<Outcome<Vec<Action>>> {
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
    }
    .map_err(OpError::rejected)?;

    let mut anchors = context
        .build_anchors(Some(cursor))
        .map_err(OpError::rejected)?;
    for action in snapshot
        .actions
        .iter()
        .filter(|action| !action.pinned && !action.is_completed())
    {
        if let Some(start) = scheduled_time(action) {
            let end = duration_end(start, context.effective_duration(action))
                .map_err(OpError::rejected)?;
            anchors.push((start, end));
        }
    }

    let mut actions = Vec::with_capacity(routine.steps.len());
    for step in &routine.steps {
        let duration = step_duration(step);
        let start = find_free_slot(cursor, duration, &anchors, context.config.granularity)
            .map_err(OpError::rejected)?;
        cursor = duration_end(start, duration).map_err(OpError::rejected)?;

        actions.push(
            Action::new(step.title.clone())
                .with_routine_id(routine.id)
                .with_duration(Some(duration))
                .with_queued(true)
                .with_start(Some(SchedulePoint::DateTime(start))),
        );
    }

    let mut changes = Changes::default();
    changes.put_all(actions.clone());
    Ok(Outcome::new(actions, changes.rescheduled()))
}
