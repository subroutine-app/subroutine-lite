pub(super) mod bounds;

use chrono::{Datelike, NaiveDate, Weekday};

use super::{
    ParseError,
    date::{add_days, parse_ordinal_number, parse_weekday_name},
    duration::join_adjacent,
    is_numeric, numeric_end,
    time::{date_at, default_time},
    title::is_escaped,
};
use crate::{
    ast::{EntityKind, ParseDraft, RecurrenceSpec, WeekdaySet, WhenSpec},
    lexer::{SpannedToken, Token},
};

pub(super) fn has_recurrence_clause(tokens: &[SpannedToken]) -> Result<bool, ParseError> {
    let mut found = false;
    for index in 0..tokens.len() {
        if !is_escaped(tokens, index) && try_nl_recurrence(tokens, index)?.is_some() {
            found = true;
        }
    }
    Ok(found)
}

pub(super) fn try_nl_recurrence(
    tokens: &[SpannedToken],
    i: usize,
) -> Result<Option<(RecurrenceSpec, usize)>, ParseError> {
    let Some(token) = tokens.get(i) else {
        return Ok(None);
    };
    let lower = token.text.to_ascii_lowercase();

    match lower.as_str() {
        "daily" => return Ok(Some((RecurrenceSpec::EveryDays(1), 1))),
        "weekly" => return Ok(Some((RecurrenceSpec::EveryWeeks(1), 1))),
        "monthly" => return Ok(Some((RecurrenceSpec::EveryMonths(1), 1))),
        "yearly" | "annually" => return Ok(Some((RecurrenceSpec::EveryYears(1), 1))),
        "weekdays" => return Ok(Some((RecurrenceSpec::weekdays(), 1))),
        "weekends" => return Ok(Some((RecurrenceSpec::weekends(), 1))),
        _ => {}
    }

    if lower == "every" && i + 1 < tokens.len() && !is_escaped(tokens, i + 1) {
        return try_every_clause(tokens, i);
    }

    if matches!(token.token, Token::OrdinalDay)
        && i + 3 < tokens.len()
        && tokens[i + 1].text.eq_ignore_ascii_case("of")
        && (tokens[i + 2].text.eq_ignore_ascii_case("every")
            || tokens[i + 2].text.eq_ignore_ascii_case("the"))
        && tokens[i + 3].text.eq_ignore_ascii_case("month")
    {
        let text = join_adjacent(tokens, i, 4);
        return Ok(try_recurrence_text(&text)?.map(|spec| (spec, 4)));
    }

    if lower.contains(',') {
        return Ok(try_recurrence_text(&lower)?.map(|spec| (spec, 1)));
    }
    Ok(None)
}

fn try_every_clause(
    tokens: &[SpannedToken],
    i: usize,
) -> Result<Option<(RecurrenceSpec, usize)>, ParseError> {
    const STOP_WORDS: &[&str] = &["at", "for", "in", "with", "lasting", "takes", "by", "until"];

    let number_end = numeric_end(tokens, i + 1);
    if number_end > i + 1 {
        let end = (number_end + 1).min(tokens.len());
        let amount = join_adjacent(tokens, i + 1, number_end - i - 1);
        let text = match tokens.get(number_end) {
            Some(unit) => format!("every {amount} {}", unit.text),
            None => format!("every {amount}"),
        };
        return try_recurrence_text(&text)?
            .map(|spec| Some((spec, end - i)))
            .ok_or_else(|| {
                ParseError::recurrence(
                    text,
                    "use a positive whole-number interval in days, weeks, months or years",
                )
            });
    }

    let mut end = i + 1;
    while end < tokens.len() {
        match tokens[end].token {
            Token::Word | Token::Number | Token::OrdinalDay => {
                let lower = tokens[end].text.to_ascii_lowercase();
                if STOP_WORDS.contains(&lower.as_str()) {
                    break;
                }
                end += 1;
            }
            Token::Punct if tokens[end].text == "," => end += 1,
            _ => break,
        }
        if end > i + 8 {
            break;
        }
    }

    for span_end in (i + 2..=end).rev() {
        let text = join_adjacent(tokens, i, span_end - i);
        if let Some(rec) = try_recurrence_text(&text)? {
            return Ok(Some((rec, span_end - i)));
        }
    }
    Ok(None)
}

pub fn try_recurrence_text(text: &str) -> Result<Option<RecurrenceSpec>, ParseError> {
    let lower = text.trim().to_ascii_lowercase();
    let spec = recurrence_spec(&lower).map_err(|error| error.with_input(text))?;
    crate::build::recurrence_to_rule(spec.as_ref())
        .map_err(|error| ParseError::recurrence(text, error.to_string()))?;
    Ok(spec)
}

