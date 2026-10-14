use std::fmt::Write as _;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration, NaiveDate, SecondsFormat, Utc};
use chronoutil::RelativeDuration;
use reqwest::{Client, StatusCode};
use serde_json::Value;
use subroutine_core::{Event, Marker};

use crate::provider::{CalendarSource, Provider, SyncOutcome};

const DEFAULT_PAST_DAYS: i64 = 30;

const DEFAULT_FUTURE_DAYS: i64 = 90;

const PAGE_SIZE: usize = 2500;

const MAX_PAGES: usize = 50;

const CALENDAR_LIST_PAGE_SIZE: usize = 250;

const PRIMARY_CALENDAR_URL: &str =
    "https://www.googleapis.com/calendar/v3/users/me/calendarList/primary";

const CALENDAR_LIST_URL: &str = "https://www.googleapis.com/calendar/v3/users/me/calendarList";

const CALENDARS_URL: &str = "https://www.googleapis.com/calendar/v3/calendars";

const PRIMARY_CALENDAR_ID: &str = "primary";

pub struct GoogleCalendarProvider {
    client: Client,
    access_token: String,
    past_days: i64,
    future_days: i64,
    calendar_ids: Vec<String>,
    accepted_events_only: bool,
    runtime: tokio::runtime::Runtime,
}

impl GoogleCalendarProvider {
    pub fn new(access_token: String) -> Self {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to create tokio runtime for Google Calendar provider");
        let client = {
            let _guard = runtime.enter();
            Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("failed to create HTTP client for Google Calendar provider")
        };
        Self {
            client,
            access_token,
            past_days: DEFAULT_PAST_DAYS,
            future_days: DEFAULT_FUTURE_DAYS,
            calendar_ids: Vec::new(),
            accepted_events_only: false,
            runtime,
        }
    }

    pub fn with_window(mut self, past_days: i64, future_days: i64) -> Self {
        self.past_days = past_days.max(0);
        self.future_days = future_days.max(0);
        self
    }

    pub fn with_calendars(mut self, calendar_ids: Vec<String>) -> Self {
        self.calendar_ids = calendar_ids;
        self
    }

    pub fn accepted_events_only(mut self) -> Self {
        self.accepted_events_only = true;
        self
    }

    fn block_on<F: std::future::Future>(&self, f: F) -> F::Output {
        self.runtime.block_on(f)
    }

    async fn get(&self, url: &str, query: &[(&str, String)]) -> Result<Value> {
        tracing::debug!(%url, "google calendar get");
        let response = self
            .client
            .get(url)
            .bearer_auth(&self.access_token)
            .query(query)
            .send()
            .await
            .with_context(|| format!("could not reach {url}"))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .with_context(|| format!("could not read the response from {url}"))?;
        if !status.is_success() {
            bail!("{}", request_error(status, &body));
        }

        serde_json::from_str(&body).with_context(|| {
            format!(
                "google calendar returned output that is not JSON: {}",
                clip(&body)
            )
        })
    }

    async fn fetch_events(&self, calendar_id: &str, now: DateTime<Utc>) -> Result<Vec<Value>> {
        let url = events_url(calendar_id);
        let mut items = Vec::new();
        let mut next: Option<String> = None;

        for page in 1..=MAX_PAGES {
            let query = events_query(self.past_days, self.future_days, now, next.as_deref());
            let payload = self.get(&url, &query).await?;
            if let Some(values) = payload.get("items").and_then(Value::as_array) {
                items.extend(values.iter().cloned());
            }
            next = payload
                .get("nextPageToken")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if next.is_none() {
                return Ok(items);
            }
            if page == MAX_PAGES {
                tracing::warn!(
                    %calendar_id,
                    "google calendar stopped paging after {MAX_PAGES} pages ({} items); \
                     results are truncated",
                    items.len(),
                );
            }
        }

        Ok(items)
    }

