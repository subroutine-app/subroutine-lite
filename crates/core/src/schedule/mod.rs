use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, Utc};
use chronoutil::RelativeDuration;

mod config;
mod point;
mod quantize;
pub use config::*;
pub use point::*;
pub use quantize::*;

use crate::{Action, checked_duration_end};

pub(crate) fn scheduled_time(action: &Action) -> Option<DateTime<Utc>> {
    if !action.is_scheduled() {
        return None;
    }
    action.start.map(DateTime::<Utc>::from)
}

pub(crate) fn duration_end(
    start: DateTime<Utc>,
    duration: RelativeDuration,
) -> Result<DateTime<Utc>, &'static str> {
    checked_duration_end(SchedulePoint::DateTime(start), duration).map(DateTime::<Utc>::from)
}

pub(crate) fn overlaps(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    unavailable: &[(DateTime<Utc>, DateTime<Utc>)],
) -> bool {
    start < end
        && unavailable.iter().any(|&(other_start, other_end)| {
            other_start < other_end && start < other_end && end > other_start
        })
}

fn latest_start_on_day(
    day: NaiveDate,
    latest: DateTime<Utc>,
    duration: RelativeDuration,
    floor: DateTime<Utc>,
    max_start: DateTime<Utc>,
    granularity: Duration,
) -> Option<DateTime<Utc>> {
    let seconds = granularity.num_seconds();
    let first = quantize_ceil(
        day.and_time(NaiveTime::MIN).and_utc().max(floor),
        granularity,
    )
    .ok()?;
    let last = day
        .and_hms_opt(23, 59, 59)?
        .and_utc()
        .min(latest)
        .min(max_start);
    let mut low = first.timestamp().div_euclid(seconds);
    let mut high = last.timestamp().div_euclid(seconds);
    let mut result = None;
    while low <= high {
        let mid = low.checked_add(high.checked_sub(low)? / 2)?;
        let start = DateTime::from_timestamp(mid.checked_mul(seconds)?, 0)?;
        if duration_end(start, duration).is_ok_and(|end| end <= latest) {
            result = Some(start);
            low = mid.checked_add(1)?;
        } else {
            high = mid.checked_sub(1)?;
        }
    }
    result
}

fn latest_start(
    latest: DateTime<Utc>,
    duration: RelativeDuration,
    floor: DateTime<Utc>,
    max_start: DateTime<Utc>,
    granularity: Duration,
) -> Option<DateTime<Utc>> {
    let max_start = max_start.min(latest);
    let mut low = floor.date_naive().num_days_from_ce();
    let mut high = max_start.date_naive().num_days_from_ce();
    let mut day = None;
    while low <= high {
        let mid = low.checked_add(high.checked_sub(low)? / 2)?;
        let date = NaiveDate::from_num_days_from_ce_opt(mid)?;
        let start = date.and_time(NaiveTime::MIN).and_utc();
        if duration_end(start, duration).is_ok_and(|end| end <= latest) {
            day = Some(date);
            low = mid.checked_add(1)?;
        } else {
            high = mid.checked_sub(1)?;
        }
    }

    let mut day = day?;
    while day >= floor.date_naive() {
        if let Some(start) =
            latest_start_on_day(day, latest, duration, floor, max_start, granularity)
        {
            return Some(start);
        }
        let previous_end = day.pred_opt()?.and_hms_opt(23, 59, 59)?.and_utc();
        day = quantize_floor(previous_end, granularity).ok()?.date_naive();
    }
    None
}

pub(crate) fn find_free_slot_backward(
    latest: DateTime<Utc>,
    slot_duration: RelativeDuration,
    unavailable: &[(DateTime<Utc>, DateTime<Utc>)],
    floor: DateTime<Utc>,
    max_start: DateTime<Utc>,
    granularity: Duration,
) -> Result<DateTime<Utc>, &'static str> {
    quantize::granularity_seconds(granularity)?;
    duration_end(DateTime::<Utc>::MIN_UTC, slot_duration)?;
    let floor = quantize_ceil(floor, granularity)?;
    let mut sorted = unavailable.to_vec();
    sorted.sort_by_key(|(_, end)| std::cmp::Reverse(*end));

    let no_slot = "not enough free time before the expedite horizon; schedule unchanged";
    let mut candidate =
        latest_start(latest, slot_duration, floor, max_start, granularity).ok_or(no_slot)?;
    for (start, end) in sorted {
        if end < start {
            return Err("blocking interval ends before it starts");
        }
        let candidate_end = duration_end(candidate, slot_duration)?;
        if overlaps(candidate, candidate_end, &[(start, end)]) {
            candidate =
                latest_start(start, slot_duration, floor, max_start, granularity).ok_or(no_slot)?;
        }
    }
    Ok(candidate)
}

pub(crate) fn find_free_slot(
    earliest: DateTime<Utc>,
    slot_duration: RelativeDuration,
    unavailable: &[(DateTime<Utc>, DateTime<Utc>)],
    granularity: Duration,
) -> Result<DateTime<Utc>, &'static str> {
    let mut sorted = unavailable.to_vec();
    sorted.sort_by_key(|(start, _)| *start);

    let mut candidate = quantize_ceil(earliest, granularity)?;
    for (start, end) in sorted {
        if end < start {
            return Err("blocking interval ends before it starts");
        }
        let candidate_end = duration_end(candidate, slot_duration)?;
        if overlaps(candidate, candidate_end, &[(start, end)]) {
            candidate = quantize_ceil(end, granularity)?;
        }
    }
    duration_end(candidate, slot_duration)?;
    Ok(candidate)
}

pub(crate) fn requeue_at(action: &Action, time: DateTime<Utc>) -> Action {
    let mut updated = action.clone();
    updated.set_queued(true);
    updated.set_start(Some(SchedulePoint::DateTime(time)));
    updated
}
