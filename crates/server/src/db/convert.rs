use chrono::{DateTime, NaiveDate, Utc};
use chronoutil::RelativeDuration;
use sqlx::{Row, postgres::PgRow, types::Json};

use subroutine_core::{Recurrence, SchedulePoint};

pub(crate) fn duration_to_sql(duration: RelativeDuration) -> String {
    duration.format_to_iso8601()
}

pub(crate) fn opt_duration_to_sql(duration: Option<RelativeDuration>) -> Option<String> {
    duration.map(duration_to_sql)
}

fn parse_duration(raw: &str) -> Result<RelativeDuration, sqlx::Error> {
    RelativeDuration::parse_from_iso8601(raw)
        .map_err(|e| sqlx::Error::Decode(format!("invalid ISO-8601 duration '{raw}': {e}").into()))
}

pub(crate) fn duration_from_row(
    row: &PgRow,
    column: &str,
) -> Result<RelativeDuration, sqlx::Error> {
    parse_duration(&row.try_get::<String, _>(column)?)
}

pub(crate) fn opt_duration_from_row(
    row: &PgRow,
    column: &str,
) -> Result<Option<RelativeDuration>, sqlx::Error> {
    row.try_get::<Option<String>, _>(column)?
        .as_deref()
        .map(parse_duration)
        .transpose()
}

pub(crate) fn recurrence_to_sql(recurrence: Option<Recurrence>) -> Option<Json<Recurrence>> {
    recurrence.map(Json)
}

pub(crate) fn recurrence_from_row(
    row: &PgRow,
    column: &str,
) -> Result<Option<Recurrence>, sqlx::Error> {
    Ok(row
        .try_get::<Option<Json<Recurrence>>, _>(column)?
        .map(|json| json.0))
}

pub(crate) fn schedule_point_to_sql(
    point: Option<SchedulePoint>,
) -> (Option<DateTime<Utc>>, Option<NaiveDate>) {
    match point {
        Some(SchedulePoint::DateTime(at)) => (Some(at), None),
        Some(SchedulePoint::Date(date)) => (None, Some(date)),
        None => (None, None),
    }
}

pub(crate) fn schedule_point_from_row(
    row: &PgRow,
    at_column: &str,
    date_column: &str,
) -> Result<Option<SchedulePoint>, sqlx::Error> {
    if let Some(at) = row.try_get::<Option<DateTime<Utc>>, _>(at_column)? {
        return Ok(Some(SchedulePoint::DateTime(at)));
    }
    Ok(row
        .try_get::<Option<NaiveDate>, _>(date_column)?
        .map(SchedulePoint::Date))
}