    async fn fetch_calendar_list(&self) -> Result<Vec<Value>> {
        let mut items = Vec::new();
        let mut next: Option<String> = None;

        for page in 1..=MAX_PAGES {
            let query = calendar_list_query(next.as_deref());
            let payload = self.get(CALENDAR_LIST_URL, &query).await?;
            if let Some(values) = payload.get("items").and_then(Value::as_array) {
                items.extend(values.iter().cloned());
            }
            next = payload
                .get("nextPageToken")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if next.is_none() {
                return Ok(items);
            }
            if page == MAX_PAGES {
                tracing::warn!(
                    "google calendar stopped listing after {MAX_PAGES} pages ({} calendars); \
                     results are truncated",
                    items.len(),
                );
            }
        }

        Ok(items)
    }
}

impl Provider for GoogleCalendarProvider {
    fn id(&self) -> &'static str {
        "google_calendar"
    }

    fn name(&self) -> &'static str {
        "Google Calendar"
    }

    fn ensure_authorized(&mut self) -> Result<()> {
        let calendar = self
            .block_on(self.get(PRIMARY_CALENDAR_URL, &[]))
            .map_err(|error| {
                tracing::warn!(%error, "google calendar is not authorized");
                error.context("could not read the primary Google calendar")
            })?;

        let summary = text(calendar.get("summary"));
        if calendar.get("id").and_then(Value::as_str).is_none() {
            bail!("Google returned no primary calendar — sign in to Google again");
        }
        tracing::info!(
            calendar = summary.unwrap_or("<unknown>"),
            "google calendar authorized"
        );
        Ok(())
    }

    fn sync(&mut self) -> Result<SyncOutcome> {
        let mut outcome = SyncOutcome::new(self.id());
        let now = Utc::now();
        let calendars = selected_calendars(&self.calendar_ids);
        let account_calendar_id = if self.accepted_events_only {
            let primary = self.block_on(self.get(PRIMARY_CALENDAR_URL, &[]))?;
            Some(
                text(primary.get("id"))
                    .context("Google returned no primary calendar id")?
                    .to_owned(),
            )
        } else {
            None
        };

        tracing::info!(
            "google calendar sync starting ({} calendars, window: -{} to +{} days)",
            calendars.len(),
            self.past_days,
            self.future_days,
        );

        let mut fetched = 0;
        let mut failures = Vec::new();
        for calendar_id in &calendars {
            match self.block_on(self.fetch_events(calendar_id, now)) {
                Ok(events) => {
                    tracing::debug!(
                        %calendar_id,
                        "google calendar fetched {} calendar entries",
                        events.len(),
                    );
                    fetched += events.len();
                    for raw in &events {
                        import_entry(&mut outcome, raw, account_calendar_id.as_deref());
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        %calendar_id,
                        %error,
                        "skipping a google calendar that could not be read",
                    );
                    failures.push(error);
                }
            }
        }

        if failures.len() == calendars.len() {
            return Err(failures.remove(0));
        }
        tracing::info!(
            "google calendar fetched {fetched} calendar entries from {} calendars",
            calendars.len() - failures.len(),
        );

        tracing::info!(
            "google calendar sync complete — {} events, {} markers",
            outcome.events().count(),
            outcome.markers().count(),
        );
        Ok(outcome)
    }

    fn list_calendars(&mut self) -> Result<Vec<CalendarSource>> {
        let entries = self.block_on(self.fetch_calendar_list())?;
        let calendars: Vec<_> = entries.iter().filter_map(convert_calendar).collect();
        tracing::info!("google calendar listed {} calendars", calendars.len());
        Ok(calendars)
    }
}

fn selected_calendars(calendar_ids: &[String]) -> Vec<&str> {
    if calendar_ids.is_empty() {
        return vec![PRIMARY_CALENDAR_ID];
    }
    calendar_ids.iter().map(String::as_str).collect()
}

fn events_url(calendar_id: &str) -> String {
    format!(
        "{CALENDARS_URL}/{}/events",
        encode_path_segment(calendar_id)
    )
}

