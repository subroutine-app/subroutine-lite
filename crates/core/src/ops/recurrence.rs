
use std::collections::HashMap;

use chrono::{DateTime, Local, Utc};
use uuid::Uuid;

use crate::{
    Action, Event, Marker, Recurrence, Routine, SchedulePoint, Signal, recurrence::occurrence_id,
};

use super::{Changes, Delete, Outcome, Snapshot};

const MAX_ROUTINE_ADVANCES: usize = 4096;
const ROUTINE_GRACE: chrono::Duration = chrono::Duration::hours(6);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReconcileSummary {
    pub events: usize,
    pub markers: usize,
    pub signals: usize,
    pub routine_runs: usize,
    pub routine_actions: usize,
    pub routine_skipped: usize,
}

impl ReconcileSummary {
    pub fn total(self) -> usize {
        self.events
            + self.markers
            + self.signals
            + self.routine_runs
            + self.routine_actions
            + self.routine_skipped
    }
}

pub fn reconcile(snapshot: &Snapshot) -> Outcome<ReconcileSummary> {
    let now = snapshot.now_utc();
    let mut changes = Changes::default();
    let mut summary = ReconcileSummary::default();

    retire_materialized_occurrences(snapshot, &mut changes, &mut summary);

    for routine in &snapshot.routines {
        reconcile_routine(snapshot, routine, now, &mut changes, &mut summary);
    }

    if summary.routine_actions > 0 {
        changes = changes.rescheduled();
    }
    Outcome::new(summary, changes)
}

fn retire_materialized_occurrences(
    snapshot: &Snapshot,
    changes: &mut Changes,
    summary: &mut ReconcileSummary,
) {
    let event_roots: HashMap<Uuid, &Event> = snapshot
        .events
        .iter()
        .filter(|event| event.id == event.lineage_id)
        .map(|event| (event.lineage_id, event))
        .collect();
    for event in snapshot.events.iter().filter(|event| {
        event.source_provider.is_none()
            && event.recurrence.is_some()
            && event.id != event.lineage_id
            && event.id == occurrence_id(event.lineage_id, "event", event.start.into())
    }) {
        let Some(root) = event_roots.get(&event.lineage_id) else {
            continue;
        };
        if unchanged_event_occurrence(event, root) {
            changes.delete(Delete::Event(event.id));
            summary.events += 1;
        }
    }

    let marker_roots: HashMap<Uuid, &Marker> = snapshot
        .markers
        .iter()
        .filter(|marker| marker.id == marker.lineage_id)
        .map(|marker| (marker.lineage_id, marker))
        .collect();
    for marker in snapshot.markers.iter().filter(|marker| {
        marker.source_provider.is_none()
            && marker.recurrence.is_some()
            && marker.id != marker.lineage_id
            && marker.id == occurrence_id(marker.lineage_id, "marker", marker.date.into())
    }) {
        let Some(root) = marker_roots.get(&marker.lineage_id) else {
            continue;
        };
        if unchanged_marker_occurrence(marker, root) {
            changes.delete(Delete::Marker(marker.id));
            summary.markers += 1;
        }
    }

    let signal_roots: HashMap<Uuid, &Signal> = snapshot
        .signals
        .iter()
        .filter(|signal| signal.id == signal.lineage_id)
        .map(|signal| (signal.lineage_id, signal))
        .collect();
    for signal in snapshot.signals.iter().filter(|signal| {
        signal.recurrence.is_some()
            && signal.id != signal.lineage_id
            && signal.id == occurrence_id(signal.lineage_id, "signal", signal.datetime.into())
    }) {
        let Some(root) = signal_roots.get(&signal.lineage_id) else {
            continue;
        };
        if unchanged_signal_occurrence(signal, root) {
            changes.delete(Delete::Signal(signal.id));
            summary.signals += 1;
        }
    }
}

fn same_series_rule(left: Option<Recurrence>, right: Option<Recurrence>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.rule == right.rule && left.end_date == right.end_date,
        _ => false,
    }
}

