use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};

use super::ParseError;

use crate::lexer::{SpannedToken, Token};

pub(super) fn try_time_token(tokens: &[SpannedToken], start: usize) -> Option<(NaiveTime, usize)> {
    if start >= tokens.len() {
        return None;
    }
    match tokens[start].token {
        Token::Time12 => {
            let t = parse_time12(&tokens[start].text)?;
            Some((t, 1))
        }
        Token::Time24 => {
            let t = NaiveTime::parse_from_str(&tokens[start].text, "%H:%M").ok()?;
            Some((t, 1))
        }
        _ => None,
    }
}

pub(super) fn try_time_suffix(tokens: &[SpannedToken], start: usize) -> Option<(NaiveTime, usize)> {
    if start >= tokens.len() {
        return None;
    }
    let lower = tokens[start].text.to_ascii_lowercase();

    if lower == "at" && start + 1 < tokens.len() {
        if let Some((t, extra)) = try_time_token(tokens, start + 1) {
            return Some((t, 1 + extra));
        }
        return None;
    }
    try_time_token(tokens, start)
}

pub(super) fn parse_named_time(text: &str) -> Option<NaiveTime> {
    match text.trim() {
        "noon" | "midday" => NaiveTime::from_hms_opt(12, 0, 0),
        "midnight" => NaiveTime::from_hms_opt(0, 0, 0),
        "morning" => NaiveTime::from_hms_opt(9, 0, 0),
        "afternoon" => NaiveTime::from_hms_opt(14, 0, 0),
        "evening" => NaiveTime::from_hms_opt(20, 0, 0),
        "night" => NaiveTime::from_hms_opt(21, 0, 0),
        _ => None,
    }
}

pub(super) fn parse_bare_named_time(text: &str) -> Option<NaiveTime> {
    match text.trim() {
        "noon" | "midday" => NaiveTime::from_hms_opt(12, 0, 0),
        "midnight" => NaiveTime::from_hms_opt(0, 0, 0),
        _ => None,
    }
}

fn parse_time12(text: &str) -> Option<NaiveTime> {
    let lower = text.trim().to_ascii_lowercase();

    if let Some(hm) = lower.strip_suffix("am") {
        let (h, m) = parse_hm(hm)?;
        let hour = if h == 12 { 0 } else { h };
        return NaiveTime::from_hms_opt(hour, m, 0);
    }
    if let Some(hm) = lower.strip_suffix("pm") {
        let (h, m) = parse_hm(hm)?;
        let hour = if h == 12 { 12 } else { h + 12 };
        if hour >= 24 {
            return None;
        }
        return NaiveTime::from_hms_opt(hour, m, 0);
    }

    if let Some(hm) = lower.strip_suffix('a') {
        let (h, m) = parse_hm(hm)?;
        let hour = if h == 12 { 0 } else { h };
        return NaiveTime::from_hms_opt(hour, m, 0);
    }
    if let Some(hm) = lower.strip_suffix('p') {
        let (h, m) = parse_hm(hm)?;
        let hour = if h == 12 { 12 } else { h + 12 };
        if hour >= 24 {
            return None;
        }
        return NaiveTime::from_hms_opt(hour, m, 0);
    }

    None
}

fn parse_hm(text: &str) -> Option<(u32, u32)> {
    if let Some((h, m)) = text.split_once(':') {
        Some((h.parse().ok()?, m.parse().ok()?))
    } else {
        Some((text.parse().ok()?, 0))
    }
}

pub(super) fn default_time() -> NaiveTime {
    NaiveTime::from_hms_opt(9, 0, 0).unwrap()
}

pub(super) fn date_at(date: NaiveDate, time: NaiveTime) -> Result<DateTime<Utc>, ParseError> {
    let ndt = NaiveDateTime::new(date, time);
    match Local.from_local_datetime(&ndt) {
        chrono::LocalResult::Single(dt) => Ok(dt.with_timezone(&Utc)),
        chrono::LocalResult::Ambiguous(earliest, _latest) => Ok(earliest.with_timezone(&Utc)),
        chrono::LocalResult::None => Err(ParseError::date(
            ndt.to_string(),
            "local time does not exist or is out of range",
        )),
    }
}

pub(super) fn local_datetime(datetime: DateTime<Utc>) -> Result<NaiveDateTime, ParseError> {
    let local = datetime.with_timezone(&Local);
    local
        .naive_utc()
        .checked_add_offset(*local.offset())
        .ok_or_else(|| {
            ParseError::date(datetime.to_string(), "local date and time is out of range")
        })
}

pub(super) fn quantize_ceil(
    datetime: DateTime<Utc>,
    granularity: Duration,
) -> Result<DateTime<Utc>, ParseError> {
    let seconds = granularity.num_seconds();
    if seconds <= 0 || Duration::try_seconds(seconds) != Some(granularity) {
        return Err(ParseError::date(
            datetime.to_string(),
            "granularity must be a positive whole number of seconds",
        ));
    }
    let timestamp = datetime.timestamp();
    let remainder = timestamp.rem_euclid(seconds);
    if remainder == 0 && datetime.timestamp_subsec_nanos() == 0 {
        return Ok(datetime);
    }
    timestamp
        .checked_add(seconds - remainder)
        .and_then(|target| DateTime::from_timestamp(target, 0))
        .ok_or_else(|| {
            ParseError::date(
                datetime.to_string(),
                "rounded date and time is out of range",
            )
        })
}