fn encode_path_segment(segment: &str) -> String {
    let mut encoded = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(char::from(byte));
            }
            _ => {
                let _ = write!(encoded, "%{byte:02X}");
            }
        }
    }
    encoded
}

fn events_query(
    past_days: i64,
    future_days: i64,
    now: DateTime<Utc>,
    page_token: Option<&str>,
) -> Vec<(&'static str, String)> {
    let start = now - Duration::days(past_days);
    let end = now + Duration::days(future_days);
    let mut query = vec![
        ("timeMin", start.to_rfc3339_opts(SecondsFormat::Secs, true)),
        ("timeMax", end.to_rfc3339_opts(SecondsFormat::Secs, true)),
        ("singleEvents", "true".to_owned()),
        ("orderBy", "startTime".to_owned()),
        ("maxResults", PAGE_SIZE.to_string()),
        ("showDeleted", "false".to_owned()),
    ];
    if let Some(page_token) = page_token {
        query.push(("pageToken", page_token.to_owned()));
    }
    query
}

fn calendar_list_query(page_token: Option<&str>) -> Vec<(&'static str, String)> {
    let mut query = vec![("maxResults", CALENDAR_LIST_PAGE_SIZE.to_string())];
    if let Some(page_token) = page_token {
        query.push(("pageToken", page_token.to_owned()));
    }
    query
}

fn request_error(status: StatusCode, body: &str) -> String {
    let summary = match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => format!(
            "Google rejected the access token (status {status}) — \
             sign in to Google again to reconnect the calendar"
        ),
        _ => format!("google calendar request failed with status {status}"),
    };
    match google_message(body) {
        Some(message) => format!("{summary}: {message}"),
        None => summary,
    }
}

fn google_message(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    text(value.get("error").and_then(|error| error.get("message"))).map(str::to_owned)
}

fn clip(text: &str) -> String {
    text.chars().take(200).collect()
}

fn convert_calendar(raw: &Value) -> Option<CalendarSource> {
    if raw.get("accessRole").and_then(Value::as_str) == Some("freeBusyReader") {
        return None;
    }
    let id = text(raw.get("id"))?;
    Some(CalendarSource {
        id: id.to_owned(),
        name: text(raw.get("summary")).unwrap_or(id).to_owned(),
        is_default: raw.get("primary").and_then(Value::as_bool).unwrap_or(false),
    })
}

fn import_entry(outcome: &mut SyncOutcome, raw: &Value, account_calendar_id: Option<&str>) {
    if raw.get("status").and_then(Value::as_str) == Some("cancelled") {
        return;
    }
    let Some(external_id) = raw.get("id").and_then(Value::as_str) else {
        tracing::warn!("skipping calendar entry with no id");
        return;
    };
    if account_calendar_id.is_some_and(|id| !event_is_accepted(raw, id)) {
        outcome.exclude(external_id);
        return;
    }
    let version = raw.get("etag").and_then(Value::as_str).map(str::to_owned);

    if is_all_day(raw) {
        match convert_all_day_event(raw, external_id) {
            Some(marker) => outcome.push_marker(external_id.to_owned(), version, marker),
            None => tracing::warn!(%external_id, "skipping undated all-day event"),
        }
    } else {
        match convert_event(raw, external_id) {
            Some(event) => outcome.push_event(external_id.to_owned(), version, event),
            None => tracing::warn!(%external_id, "skipping event with no usable start"),
        }
    }
}

fn event_is_accepted(raw: &Value, account_calendar_id: &str) -> bool {
    let is_account = |identity: &Value| {
        text(identity.get("email"))
            .is_some_and(|email| email.eq_ignore_ascii_case(account_calendar_id))
    };
    if raw.get("organizer").is_some_and(is_account) {
        return true;
    }

    let Some(attendees) = raw.get("attendees").and_then(Value::as_array) else {
        return true;
    };
    if attendees.is_empty() {
        return true;
    }

    attendees.iter().any(|attendee| {
        is_account(attendee)
            && attendee.get("responseStatus").and_then(Value::as_str) == Some("accepted")
    })
}

