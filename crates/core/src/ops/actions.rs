use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{Action, BatchPlacement, CompleteResult, SchedulePoint};

use super::{Changes, OpError, OpResult, Outcome, Snapshot, item::Identify, put};

pub fn create(mut action: Action) -> Outcome<Action> {
    action.ensure_id();
    if action.queued && action.start.is_none() {
        action.set_queued(false);
    }
    put(action)
}

pub fn update(previous: Option<&Action>, incoming: Action) -> Outcome<Action> {
    let newly_completed = incoming.is_completed()
        && incoming.recurrence.is_some()
        && incoming.source_provider.is_none()
        && previous.is_some_and(|p| !p.is_completed());

    let mut changes = Changes::default();
    changes.put(incoming.clone());
    if let Some(next) = newly_completed
        .then(|| previous.and_then(Action::next_occurrence))
        .flatten()
    {
        changes.put(next);
    }
    Outcome::new(incoming, changes)
}

pub fn queue(snapshot: &Snapshot, id: Uuid) -> OpResult<Outcome<Vec<Action>>> {
    let action = snapshot.action(id)?;
    if action.queued {
        return Err(OpError::rejected(format!("action {id} is already queued")));
    }

    let mut queued = action.clone();
    queued.set_queued(true);
    queued.set_start(Some(SchedulePoint::DateTime(
        snapshot.context().next_slot_for(action),
    )));

    let mut actions = snapshot.actions.clone();
    if let Some(slot) = actions.iter_mut().find(|a| a.id == id) {
        *slot = queued.clone();
    }
    let settled = snapshot.context_with(&actions).requeue_actions();

    let queued = settled
        .iter()
        .find(|a| a.id == id)
        .cloned()
        .unwrap_or(queued);
    let mut moved = vec![queued];
    moved.extend(settled.into_iter().filter(|a| a.id != id));

    let mut changes = Changes::default();
    changes.put_all(moved.clone());
    Ok(Outcome::new(moved, changes.rescheduled()))
}

pub fn batch(
    snapshot: &Snapshot,
    mut action: Action,
    cursor: Option<DateTime<Utc>>,
) -> Outcome<BatchPlacement> {
    action.ensure_id();

    let context = snapshot.context();
    let cursor = cursor.unwrap_or_else(|| context.batch_start());
    let placement = context.place_in_batch(cursor, action);

    let mut changes = Changes::default();
    changes.put_all(placement.moved().cloned());
    Outcome::new(placement, changes.rescheduled())
}

pub fn backlog(mut action: Action) -> Outcome<Action> {
    action.set_queued(false);
    action.set_start(None);
    action.set_pinned(false);
    put(action)
}

pub fn set_pinned(mut action: Action, pinned: bool) -> OpResult<Outcome<Action>> {
    if pinned && !action.is_scheduled() {
        return Err(OpError::rejected(format!(
            "action {} must be queued at a time before it can be pinned",
            action.id
        )));
    }
    action.set_pinned(pinned);
    Ok(put(action))
}

pub fn complete(action: Action, now: DateTime<Utc>) -> Outcome<CompleteResult> {
    let next = action
        .source_provider
        .is_none()
        .then(|| action.next_occurrence())
        .flatten();

    if action.is_completed() {
        return Outcome::new(
            CompleteResult {
                completed: action,
                next,
            },
            Changes::default(),
        );
    }

    let mut completed = action;
    completed.set_completion(Some(now));
    completed.set_queued(false);
    completed.set_pinned(false);

    let mut changes = Changes::default();
    changes.put(completed.clone());
    if let Some(next) = next.clone() {
        changes.put(next);
    }
    Outcome::new(CompleteResult { completed, next }, changes)
}

pub fn uncomplete(mut action: Action) -> Outcome<Action> {
    if !action.is_completed() {
        return Outcome::new(action, Changes::default());
    }

    action.set_completion(None);
    action.set_recurrence(None);
    put(action)
}

pub fn clear_duration(mut action: Action) -> Outcome<Action> {
    action.duration = None;
    put(action)
}
