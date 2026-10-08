pub(super) mod bounds;

use chrono::{Datelike, Duration, Local, NaiveDate, Weekday};

use super::{
    date::{parse_ordinal_number, parse_weekday_name},
    time::{date_at, default_time},
};
use crate::{
    ast::{EntityKind, ParseDraft, RecurrenceSpec, WeekdaySet, WhenSpec},
    lexer::{SpannedToken, Token},
};

pub(super) fn has_recurrence_clause(tokens: &[SpannedToken]) -> bool {
    (0..tokens.len()).any(|index| try_nl_recurrence(tokens, index).is_some())
}

pub(super) fn try_nl_recurrence(
    tokens: &[SpannedToken],
    i: usize,
) -> Option<(RecurrenceSpec, usize)> {
    let n = tokens.len();
    if i >= n {
        return None;
    }
    let lower = tokens[i].text.to_ascii_lowercase();

    match lower.as_str() {
        "daily" => return Some((RecurrenceSpec::EveryDays(1), 1)),
        "weekly" => return Some((RecurrenceSpec::EveryWeeks(1), 1)),
        "monthly" => return Some((RecurrenceSpec::EveryMonths(1), 1)),
        "yearly" | "annually" => return Some((RecurrenceSpec::EveryYears(1), 1)),
        "weekdays" => return Some((RecurrenceSpec::weekdays(), 1)),
        "weekends" => return Some((RecurrenceSpec::weekends(), 1)),
        _ => {}
    }

    if lower == "every" && i + 1 < n {
        return try_every_clause(tokens, i);
    }

    if matches!(tokens[i].token, Token::OrdinalDay)
        && let Some(day) = parse_ordinal_number(&tokens[i].text)
        && (1..=31).contains(&day)
    {
        let suffixes: &[(&str, &str, usize)] = &[
            ("of", "every", 3),
            ("of", "the", 3),
        ];
        for &(word1, word2, extra) in suffixes {
            if i + extra < n
                && tokens[i + 1].text.eq_ignore_ascii_case(word1)
                && tokens[i + 2].text.eq_ignore_ascii_case(word2)
                && tokens[i + 3].text.eq_ignore_ascii_case("month")
            {
                return Some((RecurrenceSpec::OnMonthDay(day), 1 + extra));
            }
        }
    }

    if lower.contains(',')
        && let Some(rec) = try_recurrence_text(&lower)
    {
        return Some((rec, 1));
    }

    None
}

fn try_every_clause(tokens: &[SpannedToken], i: usize) -> Option<(RecurrenceSpec, usize)> {
    const STOP_WORDS: &[&str] = &["at", "for", "in", "with", "lasting", "takes", "by", "until"];

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
            Token::Punct => {
                if tokens[end].text == "," {
                    end += 1;
                } else {
                    break;
                }
            }
            _ => break,
        }
        if end > i + 8 {
            break;
        }
    }

    for span_end in (i + 2..=end).rev() {
        let text = tokens[i..span_end]
            .iter()
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let text_lower = text.to_ascii_lowercase();
        if let Some(rec) = try_recurrence_text(&text_lower) {
            return Some((rec, span_end - i));
        }
    }
    None
}

