use chrono::{DateTime, Datelike, Duration, NaiveDate, Weekday};

use crate::lexer::{SpannedToken, Token};

pub(super) fn try_date_anchor(
    tokens: &[SpannedToken],
    start: usize,
    today: chrono::NaiveDate,
) -> Option<(NaiveDate, usize)> {
    if start >= tokens.len() {
        return None;
    }
    let lower = tokens[start].text.to_ascii_lowercase();

    match lower.as_str() {
        "today" | "tonight" => return Some((today, 1)),
        "tomorrow" | "tom" => return Some((today + Duration::days(1), 1)),
        _ => {}
    }

    if let Some(day) = parse_weekday_name(&lower) {
        return Some((next_weekday_strict(today, day), 1));
    }

    if matches!(lower.as_str(), "next" | "this") && start + 1 < tokens.len() {
        let next = tokens[start + 1].text.to_ascii_lowercase();
        if let Some(day) = parse_weekday_name(&next) {
            return Some((next_weekday_strict(today, day), 2));
        }
        if next == "week" {
            return Some((today + Duration::days(7), 2));
        }
    }

    if lower == "in" && start + 2 < tokens.len() && matches!(tokens[start + 1].token, Token::Number)
    {
        let amount = tokens[start + 1].text.parse::<u64>().ok()?;
        let unit = tokens[start + 2].text.to_ascii_lowercase();
        let days = match unit.as_str() {
            "day" | "days" => amount,
            "week" | "weeks" => amount.checked_mul(7)?,
            _ => 0,
        };
        if days > 0 {
            return Some((today.checked_add_days(chrono::Days::new(days))?, 3));
        }
    }

    if let Some(month) = parse_month_name(&lower)
        && start + 1 < tokens.len()
        && let Some(day) = parse_day_token(&tokens[start + 1])
    {
        let year = next_month_day_year(today, month, day);
        if let Some(date) = NaiveDate::from_ymd_opt(year, month, day) {
            return Some((date, 2));
        }
    }

    if lower == "the"
        && start + 1 < tokens.len()
        && matches!(tokens[start + 1].token, Token::OrdinalDay)
        && let Some(day) = parse_ordinal_number(&tokens[start + 1].text)
        && let Some(date) = next_month_with_day(today, day)
    {
        return Some((date, 2));
    }

    if matches!(tokens[start].token, Token::Rfc3339)
        && let Ok(dt) = DateTime::parse_from_rfc3339(&tokens[start].text)
    {
        return Some((dt.date_naive(), 1));
    }

    if matches!(tokens[start].token, Token::IsoDate)
        && let Ok(date) = NaiveDate::parse_from_str(&tokens[start].text, "%Y-%m-%d")
    {
        return Some((date, 1));
    }

    None
}

fn parse_day_token(token: &SpannedToken) -> Option<u32> {
    match token.token {
        Token::OrdinalDay => parse_ordinal_number(&token.text),
        Token::Number => token.text.parse().ok(),
        Token::Word => {
            let lower = token.text.to_ascii_lowercase();
            parse_ordinal_number(&lower).or_else(|| lower.parse().ok())
        }
        _ => None,
    }
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

pub(super) fn next_weekday_strict(from: NaiveDate, day: Weekday) -> NaiveDate {
    let mut d = from + Duration::days(1);
    while d.weekday() != day {
        d += Duration::days(1);
    }
    d
}

pub(super) fn this_or_next_weekday(from: NaiveDate, day: Weekday, same_ok: bool) -> NaiveDate {
    if same_ok && from.weekday() == day {
        return from;
    }
    next_weekday_strict(from, day)
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

pub(super) fn next_month_day_year(from: NaiveDate, month: u32, day: u32) -> i32 {
    let this_year = from.year();
    if let Some(d) = NaiveDate::from_ymd_opt(this_year, month, day)
        && d >= from
    {
        return this_year;
    }
    this_year + 1
}

pub(super) fn next_month_with_day(from: NaiveDate, day: u32) -> Option<NaiveDate> {
    if let Some(d) = NaiveDate::from_ymd_opt(from.year(), from.month(), day)
        && d >= from
    {
        return Some(d);
    }
    let (next_year, next_month) = if from.month() == 12 {
        (from.year() + 1, 1)
    } else {
        (from.year(), from.month() + 1)
    };
    NaiveDate::from_ymd_opt(next_year, next_month, day)
}
