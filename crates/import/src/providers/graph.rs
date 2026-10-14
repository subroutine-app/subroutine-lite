use std::fmt::Write as _;

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, SecondsFormat, Timelike, Utc};
use chronoutil::RelativeDuration;
use serde_json::Value;
use subroutine_core::{Action, Event, Marker, SchedulePoint};

#[cfg(feature = "outlook")]
use crate::provider::CalendarSource;
use crate::provider::SyncOutcome;

pub(crate) const DEFAULT_PAST_DAYS: i64 = 30;

pub(crate) const DEFAULT_FUTURE_DAYS: i64 = 90;

pub(crate) const PAGE_SIZE: usize = 500;

pub(crate) const MAX_PAGES: usize = 50;

pub(crate) const EVENT_SELECT: &str = "id,subject,isAllDay,isCancelled,isOrganizer,responseStatus,attendees,start,end,\
                                       bodyPreview,location,onlineMeeting,organizer,webLink,showAs";

pub(crate) const MESSAGE_SELECT: &str = "id,subject,bodyPreview,flag,from,webLink";

#[cfg(feature = "outlook")]
pub(crate) const CALENDAR_SELECT: &str = "id,name,isDefaultCalendar";

pub(crate) const FLAGGED_FILTER: &str = "flag/flagStatus%20ne%20'notFlagged'";

pub(crate) fn calendar_view_url(
    calendar_id: Option<&str>,
    past_days: i64,
    future_days: i64,
    now: DateTime<Utc>,
) -> String {
    let start = now - Duration::days(past_days);
    let end = now + Duration::days(future_days);
    let path = match calendar_id {
        Some(calendar_id) => format!("/me/calendars/{calendar_id}/calendarView"),
        None => "/me/calendarView".to_owned(),
    };
    format!(
        "{path}?startDateTime={}&endDateTime={}&$select={EVENT_SELECT}&$top={PAGE_SIZE}",
        start.to_rfc3339_opts(SecondsFormat::Secs, true),
        end.to_rfc3339_opts(SecondsFormat::Secs, true),
    )
}

#[cfg(feature = "workiq")]
pub(crate) fn calendar_url(past_days: i64, future_days: i64, now: DateTime<Utc>) -> String {
    calendar_view_url(None, past_days, future_days, now)
}

#[cfg(feature = "outlook")]
pub(crate) fn calendars_url() -> String {
    format!("/me/calendars?$select={CALENDAR_SELECT}&$top={PAGE_SIZE}")
}

pub(crate) fn messages_url() -> String {
    format!("/me/messages?$select={MESSAGE_SELECT}&$filter={FLAGGED_FILTER}&$top={PAGE_SIZE}")
}

// Preserve nextLink encoding: WorkIQ policy rejects a literal $skip.
#[cfg(feature = "workiq")]
pub(crate) fn relative_graph_path(link: &str) -> Option<String> {
    let after_scheme = link.split_once("://")?.1;
    let (_host, path) = after_scheme.split_once('/')?;
    let (version, rest) = path.split_once('/')?;
    if !matches!(version, "v1.0" | "beta") {
        tracing::warn!(%link, "unrecognised Graph nextLink; stopping pagination");
        return None;
    }
    Some(format!("/{rest}"))
}

pub(crate) fn import_calendar_entry(
    outcome: &mut SyncOutcome,
    raw: &Value,
    provider_id: &'static str,
    accepted_events_only: bool,
) {
    if raw.get("isCancelled").and_then(Value::as_bool) == Some(true) {
        return;
    }
    let Some(external_id) = raw.get("id").and_then(Value::as_str) else {
        tracing::warn!("skipping calendar entry with no id");
        return;
    };
    if accepted_events_only && !calendar_entry_is_accepted(raw) {
        outcome.exclude(external_id);
        return;
    }
    let version = raw
        .get("@odata.etag")
        .and_then(Value::as_str)
        .map(str::to_owned);

    if raw.get("isAllDay").and_then(Value::as_bool) == Some(true) {
        match convert_all_day_event(raw, external_id, provider_id) {
            Some(marker) => outcome.push_marker(external_id.to_owned(), version, marker),
            None => tracing::warn!(%external_id, "skipping undated all-day event"),
        }
    } else {
        match convert_event(raw, external_id, provider_id) {
            Some(event) => outcome.push_event(external_id.to_owned(), version, event),
            None => tracing::warn!(%external_id, "skipping event with no usable start"),
        }
    }
}

