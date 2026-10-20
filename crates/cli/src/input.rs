use std::{fs::File, io, path::Path};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use chronoutil::RelativeDuration;
use serde_json::Value;
use subroutine_core::SchedulePoint;

pub fn title(value: &str) -> Result<String, String> {
    if value.trim().is_empty() {
        Err("title must not be blank".into())
    } else {
        Ok(value.to_owned())
    }
}

pub fn datetime(value: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Utc))
        .map_err(|_| {
            "expected an RFC3339 timestamp with an offset, e.g. 2026-10-19T09:00:00+02:00".into()
        })
}

pub fn schedule_point(value: &str) -> Result<SchedulePoint, String> {
    if let Ok(date) = value.parse::<NaiveDate>() {
        Ok(SchedulePoint::Date(date))
    } else {
        datetime(value).map(SchedulePoint::DateTime)
    }
}

pub fn duration(value: &str) -> Result<RelativeDuration, String> {
    let value = value.trim();
    let duration = if value.starts_with('P') {
        subroutine_core::parse_iso8601_duration(value)?
    } else {
        parser::parse_duration_expr(value)
            .map_err(|error| error.to_string())?
            .ok_or("expected a duration, e.g. 30m, 1h30m, or PT30M")?
            .into()
    };
    subroutine_core::checked_duration_end(
        SchedulePoint::DateTime(DateTime::<Utc>::MIN_UTC),
        duration,
    )
    .map_err(str::to_owned)?;
    Ok(duration)
}

pub fn fixed_duration(value: &str) -> Result<Duration, String> {
    let duration = duration(value)?;
    if !duration.with_duration(Duration::zero()).is_zero() {
        return Err("expected a fixed duration, without months or years".into());
    }
    let start = DateTime::<Utc>::UNIX_EPOCH;
    let end =
        subroutine_core::checked_duration_end(start.into(), duration).map_err(str::to_owned)?;
    let duration = DateTime::<Utc>::from(end) - start;
    if duration <= Duration::zero() {
        return Err("duration must be positive".into());
    }
    Ok(duration)
}

pub fn json(path: &Path) -> Result<Value> {
    if path == Path::new("-") {
        serde_json::from_reader(io::stdin().lock()).context("read JSON from stdin")
    } else {
        let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
        serde_json::from_reader(io::BufReader::new(file))
            .with_context(|| format!("read JSON from {}", path.display()))
    }
}