pub fn try_recurrence_text(text: &str) -> Option<RecurrenceSpec> {
    let t = text.trim();
    match t {
        "daily" => return Some(RecurrenceSpec::EveryDays(1)),
        "weekly" => return Some(RecurrenceSpec::EveryWeeks(1)),
        "monthly" | "every month" => return Some(RecurrenceSpec::EveryMonths(1)),
        "yearly" | "annually" | "every year" => return Some(RecurrenceSpec::EveryYears(1)),
        "quarterly" => return Some(RecurrenceSpec::EveryMonths(3)),
        "biweekly" | "fortnightly" => return Some(RecurrenceSpec::EveryWeeks(2)),
        "weekdays" | "every weekday" | "every weekdays" => return Some(RecurrenceSpec::weekdays()),
        "weekends" | "every weekend" | "every weekends" => return Some(RecurrenceSpec::weekends()),
        "every day" => return Some(RecurrenceSpec::EveryDays(1)),
        "every week" => return Some(RecurrenceSpec::EveryWeeks(1)),
        _ => {}
    }

    if let Some(rest) = t.strip_prefix("every ") {
        if let Some(inner) = rest.strip_suffix(" days")
            && let Ok(n) = inner.trim().parse::<i64>()
        {
            return Some(RecurrenceSpec::EveryDays(n));
        }
        if let Some(inner) = rest.strip_suffix(" weeks")
            && let Ok(n) = inner.trim().parse::<i64>()
        {
            return Some(RecurrenceSpec::EveryWeeks(n));
        }
        if let Some(inner) = rest.strip_suffix(" months")
            && let Ok(n) = inner.trim().parse::<i64>()
        {
            return Some(RecurrenceSpec::EveryMonths(n));
        }
        if let Some(inner) = rest.strip_suffix(" years")
            && let Ok(n) = inner.trim().parse::<i64>()
        {
            return Some(RecurrenceSpec::EveryYears(n));
        }

        if let Some(day) = parse_ordinal_number(rest)
            && (1..=31).contains(&day)
        {
            return Some(RecurrenceSpec::OnMonthDay(day));
        }

        if let Some(ordinal_part) = rest
            .strip_suffix(" of the month")
            .or_else(|| rest.strip_suffix(" of every month"))
            && let Some(day) = parse_ordinal_number(ordinal_part.trim())
            && (1..=31).contains(&day)
        {
            return Some(RecurrenceSpec::OnMonthDay(day));
        }

        let days = parse_day_list(rest);
        if !days.is_empty() {
            return Some(RecurrenceSpec::OnWeekdays(WeekdaySet::new(days)));
        }
    }

    if let Some(ordinal_part) = t.strip_suffix(" of every month")
        && let Some(day) = parse_ordinal_number(ordinal_part.trim())
        && (1..=31).contains(&day)
    {
        return Some(RecurrenceSpec::OnMonthDay(day));
    }
    if let Some(ordinal_part) = t.strip_suffix(" of the month")
        && let Some(day) = parse_ordinal_number(ordinal_part.trim())
        && (1..=31).contains(&day)
    {
        return Some(RecurrenceSpec::OnMonthDay(day));
    }

    if t.contains(',') {
        let days = parse_day_list(t);
        if days.len() >= 2 {
            return Some(RecurrenceSpec::OnWeekdays(WeekdaySet::new(days)));
        }
    }

    None
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

pub(super) fn align_weekdays(draft: &mut ParseDraft, today: NaiveDate) {
    let date_spec: fn(NaiveDate) -> WhenSpec = match draft.kind {
        EntityKind::Action | EntityKind::RoutineStep | EntityKind::Marker => WhenSpec::NaiveDate,
        EntityKind::Event | EntityKind::Signal => {
            |date| WhenSpec::DateTime(date_at(date, default_time()))
        }
        EntityKind::ActionTemplate | EntityKind::EventTemplate => return,
    };

    if let Some(RecurrenceSpec::OnWeekdays(set)) = draft.recurrence {
        match draft.when.clone() {
            None => {
                let mut d = today + Duration::days(1);
                for _ in 0..7 {
                    if set.contains(d.weekday()) {
                        draft.when = Some(date_spec(d));
                        break;
                    }
                    d += Duration::days(1);
                }
            }
            Some(when) if !set.contains(when.date().weekday()) => {
                let cur_date = when.date();
                let mut d = cur_date + Duration::days(1);
                for _ in 0..7 {
                    if set.contains(d.weekday()) {
                        draft.when = Some(match when {
                            WhenSpec::DateTime(dt) => {
                                let t = dt.with_timezone(&Local).time();
                                WhenSpec::DateTime(date_at(d, t))
                            }
                            WhenSpec::NaiveDate(_) => WhenSpec::NaiveDate(d),
                        });
                        break;
                    }
                    d += Duration::days(1);
                }
            }
            _ => {}
        }
    }
}