fn recurrence_spec(t: &str) -> Result<Option<RecurrenceSpec>, ParseError> {
    match t {
        "daily" | "every day" => return Ok(Some(RecurrenceSpec::EveryDays(1))),
        "weekly" | "every week" => return Ok(Some(RecurrenceSpec::EveryWeeks(1))),
        "monthly" | "every month" => return Ok(Some(RecurrenceSpec::EveryMonths(1))),
        "yearly" | "annually" | "every year" => return Ok(Some(RecurrenceSpec::EveryYears(1))),
        "quarterly" => return Ok(Some(RecurrenceSpec::EveryMonths(3))),
        "biweekly" | "fortnightly" => return Ok(Some(RecurrenceSpec::EveryWeeks(2))),
        "weekdays" | "every weekday" | "every weekdays" => {
            return Ok(Some(RecurrenceSpec::weekdays()));
        }
        "weekends" | "every weekend" | "every weekends" => {
            return Ok(Some(RecurrenceSpec::weekends()));
        }
        _ => {}
    }

    if let Some(rest) = t.strip_prefix("every ") {
        if let Some((amount, unit)) = rest.rsplit_once(' ')
            && is_numeric(amount)
        {
            let n = amount.trim().parse::<i64>().map_err(|_| {
                ParseError::recurrence(
                    t,
                    "interval must be a whole number between 1 and 4294967295",
                )
            })?;
            let spec = match unit {
                "day" | "days" => RecurrenceSpec::EveryDays(n),
                "week" | "weeks" => RecurrenceSpec::EveryWeeks(n),
                "month" | "months" => RecurrenceSpec::EveryMonths(n),
                "year" | "years" => RecurrenceSpec::EveryYears(n),
                _ => {
                    return Err(ParseError::recurrence(
                        t,
                        "interval unit must be days, weeks, months or years",
                    ));
                }
            };
            return Ok(Some(spec));
        }

        if let Some(day) = month_day(rest)? {
            return Ok(Some(RecurrenceSpec::OnMonthDay(day)));
        }

        if let Some(ordinal_part) = rest
            .strip_suffix(" of the month")
            .or_else(|| rest.strip_suffix(" of every month"))
            && let Some(day) = month_day(ordinal_part.trim())?
        {
            return Ok(Some(RecurrenceSpec::OnMonthDay(day)));
        }

        let days = parse_day_list(rest);
        if !days.is_empty() {
            return Ok(Some(RecurrenceSpec::OnWeekdays(WeekdaySet::new(days))));
        }
        if is_numeric(rest) {
            return Err(ParseError::recurrence(
                t,
                "month day must be a whole number between 1 and 31",
            ));
        }
    }

    if let Some(ordinal_part) = t
        .strip_suffix(" of every month")
        .or_else(|| t.strip_suffix(" of the month"))
        && let Some(day) = month_day(ordinal_part.trim())?
    {
        return Ok(Some(RecurrenceSpec::OnMonthDay(day)));
    }

    if t.contains(',') {
        let days = parse_day_list(t);
        if days.len() >= 2 {
            return Ok(Some(RecurrenceSpec::OnWeekdays(WeekdaySet::new(days))));
        }
    }

    Ok(None)
}

fn month_day(text: &str) -> Result<Option<u32>, ParseError> {
    let number = ["th", "st", "nd", "rd"]
        .iter()
        .find_map(|suffix| text.strip_suffix(suffix))
        .unwrap_or(text);
    if !is_numeric(number) {
        return Ok(None);
    }
    parse_ordinal_number(text)
        .filter(|day| (1..=31).contains(day))
        .map(Some)
        .ok_or_else(|| ParseError::recurrence(text, "month day must be between 1 and 31"))
}

fn parse_day_list(text: &str) -> Vec<Weekday> {
    let mut days = Vec::new();
    for part in text.split([',', ' ']) {
        let s = part.trim();
        if s.is_empty() {
            continue;
        }
        match parse_weekday_name(s) {
            Some(day) => days.push(day),
            None => return Vec::new(),
        }
    }
    days
}

pub(super) fn align_weekdays(draft: &mut ParseDraft, today: NaiveDate) -> Result<(), ParseError> {
    if draft.kind.is_template() {
        return Ok(());
    }
    let Some(RecurrenceSpec::OnWeekdays(set)) = draft.recurrence else {
        return Ok(());
    };
    let date = match draft.when {
        Some(WhenSpec::DateTime(dt)) => super::time::local_datetime(dt)?.date(),
        Some(WhenSpec::NaiveDate(date)) => date,
        None => today,
    };
    if draft.when.is_some() && set.contains(date.weekday()) {
        return Ok(());
    }
    let mut next = date;
    for _ in 0..7 {
        next = add_days(next, 1)?;
        if set.contains(next.weekday()) {
            draft.when = Some(match draft.when {
                Some(WhenSpec::DateTime(dt)) => {
                    WhenSpec::DateTime(date_at(next, super::time::local_datetime(dt)?.time())?)
                }
                Some(WhenSpec::NaiveDate(_)) => WhenSpec::NaiveDate(next),
                None => match draft.kind {
                    EntityKind::Event | EntityKind::Signal => {
                        WhenSpec::DateTime(date_at(next, default_time())?)
                    }
                    _ => WhenSpec::NaiveDate(next),
                },
            });
            return Ok(());
        }
    }
    Err(ParseError::recurrence(
        &draft.raw,
        "select at least one valid weekday",
    ))
}
