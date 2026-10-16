mod clauses;
mod date;
mod duration;
mod recurrence;
mod time;
mod title;
mod when;

#[cfg(test)]
mod tests;

use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone, Utc};
use thiserror::Error;

use crate::{
    ast::{EntityKind, ParseDraft, WhenSpec},
    lexer::{Token, lex},
};
use clauses::longest_match;
use recurrence::{align_weekdays, has_recurrence_clause};
use title::assemble_title;

pub use date::parse_weekday_name;
pub use duration::parse_duration_expr;
pub use recurrence::try_recurrence_text;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("missing title")]
    MissingTitle,
    #[error("invalid duration in {input:?}: {reason}")]
    InvalidDuration { input: String, reason: String },
    #[error("invalid date or time in {input:?}: {reason}")]
    InvalidDate { input: String, reason: String },
    #[error("invalid recurrence in {input:?}: {reason}")]
    InvalidRecurrence { input: String, reason: String },
}

impl ParseError {
    fn duration(input: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidDuration {
            input: input.into(),
            reason: reason.into(),
        }
    }

    fn date(input: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidDate {
            input: input.into(),
            reason: reason.into(),
        }
    }

    fn recurrence(input: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidRecurrence {
            input: input.into(),
            reason: reason.into(),
        }
    }

    fn with_input(mut self, raw: &str) -> Self {
        match &mut self {
            Self::MissingTitle => {}
            Self::InvalidDuration { input, .. }
            | Self::InvalidDate { input, .. }
            | Self::InvalidRecurrence { input, .. } => *input = raw.to_string(),
        }
        self
    }
}

fn is_numeric(text: &str) -> bool {
    text.chars().any(|c| c.is_ascii_digit())
        && text.chars().all(|c| {
            c.is_ascii_digit() || c.is_ascii_whitespace() || matches!(c, '+' | '-' | '.' | ',')
        })
}

fn numeric_end(tokens: &[crate::lexer::SpannedToken], start: usize) -> usize {
    start
        + tokens[start..]
            .iter()
            .take_while(|token| {
                token.token == Token::Number || matches!(token.text.as_str(), "+" | "-" | "." | ",")
            })
            .count()
}

pub fn parse_action(input: &str) -> Result<ParseDraft, ParseError> {
    parse_impl(input, EntityKind::Action, Local::now())
}

pub fn parse_event(input: &str) -> Result<ParseDraft, ParseError> {
    parse_impl(input, EntityKind::Event, Local::now())
}

pub fn parse_action_template(input: &str) -> Result<ParseDraft, ParseError> {
    parse_impl(input, EntityKind::ActionTemplate, Local::now())
}

pub fn parse_event_template(input: &str) -> Result<ParseDraft, ParseError> {
    parse_impl(input, EntityKind::EventTemplate, Local::now())
}

pub fn parse_signal(input: &str) -> Result<ParseDraft, ParseError> {
    parse_impl(input, EntityKind::Signal, Local::now())
}

pub fn parse_routine_step(input: &str) -> Result<ParseDraft, ParseError> {
    parse_impl(input, EntityKind::RoutineStep, Local::now())
}

pub fn parse_marker(input: &str) -> Result<ParseDraft, ParseError> {
    parse_impl(input, EntityKind::Marker, Local::now())
}

/// Defaults undated markers and bare times to `date`; relative phrases still use the current time.
pub fn parse_marker_on_date(input: &str, date: NaiveDate) -> Result<ParseDraft, ParseError> {
    parse_on_date(input, EntityKind::Marker, date)
}

/// Defaults undated input and time-only clauses to `date`, without supplying a clock time.
/// Explicit dates override it; relative phrases still use the current local date/time.
/// Templates ignore `date`, and weekday recurrences align from it when no date is explicit.
pub fn parse_on_date(
    input: &str,
    kind: EntityKind,
    date: NaiveDate,
) -> Result<ParseDraft, ParseError> {
    parse_in_timezone(input, kind, Local::now(), Duration::minutes(5), Some(date))
}

