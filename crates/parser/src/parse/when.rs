use chrono::{DateTime, Duration, NaiveDate, NaiveTime, Utc};

use super::date::*;
use super::time::*;
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

pub(super) fn try_nl_when(
    tokens: &[SpannedToken],
    i: usize,
    kind: EntityKind,
    now: DateTime<Utc>,
    today: chrono::NaiveDate,
    granularity: Duration,
) -> Option<(WhenSpec, usize)> {
    let date_spec: fn(NaiveDate) -> WhenSpec = match kind {
        EntityKind::Action | EntityKind::RoutineStep | EntityKind::Marker => WhenSpec::NaiveDate,
        EntityKind::Event | EntityKind::Signal => {
            |date| WhenSpec::DateTime(date_at(date, default_time()))
        }
        EntityKind::ActionTemplate | EntityKind::EventTemplate => return None,
    };

    let n = tokens.len();
    if i >= n {
        return None;
    }

    let lower = tokens[i].text.to_ascii_lowercase();

    if matches!(tokens[i].token, Token::Time12 | Token::Time24)
        && let Some((time, _time_len)) = try_time_token(tokens, i)
        && let Some(date) = try_date_anchor(tokens, i + 1, today)
    {
        return Some((WhenSpec::DateTime(date_at(date.0, time)), 1 + date.1));
    }

    if lower == "in" && i + 2 < n && matches!(tokens[i + 1].token, Token::Number) {
        let unit = tokens[i + 2].text.to_ascii_lowercase();
        let amount: i64 = tokens[i + 1].text.parse().ok()?;
        let spec = match unit.as_str() {
            "hour" | "hours" | "hr" | "hrs" => {
                Some(WhenSpec::DateTime(now + Duration::hours(amount)))
            }
            "minute" | "minutes" | "min" | "mins" => {
                Some(WhenSpec::DateTime(now + Duration::minutes(amount)))
            }
            "day" | "days" => {
                let date = today + chrono::Days::new(amount as u64);
                Some(date_spec(date))
            }
            "week" | "weeks" => {
                let date = today + chrono::Days::new(amount as u64 * 7);
                Some(date_spec(date))
            }
            _ => None,
        };
        if let Some(spec) = spec {
            return Some((spec, 3));
        }
    }

    if lower == "this" && i + 1 < n {
        let next = tokens[i + 1].text.to_ascii_lowercase();
        let time_opt = match next.as_str() {
            "morning" => Some(NaiveTime::from_hms_opt(9, 0, 0).unwrap()),
            "afternoon" => Some(NaiveTime::from_hms_opt(14, 0, 0).unwrap()),
            "evening" => Some(NaiveTime::from_hms_opt(20, 0, 0).unwrap()),
            "night" => Some(NaiveTime::from_hms_opt(21, 0, 0).unwrap()),
            _ => None,
        };
        if let Some(t) = time_opt {
            return Some((WhenSpec::DateTime(date_at(today, t)), 2));
        }
        if let Some(day) = parse_weekday_name(&next) {
            let date = this_or_next_weekday(today, day, false);
            return Some((date_spec(date), 2));
        }
        if next == "week" {
            let date = today + chrono::Days::new(7);
            return Some((date_spec(date), 2));
        }
    }

    if lower == "next" && i + 1 < n {
        let next = tokens[i + 1].text.to_ascii_lowercase();
        if let Some(day) = parse_weekday_name(&next) {
            let date = next_weekday_strict(today, day);
            if let Some((time, extra)) = try_time_suffix(tokens, i + 2) {
                return Some((WhenSpec::DateTime(date_at(date, time)), 2 + extra));
            }
            return Some((date_spec(date), 2));
        }
        if next == "week" {
            let date = today + chrono::Days::new(7);
            return Some((date_spec(date), 2));
        }
    }

    if let Some(day) = parse_weekday_name(&lower) {
        let date = next_weekday_strict(today, day);
        if let Some((time, extra)) = try_time_suffix(tokens, i + 1) {
            return Some((WhenSpec::DateTime(date_at(date, time)), 1 + extra));
        }
        return Some((date_spec(date), 1));
    }

    if lower == "now" {
        return Some((
            WhenSpec::DateTime(subroutine_core::quantize_ceil(now, granularity)),
            1,
        ));
    }

    if lower == "tonight" {
        let t = NaiveTime::from_hms_opt(20, 0, 0).unwrap();
        return Some((WhenSpec::DateTime(date_at(today, t)), 1));
    }

    if lower == "later" {
        let t = NaiveTime::from_hms_opt(14, 0, 0).unwrap();
        return Some((WhenSpec::DateTime(date_at(today, t)), 1));
    }
    if lower == "soon" {
        let t = NaiveTime::from_hms_opt(9, 0, 0).unwrap();
        let date = today + chrono::Days::new(1);
        return Some((WhenSpec::DateTime(date_at(date, t)), 1));
    }

    if lower == "today" {
        if let Some((time, extra)) = try_time_suffix(tokens, i + 1) {
            return Some((WhenSpec::DateTime(date_at(today, time)), 1 + extra));
        }
        return Some((date_spec(today), 1));
    }

    if lower == "tomorrow" || lower == "tom" {
        let date = today + chrono::Days::new(1);
        if let Some((time, extra)) = try_time_suffix(tokens, i + 1) {
            return Some((WhenSpec::DateTime(date_at(date, time)), 1 + extra));
        }
        return Some((date_spec(date), 1));
    }

    if let Some((time, len)) = try_nl_time(tokens, i) {
        return Some((WhenSpec::DateTime(date_at(today, time)), len));
    }

    if lower == "on" && i + 1 < n {
        let next = tokens[i + 1].text.to_ascii_lowercase();
        if let Some(day) = parse_weekday_name(&next) {
            let date = next_weekday_strict(today, day);
            if let Some((time, extra)) = try_time_suffix(tokens, i + 2) {
                return Some((WhenSpec::DateTime(date_at(date, time)), 2 + extra));
            }
            return Some((date_spec(date), 2));
        }
    }

    if let Some(month) = parse_month_name(&lower)
        && i + 1 < n
    {
        let next_lower = tokens[i + 1].text.to_ascii_lowercase();
        let day_opt = if matches!(tokens[i + 1].token, Token::OrdinalDay) {
            parse_ordinal_number(&tokens[i + 1].text)
        } else if matches!(tokens[i + 1].token, Token::Number) {
            tokens[i + 1].text.parse::<u32>().ok()
        } else if matches!(tokens[i + 1].token, Token::Word) {
            parse_ordinal_number(&next_lower).or_else(|| next_lower.parse::<u32>().ok())
        } else {
            None
        };
        if let Some(day) = day_opt {
            let year = next_month_day_year(today, month, day);
            if let Some(date) = NaiveDate::from_ymd_opt(year, month, day) {
                if let Some((time, extra)) = try_time_suffix(tokens, i + 2) {
                    return Some((WhenSpec::DateTime(date_at(date, time)), 2 + extra));
                }
                return Some((date_spec(date), 2));
            }
        }
    }

    if lower == "the" && i + 1 < n {
        let next = &tokens[i + 1];
        let day_opt = if matches!(next.token, Token::OrdinalDay) {
            parse_ordinal_number(&next.text)
        } else {
            None
        };
        if let Some(day) = day_opt
            && let Some(date) = next_month_with_day(today, day)
        {
            if let Some((time, extra)) = try_time_suffix(tokens, i + 2) {
                return Some((WhenSpec::DateTime(date_at(date, time)), 2 + extra));
            }
            return Some((date_spec(date), 2));
        }
    }

    if matches!(tokens[i].token, Token::Rfc3339)
        && let Ok(dt) = DateTime::parse_from_rfc3339(&tokens[i].text)
    {
        return Some((WhenSpec::DateTime(dt.with_timezone(&Utc)), 1));
    }

    if matches!(tokens[i].token, Token::IsoDate)
        && let Ok(date) = NaiveDate::parse_from_str(&tokens[i].text, "%Y-%m-%d")
    {
        if let Some((time, extra)) = try_time_token(tokens, i + 1) {
            return Some((WhenSpec::DateTime(date_at(date, time)), 1 + extra));
        }
        return Some((date_spec(date), 1));
    }

    None
}