fn unchanged_event_occurrence(occurrence: &Event, root: &Event) -> bool {
    occurrence.template_id == root.template_id
        && occurrence.title == root.title
        && occurrence.content == root.content
        && occurrence.duration == root.duration
        && occurrence.source_busy == root.source_busy
        && occurrence.busy_override == root.busy_override
        && occurrence.source_provider == root.source_provider
        && occurrence.source_external_id == root.source_external_id
        && same_series_rule(occurrence.recurrence, root.recurrence)
}

fn unchanged_marker_occurrence(occurrence: &Marker, root: &Marker) -> bool {
    let occurrence_span = occurrence.end_date.map(|end| end - occurrence.date);
    let root_span = root.end_date.map(|end| end - root.date);
    occurrence.template_id == root.template_id
        && occurrence.title == root.title
        && occurrence.content == root.content
        && occurrence_span == root_span
        && occurrence.source_provider == root.source_provider
        && occurrence.source_external_id == root.source_external_id
        && same_series_rule(occurrence.recurrence, root.recurrence)
}

fn unchanged_signal_occurrence(occurrence: &Signal, root: &Signal) -> bool {
    occurrence.template_id == root.template_id
        && occurrence.title == root.title
        && occurrence.content == root.content
        && same_series_rule(occurrence.recurrence, root.recurrence)
}

fn reconcile_routine(
    snapshot: &Snapshot,
    routine: &Routine,
    now: DateTime<Utc>,
    changes: &mut Changes,
    summary: &mut ReconcileSummary,
) {
    let (Some(mut target), Some(mut recurrence)) = (routine.target, routine.recurrence) else {
        return;
    };
    let mut advanced = routine.clone();
    let mut changed = false;

    for _ in 0..MAX_ROUTINE_ADVANCES {
        if !is_due(target, snapshot.now, now) {
            break;
        }
        if recurrence
            .end_date
            .is_some_and(|end_date| target.date_naive() > end_date)
        {
            advanced.target = None;
            changed = true;
            break;
        }

        if should_run_routine(target, snapshot.now, now) {
            let actions = routine_actions(snapshot, routine, target);
            summary.routine_runs += 1;
            summary.routine_actions += actions.len();
            changes.put_all(actions);
        } else {
            summary.routine_skipped += 1;
        }
        changed = true;

        let Some((next, next_recurrence)) = recurrence.advance(target) else {
            advanced.target = None;
            advanced.recurrence = Some(recurrence.set_remaining(Some(0)));
            break;
        };
        target = next;
        recurrence = next_recurrence;
        advanced.target = Some(target);
        advanced.recurrence = Some(recurrence);
    }

    if changed {
        changes.put(advanced);
    }
}

fn is_due(target: SchedulePoint, now_local: DateTime<Local>, now_utc: DateTime<Utc>) -> bool {
    match target {
        SchedulePoint::DateTime(datetime) => datetime <= now_utc,
        SchedulePoint::Date(date) => date <= now_local.date_naive(),
    }
}

fn should_run_routine(
    target: SchedulePoint,
    now_local: DateTime<Local>,
    now_utc: DateTime<Utc>,
) -> bool {
    match target {
        SchedulePoint::DateTime(datetime) => datetime >= now_utc - ROUTINE_GRACE,
        SchedulePoint::Date(date) => date == now_local.date_naive(),
    }
}

fn routine_actions(snapshot: &Snapshot, routine: &Routine, target: SchedulePoint) -> Vec<Action> {
    let mut cursor = DateTime::<Utc>::from(target);
    routine
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            let duration = step
                .duration
                .unwrap_or(snapshot.settings.default_step_duration);
            let occurrence_key = format!("routine-run:{target:?}:step:{index}");
            let id = Uuid::new_v5(&routine.id, occurrence_key.as_bytes());
            let action = Action::new(step.title.clone())
                .with_recurrence_id(id)
                .with_routine_id(routine.id)
                .with_duration(Some(duration))
                .with_queued(true)
                .with_start(Some(SchedulePoint::DateTime(cursor)));
            cursor = cursor + duration;
            Action { id, ..action }
        })
        .collect()
}