fn is_all_day(raw: &Value) -> bool {
    raw.get("start")
        .and_then(|start| start.get("date"))
        .is_some()
}

fn convert_event(raw: &Value, external_id: &str) -> Option<Event> {
    let start = google_datetime(raw.get("start"))?;
    let span = google_datetime(raw.get("end")).map(|end| end - start);
    let duration = match span {
        Some(span) if span.num_seconds() > 0 => RelativeDuration::from(span),
        _ => RelativeDuration::hours(1),
    };

    let mut event = Event::new(summary(raw), start, duration)
        .with_content(build_event_content(raw))
        .with_source_provider("google_calendar")
        .with_source_external_id(external_id.to_owned());
    event.source_busy = match raw.get("transparency").and_then(Value::as_str) {
        Some("transparent") => Some(false),
        Some("opaque") => Some(true),
        _ => None,
    };
    Some(event)
}

fn convert_all_day_event(raw: &Value, external_id: &str) -> Option<Marker> {
    let start_date = google_date(raw.get("start"))?;

    let end_date = google_date(raw.get("end"))
        .and_then(|end| end.pred_opt())
        .filter(|end| *end > start_date);

    let mut marker = Marker::new(summary(raw), start_date)
        .with_source_provider("google_calendar")
        .with_source_external_id(external_id.to_owned());
    marker.content = build_event_content(raw);
    marker.end_date = end_date;
    Some(marker)
}

fn summary(raw: &Value) -> String {
    text(raw.get("summary")).unwrap_or("(no title)").to_owned()
}

fn build_event_content(raw: &Value) -> Option<String> {
    let mut parts = String::new();
    if let Some(description) = text(raw.get("description")) {
        parts.push_str(description);
    }

    if let Some(location) = text(raw.get("location")) {
        push_line(&mut parts, format_args!("[location: {location}]"));
    }
    if let Some(organizer) = raw.get("organizer").and_then(display_identity) {
        push_line(&mut parts, format_args!("[organizer: {organizer}]"));
    }
    if let Some(join_url) = raw.get("hangoutLink").and_then(Value::as_str) {
        push_line(&mut parts, format_args!("[join: {join_url}]"));
    }
    if let Some(link) = raw.get("htmlLink").and_then(Value::as_str) {
        push_line(&mut parts, format_args!("[open: {link}]"));
    }

    (!parts.is_empty()).then_some(parts)
}

fn push_line(parts: &mut String, line: std::fmt::Arguments<'_>) {
    if !parts.is_empty() {
        parts.push('\n');
    }
    let _ = parts.write_fmt(line);
}

fn display_identity(organizer: &Value) -> Option<String> {
    let name = text(organizer.get("displayName"));
    let email = text(organizer.get("email"));
    match (name, email) {
        (Some(name), Some(email)) if name != email => Some(format!("{name} <{email}>")),
        (Some(value), _) | (None, Some(value)) => Some(value.to_owned()),
        (None, None) => None,
    }
}

fn text(field: Option<&Value>) -> Option<&str> {
    field
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn google_datetime(field: Option<&Value>) -> Option<DateTime<Utc>> {
    let raw = text(field.and_then(|field| field.get("dateTime")))?;
    match DateTime::parse_from_rfc3339(raw) {
        Ok(parsed) => Some(parsed.with_timezone(&Utc)),
        Err(error) => {
            tracing::warn!(%raw, %error, "google calendar returned an unreadable timestamp");
            None
        }
    }
}

fn google_date(field: Option<&Value>) -> Option<NaiveDate> {
    let raw = text(field.and_then(|field| field.get("date")))?;
    match NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        Ok(parsed) => Some(parsed),
        Err(error) => {
            tracing::warn!(%raw, %error, "google calendar returned an unreadable date");
            None
        }
    }
}
