use chrono::{DateTime, Duration, Utc};

pub(super) fn granularity_seconds(granularity: Duration) -> Result<i64, &'static str> {
    let seconds = granularity.num_seconds();
    if seconds <= 0 || Duration::try_seconds(seconds) != Some(granularity) {
        return Err("granularity must be a positive whole number of seconds");
    }
    Ok(seconds)
}

pub fn quantize_floor(
    dt: DateTime<Utc>,
    granularity: Duration,
) -> Result<DateTime<Utc>, &'static str> {
    let seconds = granularity_seconds(granularity)?;
    let target = dt
        .timestamp()
        .checked_sub(dt.timestamp().rem_euclid(seconds));
    target
        .and_then(|target| DateTime::from_timestamp(target, 0))
        .ok_or("quantized time is outside the supported calendar range")
}

pub fn quantize_ceil(
    dt: DateTime<Utc>,
    granularity: Duration,
) -> Result<DateTime<Utc>, &'static str> {
    let seconds = granularity_seconds(granularity)?;
    let remainder = dt.timestamp().rem_euclid(seconds);
    let target = if remainder == 0 && dt.timestamp_subsec_nanos() == 0 {
        Some(dt.timestamp())
    } else {
        dt.timestamp().checked_add(seconds - remainder)
    };
    target
        .and_then(|target| DateTime::from_timestamp(target, 0))
        .ok_or("quantized time is outside the supported calendar range")
}

pub fn quantize_duration(
    duration: Duration,
    granularity: Duration,
) -> Result<Duration, &'static str> {
    let seconds = granularity_seconds(granularity)?;
    if duration < Duration::zero() {
        return Err("duration cannot be negative");
    }
    let whole_seconds = duration.num_seconds();
    let remainder = whole_seconds.rem_euclid(seconds);
    let target = if remainder == 0 && duration.subsec_nanos() == 0 {
        Some(whole_seconds)
    } else {
        whole_seconds.checked_add(seconds - remainder)
    };
    target
        .and_then(Duration::try_seconds)
        .ok_or("quantized duration is out of range")
}
