use chrono::{DateTime, Duration, NaiveTime, TimeZone, Utc};

use super::{ParseError, TemporalContext, date::*, time::*};
use crate::ast::{EntityKind, WhenSpec};
use crate::lexer::{SpannedToken, Token};

pub(super) fn try_nl_time(tokens: &[SpannedToken], i: usize) -> Option<(NaiveTime, usize)> {
    let lower = tokens.get(i)?.text.to_ascii_lowercase();
    if lower == "at" {
        let next = tokens.get(i + 1)?.text.to_ascii_lowercase();
        return parse_named_time(&next)
            .map(|time| (time, 2))
            .or_else(|| try_time_suffix(tokens, i));
    }
    parse_bare_named_time(&lower)
        .map(|time| (time, 1))
        .or_else(|| try_time_token(tokens, i))
}

pub(super) fn try_nl_when<Tz: TimeZone>(
    tokens: &[SpannedToken],
    i: usize,
    kind: EntityKind,
    context: &TemporalContext<Tz>,
) -> Result<Option<(WhenSpec, usize)>, ParseError> {
    let TemporalContext {
        now,
        today,
        granularity,
        default_date,
        timezone,
    } = context;
    let (now, today, granularity, default_date) = (*now, *today, *granularity, *default_date);
    if kind.is_template() {
        return Ok(None);
    }
    let Some(token) = tokens.get(i) else {
        return Ok(None);
    };
    let lower = token.text.to_ascii_lowercase();

    if let Some((time, time_len)) = try_nl_time(tokens, i)
        && let Some((date, date_len)) = try_date_anchor(tokens, i + time_len, today)?
    {
        return Ok(Some((
            WhenSpec::DateTime(date_at(date, time, timezone)?),
            time_len + date_len,
        )));
    }

    if let Some((amount, unit, len)) = try_relative_amount(tokens, i)?
        && matches!(
            unit.as_str(),
            "hour" | "hours" | "hr" | "hrs" | "minute" | "minutes" | "min" | "mins"
        )
    {
        let amount = i64::try_from(amount)
            .map_err(|_| ParseError::date(&token.text, "relative offset is out of range"))?;
        let duration = if matches!(unit.as_str(), "hour" | "hours" | "hr" | "hrs") {
            Duration::try_hours(amount)
        } else {
            Duration::try_minutes(amount)
        };
        let datetime = duration
            .and_then(|duration| now.checked_add_signed(duration))
            .ok_or_else(|| {
                ParseError::date(&token.text, "resulting date and time is out of range")
            })?;
        return Ok(Some((WhenSpec::DateTime(datetime), len)));
    }

    if lower == "this"
        && let Some(next) = tokens.get(i + 1)
        && matches!(
            next.text.to_ascii_lowercase().as_str(),
            "morning" | "afternoon" | "evening" | "night"
        )
        && let Some(time) = parse_named_time(&next.text.to_ascii_lowercase())
    {
        return Ok(Some((
            WhenSpec::DateTime(date_at(today, time, timezone)?),
            2,
        )));
    }

    if lower == "now" {
        return Ok(Some((
            WhenSpec::DateTime(quantize_ceil(now, granularity)?),
            1,
        )));
    }

    let named = match lower.as_str() {
        "tonight" => Some((today, NaiveTime::from_hms_opt(20, 0, 0).unwrap())),
        "later" => Some((today, NaiveTime::from_hms_opt(14, 0, 0).unwrap())),
        "soon" => Some((add_days(today, 1)?, default_time())),
        _ => None,
    };
    if let Some((date, time)) = named {
        return Ok(Some((
            WhenSpec::DateTime(date_at(date, time, timezone)?),
            1,
        )));
    }

    if let Some((time, len)) = try_nl_time(tokens, i) {
        return Ok(Some((
            WhenSpec::DateTime(date_at(default_date.unwrap_or(today), time, timezone)?),
            len,
        )));
    }

    if token.token == Token::Rfc3339 {
        let datetime = DateTime::parse_from_rfc3339(&token.text)
            .map_err(|_| ParseError::date(&token.text, "invalid RFC 3339 date and time"))?;
        return Ok(Some((WhenSpec::DateTime(datetime.with_timezone(&Utc)), 1)));
    }

    let intro = usize::from(
        lower == "on"
            && tokens
                .get(i + 1)
                .is_some_and(|token| parse_weekday_name(&token.text).is_some()),
    );
    if let Some((date, len)) = try_date_anchor(tokens, i + intro, today)? {
        let len = len + intro;
        if let Some((time, extra)) = try_nl_time(tokens, i + len) {
            return Ok(Some((
                WhenSpec::DateTime(date_at(date, time, timezone)?),
                len + extra,
            )));
        }
        return Ok(Some((WhenSpec::NaiveDate(date), len)));
    }

    Ok(None)
}
