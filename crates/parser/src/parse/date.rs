use chrono::{DateTime, Datelike, NaiveDate, Weekday};

use super::{ParseError, duration::join_adjacent, numeric_end};
use crate::lexer::{SpannedToken, Token};

pub(super) fn try_date_anchor(
    tokens: &[SpannedToken],
    start: usize,
    today: NaiveDate,
) -> Result<Option<(NaiveDate, usize)>, ParseError> {
    let Some(token) = tokens.get(start) else {
        return Ok(None);
    };
    let lower = token.text.to_ascii_lowercase();

    match lower.as_str() {
        "today" | "tonight" => return Ok(Some((today, 1))),
        "tomorrow" | "tom" => return Ok(Some((add_days(today, 1)?, 1))),
        _ => {}
    }

    if let Some(day) = parse_weekday_name(&lower) {
        return Ok(Some((next_weekday_strict(today, day)?, 1)));
    }

    if matches!(lower.as_str(), "next" | "this") && start + 1 < tokens.len() {
        let next = tokens[start + 1].text.to_ascii_lowercase();
        if let Some(day) = parse_weekday_name(&next) {
            return Ok(Some((next_weekday_strict(today, day)?, 2)));
        }
        if next == "week" {
            return Ok(Some((add_days(today, 7)?, 2)));
        }
    }

    if let Some((amount, unit, len)) = try_relative_amount(tokens, start)? {
        let days = match unit.as_str() {
            "day" | "days" => Some(amount),
            "week" | "weeks" => Some(
                amount
                    .checked_mul(7)
                    .ok_or_else(|| ParseError::date(&token.text, "week offset is out of range"))?,
            ),
            _ => None,
        };
        if let Some(days) = days {
            return Ok(Some((add_days(today, days)?, len)));
        }
    }

    if let Some(month) = parse_month_name(&lower)
        && let Some(next) = tokens.get(start + 1)
        && let Some(day) = parse_day_token(next)?
    {
        return Ok(Some((next_month_day(today, month, day)?, 2)));
    }

    if lower == "the"
        && let Some(next) = tokens.get(start + 1)
        && matches!(next.token, Token::OrdinalDay)
        && let Some(day) = parse_day_token(next)?
    {
        let date = next_month_with_day(today, day)
            .ok_or_else(|| ParseError::date(&next.text, "month day is invalid or out of range"))?;
        return Ok(Some((date, 2)));
    }

    if matches!(token.token, Token::Rfc3339) {
        let dt = DateTime::parse_from_rfc3339(&token.text)
            .map_err(|_| ParseError::date(&token.text, "invalid RFC 3339 date and time"))?;
        return Ok(Some((dt.date_naive(), 1)));
    }

    if matches!(token.token, Token::IsoDate) {
        let date = NaiveDate::parse_from_str(&token.text, "%Y-%m-%d")
            .map_err(|_| ParseError::date(&token.text, "invalid calendar date"))?;
        return Ok(Some((date, 1)));
    }

    Ok(None)
}

pub(super) fn try_relative_amount(
    tokens: &[SpannedToken],
    start: usize,
) -> Result<Option<(u64, String, usize)>, ParseError> {
    if !tokens
        .get(start)
        .is_some_and(|token| token.text.eq_ignore_ascii_case("in"))
    {
        return Ok(None);
    }
    let end = numeric_end(tokens, start + 1);
    let Some(unit) = tokens.get(end).map(|token| token.text.to_ascii_lowercase()) else {
        return Ok(None);
    };
    if end == start + 1
        || !matches!(
            unit.as_str(),
            "day"
                | "days"
                | "week"
                | "weeks"
                | "hour"
                | "hours"
                | "hr"
                | "hrs"
                | "minute"
                | "minutes"
                | "min"
                | "mins"
        )
    {
        return Ok(None);
    }
    let text = join_adjacent(tokens, start + 1, end - start - 1);
    let amount = text.parse::<u64>().map_err(|_| {
        ParseError::date(
            text,
            "relative offset must be a nonnegative whole number within the supported range",
        )
    })?;
    Ok(Some((amount, unit, end - start + 1)))
}

pub(super) fn add_days(from: NaiveDate, days: u64) -> Result<NaiveDate, ParseError> {
    from.checked_add_days(chrono::Days::new(days))
        .ok_or_else(|| ParseError::date(from.to_string(), "resulting date is out of range"))
}

fn parse_day_token(token: &SpannedToken) -> Result<Option<u32>, ParseError> {
    let day = match token.token {
        Token::OrdinalDay => parse_ordinal_number(&token.text),
        Token::Number => token.text.parse().ok(),
        _ => return Ok(None),
    };
    day.filter(|day| (1..=31).contains(day))
        .map(Some)
        .ok_or_else(|| ParseError::date(&token.text, "month day must be between 1 and 31"))
}

pub fn parse_weekday_name(s: &str) -> Option<Weekday> {
    match s.trim_end_matches('s').to_ascii_lowercase().as_str() {
        "mon" | "monday" => Some(Weekday::Mon),
        "tue" | "tuesday" => Some(Weekday::Tue),
        "wed" | "wednesday" => Some(Weekday::Wed),
        "thu" | "thursday" => Some(Weekday::Thu),
        "fri" | "friday" => Some(Weekday::Fri),
        "sat" | "saturday" => Some(Weekday::Sat),
        "sun" | "sunday" => Some(Weekday::Sun),
        _ => None,
    }
}

pub(super) fn next_weekday_strict(from: NaiveDate, day: Weekday) -> Result<NaiveDate, ParseError> {
    let days = (day.num_days_from_monday() + 6 - from.weekday().num_days_from_monday()) % 7 + 1;
    add_days(from, u64::from(days))
}

pub(super) fn parse_month_name(s: &str) -> Option<u32> {
    match s.to_ascii_lowercase().as_str() {
        "january" | "jan" => Some(1),
        "february" | "feb" => Some(2),
        "march" | "mar" => Some(3),
        "april" | "apr" => Some(4),
        "may" => Some(5),
        "june" | "jun" => Some(6),
        "july" | "jul" => Some(7),
        "august" | "aug" => Some(8),
        "september" | "sep" | "sept" => Some(9),
        "october" | "oct" => Some(10),
        "november" | "nov" => Some(11),
        "december" | "dec" => Some(12),
        _ => None,
    }
}

pub(super) fn parse_ordinal_number(text: &str) -> Option<u32> {
    let lower = text.to_ascii_lowercase();
    let digits = lower
        .trim_end_matches("th")
        .trim_end_matches("st")
        .trim_end_matches("nd")
        .trim_end_matches("rd");
    digits.parse().ok()
}

fn next_month_day(from: NaiveDate, month: u32, day: u32) -> Result<NaiveDate, ParseError> {
    let this_year = from.year();
    if let Some(date) = NaiveDate::from_ymd_opt(this_year, month, day)
        && date >= from
    {
        return Ok(date);
    }
    this_year
        .checked_add(1)
        .and_then(|year| NaiveDate::from_ymd_opt(year, month, day))
        .ok_or_else(|| ParseError::date(from.to_string(), "month day is invalid or out of range"))
}

fn next_month_with_day(from: NaiveDate, day: u32) -> Option<NaiveDate> {
    if let Some(d) = NaiveDate::from_ymd_opt(from.year(), from.month(), day)
        && d >= from
    {
        return Some(d);
    }
    let (next_year, next_month) = if from.month() == 12 {
        (from.year().checked_add(1)?, 1)
    } else {
        (from.year(), from.month() + 1)
    };
    NaiveDate::from_ymd_opt(next_year, next_month, day)
}
