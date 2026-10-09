use crate::Action;

use super::{Changes, OpError, OpResult, Outcome, Snapshot};

pub fn refresh(snapshot: &Snapshot) -> OpResult<Outcome<Vec<Action>>> {
    snapshot
        .context()
        .requeue_actions()
        .map(moved)
        .map_err(OpError::rejected)
}

pub fn expedite(snapshot: &Snapshot) -> OpResult<Outcome<Vec<Action>>> {
    let horizon = snapshot
        .now_utc()
        .checked_add_signed(snapshot.settings.expedite_horizon)
        .ok_or_else(|| {
            OpError::rejected("expedite horizon is outside the supported calendar range")
        })?;
    snapshot
        .context()
        .expedite_actions(horizon)
        .map(moved)
        .map_err(OpError::rejected)
}

pub fn auto_queue(snapshot: &Snapshot) -> OpResult<Outcome<Vec<Action>>> {
    snapshot
        .context()
        .auto_queue_due_backlogged()
        .map(moved)
        .map_err(OpError::rejected)
}

fn moved(actions: Vec<Action>) -> Outcome<Vec<Action>> {
    let mut changes = Changes::default();
    changes.put_all(actions.clone());
    Outcome::new(actions, changes.rescheduled())
}
