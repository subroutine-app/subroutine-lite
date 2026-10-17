use chrono::{DateTime, Duration, NaiveDate, SecondsFormat, Utc};
use chrono_tz::Tz;
use parser::{EntityKind, HighlightKind, ParseError, WhenSpec};
use serde::{Deserialize, Serialize};
use subroutine_core::Recurrence;

pub(crate) const MAX_REQUEST_BYTES: usize = 131_072;
const MAX_TEXT_BYTES: usize = 16_384;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Request {
    version: u32,
    text: String,
    kind: String,
    now: String,
    timezone: String,
    default_date: Option<String>,
}

struct Context {
    kind: EntityKind,
    now: DateTime<Utc>,
    timezone: Tz,
    default_date: Option<NaiveDate>,
}

impl Request {
    fn context(&self) -> Result<Context, &'static str> {
        if self.version != 1 {
            return Err("Unsupported parser request version; expected 1");
        }
        if self.text.len() > MAX_TEXT_BYTES {
            return Err("Parser text exceeds the 16384-byte UTF-8 limit");
        }
        let kind = match self.kind.as_str() {
            "Action" => EntityKind::Action,
            "Event" => EntityKind::Event,
            "Marker" => EntityKind::Marker,
            "Signal" => EntityKind::Signal,
            "RoutineStep" => EntityKind::RoutineStep,
            "ActionTemplate" => EntityKind::ActionTemplate,
            "EventTemplate" => EntityKind::EventTemplate,
            _ => {
                return Err(
                    "Unsupported parser kind; expected Action, Event, Marker, Signal, RoutineStep, ActionTemplate or EventTemplate",
                );
            }
        };
        let now = DateTime::parse_from_rfc3339(&self.now)
            .ok()
            .filter(|now| now.offset().local_minus_utc() == 0)
            .ok_or("Invalid now; expected a UTC RFC3339 timestamp")?
            .to_utc();
        let timezone = self
            .timezone
            .parse()
            .map_err(|_| "Invalid timezone; expected an IANA timezone ID")?;
        let default_date = self
            .default_date
            .as_deref()
            .map(|date| {
                NaiveDate::parse_from_str(date, "%Y-%m-%d")
                    .ok()
                    .filter(|parsed| date.len() == 10 && parsed.to_string() == date)
                    .ok_or("Invalid defaultDate; expected YYYY-MM-DD")
            })
            .transpose()?;
        Ok(Context {
            kind,
            now,
            timezone,
            default_date,
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Response {
    version: u32,
    title: String,
    when_value: Option<String>,
    naive_time: Option<String>,
    duration_seconds: Option<i64>,
    recurrence: Option<Recurrence>,
    warnings: Vec<String>,
    highlights: Vec<Highlight>,
    error: Option<String>,
}

impl Response {
    fn empty() -> Self {
        Self {
            version: 1,
            title: String::new(),
            when_value: None,
            naive_time: None,
            duration_seconds: None,
            recurrence: None,
            warnings: Vec::new(),
            highlights: Vec::new(),
            error: None,
        }
    }

    pub(crate) fn error(error: impl Into<String>) -> Self {
        Self {
            error: Some(error.into()),
            ..Self::empty()
        }
    }
}

#[derive(Serialize)]
struct Highlight {
    start: i32,
    end: i32,
    kind: HighlightKind,
}

pub(crate) fn parse(json: &str) -> Result<Response, &'static str> {
    if json.len() > MAX_REQUEST_BYTES {
        return Ok(Response::error(
            "Parser request exceeds the 131072-byte limit",
        ));
    }
    let request: Request = match serde_json::from_str(json) {
        Ok(request) => request,
        Err(_) => {
            return Ok(Response::error(
                "Invalid parser request JSON or field types",
            ));
        }
    };
    let context = match request.context() {
        Ok(context) => context,
        Err(error) => return Ok(Response::error(error)),
    };
    if request.text.trim().is_empty() {
        return Ok(Response::empty());
    }
    let mut draft = match parser::parse_in_timezone(
        &request.text,
        context.kind,
        context.now.with_timezone(&context.timezone),
        Duration::minutes(5),
        context.default_date,
    ) {
        Ok(draft) => draft,
        Err(error) => return Ok(Response::error(parse_error(error))),
    };
    if draft.recurrence.is_none()
        && !draft
            .highlights
            .iter()
            .any(|(_, kind)| *kind == HighlightKind::When)
    {
        draft.when = None;
    }
    let recurrence =
        match parser::recurrence_to_rule_in_timezone(draft.recurrence.as_ref(), context.timezone) {
            Ok(recurrence) => recurrence.map(|recurrence| {
                recurrence
                    .with_end_date(draft.recurrence_end_date)
                    .with_remaining(draft.recurrence_remaining)
            }),
            Err(error) => return Ok(Response::error(error.to_string())),
        };
    let offsets = utf16_offsets(&request.text);
    let mut highlights = draft
        .highlights
        .into_iter()
        .map(|(range, kind)| {
            let start = offsets.get(range.start).copied().flatten();
            let end = offsets.get(range.end).copied().flatten();
            match (start, end) {
                (Some(start), Some(end)) if start <= end => Ok(Highlight { start, end, kind }),
                _ => Err("Parser returned an invalid highlight boundary"),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    highlights.sort_by_key(|highlight| (highlight.start, highlight.end));
    Ok(Response {
        version: 1,
        title: draft.title,
        when_value: draft.when.map(|when| match when {
            WhenSpec::NaiveDate(date) => date.to_string(),
            WhenSpec::DateTime(time) => time.to_rfc3339_opts(SecondsFormat::AutoSi, true),
        }),
        naive_time: draft.naive_time.map(|time| time.to_string()),
        duration_seconds: draft.duration.map(|duration| duration.num_seconds()),
        recurrence,
        warnings: draft.warnings,
        highlights,
        error: None,
    })
}

fn utf16_offsets(text: &str) -> Vec<Option<i32>> {
    let mut offsets = vec![None; text.len() + 1];
    let mut offset = 0;
    for (byte, character) in text.char_indices() {
        offsets[byte] = Some(offset);
        offset += character.len_utf16() as i32;
    }
    offsets[text.len()] = Some(offset);
    offsets
}

fn parse_error(error: ParseError) -> String {
    match error {
        ParseError::MissingTitle => "Missing title".into(),
        ParseError::InvalidDate { reason, .. } => format!("Invalid date or time: {reason}"),
        ParseError::InvalidDuration { reason, .. } => format!("Invalid duration: {reason}"),
        ParseError::InvalidRecurrence { reason, .. } => format!("Invalid recurrence: {reason}"),
    }
}

#[cfg(test)]
mod tests;