fn parse_impl(
    input: &str,
    kind: EntityKind,
    now: DateTime<Local>,
) -> Result<ParseDraft, ParseError> {
    parse_with_context(input, kind, now, Duration::minutes(5))
}

pub fn parse_with_context(
    input: &str,
    kind: EntityKind,
    now: DateTime<Local>,
    granularity: Duration,
) -> Result<ParseDraft, ParseError> {
    parse_in_timezone(input, kind, now, granularity, None)
}

/// Uses the timezone carried by `now` for all calendar and wall-clock operations.
/// Explicit dates and relative phrases override `default_date`; bare times use it.
/// Nonexistent wall times fail, and ambiguous wall times use the earlier instant.
pub fn parse_in_timezone<Tz: TimeZone>(
    input: &str,
    kind: EntityKind,
    now: DateTime<Tz>,
    granularity: Duration,
    default_date: Option<NaiveDate>,
) -> Result<ParseDraft, ParseError> {
    parse_tokens(input, kind, now, granularity, default_date)
        .map_err(|error| error.with_input(input))
}

struct TemporalContext<Tz> {
    now: DateTime<Utc>,
    timezone: Tz,
    today: NaiveDate,
    granularity: Duration,
    default_date: Option<NaiveDate>,
}

fn parse_tokens<Tz: TimeZone>(
    input: &str,
    kind: EntityKind,
    now: DateTime<Tz>,
    granularity: Duration,
    default_date: Option<NaiveDate>,
) -> Result<ParseDraft, ParseError> {
    let default_date = default_date.filter(|_| !kind.is_template());
    let context = TemporalContext {
        now: now.to_utc(),
        timezone: now.timezone(),
        today: time::local_datetime(now.to_utc(), &now.timezone())?.date(),
        granularity,
        default_date,
    };
    let tokens = lex(input);
    let has_recurrence = has_recurrence_clause(&tokens)?;
    let mut draft = ParseDraft::new(kind.clone(), input);
    let mut consumed = vec![false; tokens.len()];

    let mut i = 0;
    while i < tokens.len() {
        if consumed[i] {
            i += 1;
            continue;
        }

        if kind.is_template()
            && tokens[i].text.eq_ignore_ascii_case("in")
            && tokens.get(i + 1).is_some_and(|t| t.token == Token::Number)
            && let Some((_, len)) = duration::try_nl_duration(&tokens, i + 1)?
        {
            i += 1 + len;
            continue;
        }

        if let Some(matched) = longest_match(&tokens, i, &context, &draft, has_recurrence)? {
            let len = matched.len;
            let kind = matched.apply(&mut draft);
            let start = tokens[i].span.start;
            let end = tokens[i + len - 1].span.end;
            draft.highlights.push((start..end, kind));
            consumed[i..i + len].fill(true);
            i += len;
        } else {
            i += 1;
        }
    }

    assemble_title(&mut draft, &tokens, &consumed);
    draft.when = draft.when.or(default_date.map(WhenSpec::NaiveDate));
    draft
        .recurrence
        .as_ref()
        .map(crate::build::recurrence_rule)
        .transpose()
        .map_err(|error| ParseError::recurrence(input, error.to_string()))?;
    align_weekdays(&mut draft, context.today, &context.timezone)?;
    if let Some(duration) = draft.duration {
        let fits = match draft.when {
            Some(crate::ast::WhenSpec::DateTime(start)) => {
                start.checked_add_signed(duration).is_some()
            }
            Some(crate::ast::WhenSpec::NaiveDate(start)) => {
                start.checked_add_signed(duration).is_some()
            }
            None => now.checked_add_signed(duration).is_some(),
        };
        if !fits {
            return Err(ParseError::duration(input, "end date is out of range"));
        }
    }

    if draft.title.trim().is_empty() {
        return Err(ParseError::MissingTitle);
    }

    Ok(draft)
}
