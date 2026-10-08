use chrono::{DateTime, Duration, Utc};
use chronoutil::RelativeDuration;

mod config;
mod point;
mod quantize;
pub use config::*;
pub use point::*;
pub use quantize::*;

use crate::Action;

pub(crate) fn scheduled_time(action: &Action) -> Option<DateTime<Utc>> {
    if !action.is_scheduled() {
        return None;
    }
    action.start.map(DateTime::<Utc>::from)
}

pub(crate) fn find_free_slot_backward(
    latest: DateTime<Utc>,
    slot_duration: RelativeDuration,
    unavailable: &[(DateTime<Utc>, DateTime<Utc>)],
    floor: DateTime<Utc>,
    granularity: Duration,
) -> DateTime<Utc> {
    let mut sorted = unavailable.to_vec();
    sorted.sort_by_key(|(_, end)| std::cmp::Reverse(*end));

    let candidate_end = sorted
        .into_iter()
        .fold(latest, |candidate_end, (start, end)| {
            let candidate_start = (candidate_end - slot_duration).max(floor);
            if candidate_start < end && candidate_end > start {
                quantize_floor(start, granularity)
            } else {
                candidate_end
            }
        });

    (candidate_end - slot_duration).max(floor)
}

pub(crate) fn find_free_slot(
    earliest: DateTime<Utc>,
    slot_duration: RelativeDuration,
    unavailable: &[(DateTime<Utc>, DateTime<Utc>)],
    granularity: Duration,
) -> DateTime<Utc> {
    let mut sorted = unavailable.to_vec();
    sorted.sort_by_key(|(start, _)| *start);

    sorted
        .into_iter()
        .fold(earliest, |candidate, (start, end)| {
            let candidate_end = candidate + slot_duration;
            if candidate < end && candidate_end > start {
                quantize_ceil(end, granularity)
            } else {
                candidate
            }
        })
}

pub(crate) fn requeue_at(action: &Action, time: DateTime<Utc>) -> Action {
    let mut updated = action.clone();
    updated.set_queued(true);
    updated.set_start(Some(SchedulePoint::DateTime(time)));
    updated
}
