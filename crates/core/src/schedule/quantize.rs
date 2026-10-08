use chrono::{DateTime, Duration, Utc};

pub fn quantize_floor(dt: DateTime<Utc>, granularity: Duration) -> DateTime<Utc> {
    let segment_secs = granularity.num_seconds().max(1);
    let total_seconds = dt.timestamp();
    let remainder = total_seconds.rem_euclid(segment_secs);
    DateTime::from_timestamp(total_seconds - remainder, 0).unwrap_or(dt)
}

pub fn quantize_ceil(dt: DateTime<Utc>, granularity: Duration) -> DateTime<Utc> {
    let segment_secs = granularity.num_seconds().max(1);
    let total_seconds = dt.timestamp();
    let remainder = total_seconds.rem_euclid(segment_secs);
    let target = if remainder == 0 && dt.timestamp_subsec_nanos() == 0 {
        total_seconds
    } else {
        total_seconds + (segment_secs - remainder)
    };
    DateTime::from_timestamp(target, 0).unwrap_or(dt)
}

pub fn quantize_duration(duration: Duration, granularity: Duration) -> Duration {
    let segment_secs = granularity.num_seconds().max(1);
    let total_seconds = duration.num_seconds();
    let remainder = total_seconds.rem_euclid(segment_secs);
    if remainder == 0 {
        duration
    } else {
        duration + Duration::seconds(segment_secs - remainder)
    }
}