fn calendar_entry_is_accepted(raw: &Value) -> bool {
    if raw.get("isOrganizer").and_then(Value::as_bool) == Some(true) {
        return true;
    }

    match raw
        .get("responseStatus")
        .and_then(|status| status.get("response"))
        .and_then(Value::as_str)
    {
        Some("accepted" | "organizer") => true,
        Some("declined" | "notResponded" | "tentativelyAccepted") => false,
        _ => raw
            .get("attendees")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty),
    }
}

pub(crate) fn import_flagged_message(
    outcome: &mut SyncOutcome,
    raw: &Value,
    provider_id: &'static str,
) {
    let Some(external_id) = raw.get("id").and_then(Value::as_str) else {
        tracing::warn!("skipping message with no id");
        return;
    };
    let action = convert_flagged_message(raw, external_id, provider_id);
    let version = Some(flag_version(raw));
    outcome.push_action(external_id.to_owned(), version, action);
}

#[cfg(feature = "outlook")]
pub(crate) fn convert_calendar(raw: &Value) -> Option<CalendarSource> {
    let id = raw.get("id").and_then(Value::as_str)?;
    let name = raw
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(id);
    Some(CalendarSource {
        id: id.to_owned(),
        name: name.to_owned(),
        is_default: raw
            .get("isDefaultCalendar")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

pub(crate) fn convert_event(
    raw: &Value,
    external_id: &str,
    provider_id: &'static str,
) -> Option<Event> {
    let start = graph_datetime(raw.get("start"))?;
    let span = graph_datetime(raw.get("end")).map(|end| end - start);
    let duration = match span {
        Some(span) if span.num_seconds() > 0 => RelativeDuration::from(span),
        _ => RelativeDuration::hours(1),
    };

    let mut event = Event::new(subject(raw), start, duration)
        .with_content(build_event_content(raw))
        .with_source_provider(provider_id)
        .with_source_external_id(external_id.to_owned());
    event.source_busy = match raw.get("showAs").and_then(Value::as_str) {
        Some("free" | "workingElsewhere") => Some(false),
        Some("busy" | "tentative" | "oof") => Some(true),
        _ => None,
    };
    Some(event)
}

pub(crate) fn convert_all_day_event(
    raw: &Value,
    external_id: &str,
    provider_id: &'static str,
) -> Option<Marker> {
    let start_date = graph_date(raw.get("start"))?;

    let end_date = graph_date(raw.get("end"))
        .and_then(|end| end.pred_opt())
        .filter(|end| *end > start_date);

    let mut marker = Marker::new(subject(raw), start_date)
        .with_source_provider(provider_id)
        .with_source_external_id(external_id.to_owned());
    marker.content = build_event_content(raw);
    marker.end_date = end_date;
    Some(marker)
}

pub(crate) fn convert_flagged_message(
    raw: &Value,
    external_id: &str,
    provider_id: &'static str,
) -> Action {
    let flag = raw.get("flag");
    let start = graph_datetime(flag.and_then(|f| f.get("dueDateTime")))
        .or_else(|| graph_datetime(flag.and_then(|f| f.get("startDateTime"))))
        .map(schedule_point);

    let completion = (flag_status(raw) == "complete").then(|| {
        graph_datetime(flag.and_then(|f| f.get("completedDateTime"))).unwrap_or_else(Utc::now)
    });

    Action::new(subject(raw))
        .with_content(build_message_content(raw))
        .with_queued(completion.is_none() && matches!(start, Some(SchedulePoint::DateTime(_))))
        .with_start(start)
        .with_completion(completion)
        .with_source_provider(provider_id)
        .with_source_external_id(external_id.to_owned())
}

pub(crate) fn flag_version(raw: &Value) -> String {
    let flag = raw.get("flag");
    let part = |field: &str| {
        flag.and_then(|f| f.get(field))
            .and_then(|v| v.get("dateTime"))
            .and_then(Value::as_str)
            .unwrap_or("-")
            .to_owned()
    };
    format!(
        "{}|{}|{}|{}|{}",
        flag_status(raw),
        part("dueDateTime"),
        part("startDateTime"),
        part("completedDateTime"),
        subject(raw),
    )
}

pub(crate) fn flag_status(raw: &Value) -> &str {
    raw.get("flag")
        .and_then(|f| f.get("flagStatus"))
        .and_then(Value::as_str)
        .unwrap_or("notFlagged")
}

pub(crate) fn subject(raw: &Value) -> String {
    raw.get("subject")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|subject| !subject.is_empty())
        .unwrap_or("(no subject)")
        .to_owned()
}

pub(crate) fn build_event_content(raw: &Value) -> Option<String> {
    let mut parts = String::new();
    push_preview(&mut parts, raw);

    if let Some(location) = raw
        .get("location")
        .and_then(|l| l.get("displayName"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        push_line(&mut parts, format_args!("[location: {location}]"));
    }
    if let Some(organizer) = raw
        .get("organizer")
        .and_then(|o| o.get("emailAddress"))
        .and_then(display_identity)
    {
        push_line(&mut parts, format_args!("[organizer: {organizer}]"));
    }
    if let Some(join_url) = raw
        .get("onlineMeeting")
        .and_then(|m| m.get("joinUrl"))
        .and_then(Value::as_str)
    {
        push_line(&mut parts, format_args!("[join: {join_url}]"));
    }
    push_web_link(&mut parts, raw);

    (!parts.is_empty()).then_some(parts)
}

pub(crate) fn build_message_content(raw: &Value) -> Option<String> {
    let mut parts = String::new();
    push_preview(&mut parts, raw);

    if let Some(from) = raw
        .get("from")
        .and_then(|f| f.get("emailAddress"))
        .and_then(display_identity)
    {
        push_line(&mut parts, format_args!("[from: {from}]"));
    }
    push_web_link(&mut parts, raw);

    (!parts.is_empty()).then_some(parts)
}

pub(crate) fn push_preview(parts: &mut String, raw: &Value) {
    if let Some(preview) = raw
        .get("bodyPreview")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push_str(preview);
    }
}

pub(crate) fn push_web_link(parts: &mut String, raw: &Value) {
    if let Some(link) = raw.get("webLink").and_then(Value::as_str) {
        push_line(parts, format_args!("[open: {link}]"));
    }
}

pub(crate) fn push_line(parts: &mut String, line: std::fmt::Arguments<'_>) {
    if !parts.is_empty() {
        parts.push('\n');
    }
    let _ = parts.write_fmt(line);
}

pub(crate) fn display_identity(email_address: &Value) -> Option<String> {
    let name = email_address
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let address = email_address
        .get("address")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match (name, address) {
        (Some(name), Some(address)) if name != address => Some(format!("{name} <{address}>")),
        (Some(value), _) | (None, Some(value)) => Some(value.to_owned()),
        (None, None) => None,
    }
}

pub(crate) fn schedule_point(due: DateTime<Utc>) -> SchedulePoint {
    if due.hour() == 0 && due.minute() == 0 && due.second() == 0 {
        SchedulePoint::Date(due.date_naive())
    } else {
        SchedulePoint::DateTime(due)
    }
}

pub(crate) fn graph_datetime(field: Option<&Value>) -> Option<DateTime<Utc>> {
    let field = field?;
    let raw = field.get("dateTime").and_then(Value::as_str)?;

    if let Ok(parsed) = DateTime::parse_from_rfc3339(raw) {
        return Some(parsed.with_timezone(&Utc));
    }

    let zone = field
        .get("timeZone")
        .and_then(Value::as_str)
        .unwrap_or("UTC");
    if !matches!(
        zone,
        "UTC" | "Etc/GMT" | "GMT Standard Time" | "tzone://Microsoft/Utc"
    ) {
        tracing::warn!(%zone, "Graph returned a non-UTC time zone; reading the value as UTC");
    }

    NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%.f")
        .ok()
        .map(|naive| naive.and_utc())
}

pub(crate) fn graph_date(field: Option<&Value>) -> Option<NaiveDate> {
    graph_datetime(field).map(|value| value.date_naive())
}
