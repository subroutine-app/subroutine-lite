use crate::Action;

use super::{Changes, Outcome, Snapshot};

pub fn refresh(snapshot: &Snapshot) -> Outcome<Vec<Action>> {
    moved(snapshot.context().requeue_actions())
}

pub fn expedite(snapshot: &Snapshot) -> Outcome<Vec<Action>> {
    let horizon = snapshot.now_utc() + snapshot.settings.expedite_horizon;
    moved(snapshot.context().expedite_actions(horizon))
}

pub fn auto_queue(snapshot: &Snapshot) -> Outcome<Vec<Action>> {
    moved(snapshot.context().auto_queue_due_backlogged())
}

fn moved(actions: Vec<Action>) -> Outcome<Vec<Action>> {
    let mut changes = Changes::default();
    changes.put_all(actions.clone());
    Outcome::new(actions, changes.rescheduled())
}
