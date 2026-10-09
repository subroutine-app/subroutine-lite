use std::num::NonZeroU32;

use chrono::{Datelike, Days, Duration, Months, NaiveDate, Offset, Weekday};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::SchedulePoint;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recurrence {
    pub rule: RecurrenceRule,
    pub end_date: Option<NaiveDate>,
    pub remaining: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<Tz>,
}

impl Recurrence {
    pub fn new(rule: RecurrenceRule) -> Self {
        Self {
            rule,
            end_date: None,
            remaining: None,
            timezone: None,
        }
    }

    pub fn in_local_timezone(rule: RecurrenceRule) -> Result<Self, &'static str> {
        let timezone = iana_time_zone::get_timezone()
            .ok()
            .and_then(|name| name.parse().ok())
            .ok_or("could not determine the local timezone")?;
        Ok(Self {
            timezone: Some(timezone),
            ..Self::new(rule)
        })
    }

    pub fn with_end_date(mut self, end_date: Option<NaiveDate>) -> Self {
        self.end_date = end_date;
        self
    }

    pub fn with_remaining(mut self, remaining: Option<u32>) -> Self {
        self.remaining = remaining;
        self
    }

    pub fn set_rule(mut self, rule: RecurrenceRule) -> Self {
        self.rule = rule;
        self
    }

    pub fn set_end_date(mut self, end_date: Option<NaiveDate>) -> Self {
        self.end_date = end_date;
        self
    }

    pub fn set_remaining(mut self, remaining: Option<u32>) -> Self {
        self.remaining = remaining;
        self
    }

    pub fn occurs_on(&self, date: NaiveDate) -> bool {
        if self.remaining == Some(0) {
            return false;
        }
        if let Some(end) = self.end_date
            && date > end
        {
            return false;
        }
        self.rule.occurs_on(date)
    }

    pub(crate) fn fast_forward_before(
        &self,
        after: impl Into<SchedulePoint>,
        target: NaiveDate,
    ) -> (SchedulePoint, Self) {
        let original = after.into();
        let Some(after) = self.calendar_point(original) else {
            return (original, *self);
        };
        let target = if matches!(original, SchedulePoint::DateTime(_)) {
            target.checked_sub_days(Days::new(2)).unwrap_or(target)
        } else {
            target
        };
        if let RecurrenceRule::WeeklyDays(days) = self.rule {
            let Some(candidate) = (1..=7)
                .filter_map(|offset| target.checked_sub_days(Days::new(offset)))
                .find(|date| days.contains(date.weekday()))
            else {
                return (original, *self);
            };
            let distance = (candidate - after.date_naive()).num_days();
            if distance <= 0 {
                return (original, *self);
            }
            let full_weeks = distance as u64 / 7;
            let remainder = distance as u64 % 7;
            let days_per_week = days.iter(Weekday::Mon).count() as u64;
            let mut skipped = full_weeks * days_per_week;
            let full_week_days = full_weeks * 7;
            skipped += (1..=remainder)
                .filter(|offset| {
                    after
                        .checked_add_days(Days::new(full_week_days + *offset))
                        .is_some_and(|point| days.contains(point.weekday()))
                })
                .count() as u64;
            let mut point = after.with_date(candidate);
            while skipped > 0 && self.resolve_point(point).is_none() {
                let Some(previous) = (1..=7)
                    .filter_map(|days| point.date_naive().checked_sub_days(Days::new(days)))
                    .find(|date| days.contains(date.weekday()))
                else {
                    return (original, *self);
                };
                point = after.with_date(previous);
                skipped -= 1;
            }
            if skipped == 0 {
                return (original, *self);
            }
            if self
                .remaining
                .is_some_and(|remaining| skipped >= u64::from(remaining))
            {
                let mut exhausted = *self;
                exhausted.remaining = Some(0);
                return (original, exhausted);
            }
            let mut recurrence = *self;
            recurrence.remaining = recurrence
                .remaining
                .map(|remaining| remaining - skipped as u32);
            return self
                .resolve_point(point)
                .map(|point| (point, recurrence))
                .unwrap_or((original, *self));
        }

        let interval_days = match self.rule {
            RecurrenceRule::Relative { unit, interval } => match unit {
                RecurrenceUnit::Days => u64::from(interval.get()),
                RecurrenceUnit::Weeks => u64::from(interval.get()) * 7,
                RecurrenceUnit::Months | RecurrenceUnit::Years => return (original, *self),
            },
            _ => return (original, *self),
        };
        let distance = (target - after.date_naive()).num_days();
        if distance <= 1 {
            return (original, *self);
        }
        let skipped = (distance as u64 - 1) / interval_days;
        let mut skipped = self
            .remaining
            .map_or(skipped, |remaining| skipped.min(u64::from(remaining)));
        let point = loop {
            if skipped == 0 {
                return (original, *self);
            }
            if let Some(point) = skipped
                .checked_mul(interval_days)
                .and_then(|days| after.checked_add_days(Days::new(days)))
                .and_then(|point| self.resolve_point(point))
            {
                break point;
            }
            skipped -= 1;
        };
        let mut recurrence = *self;
        recurrence.remaining = recurrence
            .remaining
            .map(|remaining| remaining - skipped as u32);
        (point, recurrence)
    }

    pub fn next(&self, after: impl Into<SchedulePoint>) -> Option<SchedulePoint> {
        self.next_counted(after.into()).map(|(point, _)| point)
    }

    fn next_counted(&self, after: SchedulePoint) -> Option<(SchedulePoint, u32)> {
        let mut cursor = self.calendar_point(after)?;
        let mut consumed = 0u32;
        loop {
            if self
                .remaining
                .is_some_and(|remaining| consumed >= remaining)
            {
                return None;
            }
            let next = self.rule.next(cursor)?;
            if next <= cursor || self.end_date.is_some_and(|end| next.date_naive() > end) {
                return None;
            }
            consumed = consumed.checked_add(1)?;
            if let Some(point) = self.resolve_point(next).filter(|point| *point > after) {
                return Some((point, consumed));
            }
            cursor = next;
        }
    }

    fn calendar_point(&self, point: SchedulePoint) -> Option<SchedulePoint> {
        match (point, self.timezone) {
            (SchedulePoint::DateTime(datetime), Some(timezone)) => {
                let offset = datetime
                    .with_timezone(&timezone)
                    .offset()
                    .fix()
                    .local_minus_utc();
                datetime
                    .checked_add_signed(Duration::seconds(i64::from(offset)))
                    .map(SchedulePoint::DateTime)
            }
            _ => Some(point),
        }
    }

    fn resolve_point(&self, point: SchedulePoint) -> Option<SchedulePoint> {
        match (point, self.timezone) {
            (SchedulePoint::DateTime(datetime), Some(timezone)) => datetime
                .naive_utc()
                .and_local_timezone(timezone)
                .earliest()
                .map(|datetime| SchedulePoint::DateTime(datetime.to_utc())),
            _ => Some(point),
        }
    }

    pub fn advance(&self, after: impl Into<SchedulePoint>) -> Option<(SchedulePoint, Self)> {
        let (next, consumed) = self.next_counted(after.into())?;
        let mut advanced = *self;
        advanced.remaining = advanced.remaining.map(|remaining| remaining - consumed);
        Some((next, advanced))
    }

    pub fn describe(&self) -> String {
        let rule = self.rule.describe();
        match (self.remaining, self.end_date) {
            (Some(0), _) => "finished".to_string(),
            (Some(n), _) => format!("{rule}, {n} left"),
            (None, Some(end)) => format!("{rule} until {}", end.format("%b %-d")),
            (None, None) => rule,
        }
    }
}

pub(crate) fn occurrence_id(
    lineage_id: uuid::Uuid,
    item_kind: &str,
    point: SchedulePoint,
) -> uuid::Uuid {
    let point = match point {
        SchedulePoint::DateTime(datetime) => format!("time:{}", datetime.to_rfc3339()),
        SchedulePoint::Date(date) => format!("date:{date}"),
    };
    uuid::Uuid::new_v5(&lineage_id, format!("{item_kind}:{point}").as_bytes())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
pub enum RecurrenceRule {
    #[serde(rename = "year")]
    Anniversary(NaiveDate),
    #[serde(rename = "day")]
    MonthlyDay(#[serde(deserialize_with = "deserialize_month_day")] u32),
    #[serde(rename = "weekday")]
    MonthlyWeekday {
        #[serde(with = "weekday_serde")]
        weekday: Weekday,
        ordinal: WeekOrdinal,
    },
    #[serde(
        rename = "week",
        serialize_with = "serialize_weekday_set",
        deserialize_with = "deserialize_weekday_set"
    )]
    WeeklyDays(chrono::WeekdaySet),
    Relative {
        unit: RecurrenceUnit,
        interval: NonZeroU32,
    },
}

impl RecurrenceRule {
    pub fn days(n: u32) -> Self {
        Self::relative(RecurrenceUnit::Days, n)
    }

    pub fn weeks(n: u32) -> Self {
        Self::relative(RecurrenceUnit::Weeks, n)
    }

    pub fn months(n: u32) -> Self {
        Self::relative(RecurrenceUnit::Months, n)
    }

    pub fn years(n: u32) -> Self {
        Self::relative(RecurrenceUnit::Years, n)
    }

    fn relative(unit: RecurrenceUnit, interval: u32) -> Self {
        Self::Relative {
            unit,
            interval: NonZeroU32::new(interval).expect("recurrence interval must be non-zero"),
        }
    }

    fn occurs_on(&self, date: NaiveDate) -> bool {
        match self {
            RecurrenceRule::Anniversary(start_date) => {
                date.month() == start_date.month() && date.day() == start_date.day()
            }
            RecurrenceRule::MonthlyDay(day) => date.day() == *day,
            RecurrenceRule::MonthlyWeekday { weekday, ordinal } => {
                ordinal.nth_weekday_of_month(date.year(), date.month(), *weekday) == Some(date)
            }
            RecurrenceRule::WeeklyDays(weekday_set) => weekday_set.contains(date.weekday()),
            RecurrenceRule::Relative { .. } => false,
        }
    }

    pub fn next(&self, after: impl Into<SchedulePoint>) -> Option<SchedulePoint> {
        let after = after.into();
        match self {
            RecurrenceRule::Anniversary(start) => Self::next_yearly(after, |year| {
                start.with_year(year).map(|date| after.with_date(date))
            }),
            RecurrenceRule::MonthlyDay(day) => Self::next_monthly(after, |year, month| {
                NaiveDate::from_ymd_opt(year, month, *day)
            }),
            RecurrenceRule::MonthlyWeekday { weekday, ordinal } => {
                Self::next_monthly(after, |year, month| {
                    ordinal.nth_weekday_of_month(year, month, *weekday)
                })
            }
            RecurrenceRule::WeeklyDays(weekday_set) => {
                if weekday_set.is_empty() {
                    return None;
                }

                let today = after.weekday();
                let offset_from_today = |day: Weekday| {
                    (day.num_days_from_monday() as i64 - today.num_days_from_monday() as i64)
                        .rem_euclid(7) as u64
                };

                let mut candidates = weekday_set.iter(today);
                let first = candidates.next()?;
                let offset = if first == today {
                    candidates.next().map(offset_from_today).unwrap_or(7)
                } else {
                    offset_from_today(first)
                };

                after.checked_add_days(Days::new(offset))
            }
            RecurrenceRule::Relative { unit, interval } => {
                let interval = interval.get();
                match unit {
                    RecurrenceUnit::Days => after.checked_add_days(Days::new(u64::from(interval))),
                    RecurrenceUnit::Weeks => {
                        after.checked_add_days(Days::new(u64::from(interval) * 7))
                    }
                    RecurrenceUnit::Months => after.checked_add_months(Months::new(interval)),
                    RecurrenceUnit::Years => {
                        after.checked_add_months(Months::new(interval.checked_mul(12)?))
                    }
                }
            }
        }
    }

    fn next_yearly(
        after: SchedulePoint,
        candidate_for_year: impl Fn(i32) -> Option<SchedulePoint>,
    ) -> Option<SchedulePoint> {
        (after.year()..)
            .take(8)
            .find_map(|year| candidate_for_year(year).filter(|candidate| *candidate > after))
    }

    fn next_monthly(
        after: SchedulePoint,
        candidate_in_month: impl Fn(i32, u32) -> Option<NaiveDate>,
    ) -> Option<SchedulePoint> {
        let today = after.date_naive();
        let first_of_month = NaiveDate::from_ymd_opt(after.year(), after.month(), 1)?;
        (0..24u32)
            .filter_map(|months_ahead| {
                let cursor = first_of_month.checked_add_months(Months::new(months_ahead))?;
                candidate_in_month(cursor.year(), cursor.month())
            })
            .find(|date| *date > today)
            .map(|date| after.with_date(date))
    }

    pub fn describe(&self) -> String {
        match self {
            RecurrenceRule::Anniversary(start) => {
                format!("every {}", start.format("%B %-d"))
            }
            RecurrenceRule::MonthlyDay(day) => format!("every {}", fmt_ordinal(*day)),
            RecurrenceRule::MonthlyWeekday { weekday, ordinal } => {
                format!(
                    "every {} {}",
                    ordinal.label(),
                    capitalize(weekday_name(*weekday))
                )
            }
            RecurrenceRule::WeeklyDays(days) => {
                let names: Vec<&str> = days.iter(Weekday::Mon).map(weekday_abbreviation).collect();
                if names.is_empty() {
                    "never".to_string()
                } else {
                    format!("every {}", names.join(", "))
                }
            }
            RecurrenceRule::Relative { unit, interval } => {
                let interval = interval.get();
                if interval == 1 {
                    format!("every {}", unit.label())
                } else {
                    format!("every {interval} {}s", unit.label())
                }
            }
        }
    }
}

impl std::fmt::Display for RecurrenceRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.describe())
    }
}
fn fmt_ordinal(n: u32) -> String {
    let suffix = match n % 100 {
        11..=13 => "th",
        _ => match n % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        },
    };
    format!("{}{}", n, suffix)
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecurrenceUnit {
    Days,
    Weeks,
    Months,
    Years,
}

impl RecurrenceUnit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Days => "days",
            Self::Weeks => "weeks",
            Self::Months => "months",
            Self::Years => "years",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "days" => Some(Self::Days),
            "weeks" => Some(Self::Weeks),
            "months" => Some(Self::Months),
            "years" => Some(Self::Years),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Days => "day",
            Self::Weeks => "week",
            Self::Months => "month",
            Self::Years => "year",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WeekOrdinal {
    First,
    Second,
    Third,
    Fourth,
    Last,
}

impl From<WeekOrdinal> for u8 {
    fn from(ordinal: WeekOrdinal) -> Self {
        match ordinal {
            WeekOrdinal::First => 1,
            WeekOrdinal::Second => 2,
            WeekOrdinal::Third => 3,
            WeekOrdinal::Fourth => 4,
            WeekOrdinal::Last => 5,
        }
    }
}

impl WeekOrdinal {
    pub fn label(self) -> &'static str {
        match self {
            Self::First => "first",
            Self::Second => "second",
            Self::Third => "third",
            Self::Fourth => "fourth",
            Self::Last => "last",
        }
    }

    pub fn nth_weekday_of_month(
        self,
        year: i32,
        month: u32,
        weekday: Weekday,
    ) -> Option<NaiveDate> {
        let try_ordinal =
            |ordinal: u8| NaiveDate::from_weekday_of_month_opt(year, month, weekday, ordinal);
        match self {
            Self::Last => try_ordinal(5).or_else(|| try_ordinal(4)),
            _ => try_ordinal(u8::from(self)),
        }
    }
}

fn serialize_weekday_set<S>(days: &chrono::WeekdaySet, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let names: Vec<&str> = days.iter(Weekday::Mon).map(weekday_name).collect();
    names.serialize(serializer)
}

fn deserialize_weekday_set<'de, D>(deserializer: D) -> Result<chrono::WeekdaySet, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    let names: Vec<String> = Vec::deserialize(deserializer)?;
    let mut days = chrono::WeekdaySet::EMPTY;
    for name in &names {
        let weekday = weekday_from_name(name)
            .ok_or_else(|| Error::custom(format!("invalid day of week: {name}")))?;
        days.insert(weekday);
    }
    if days.is_empty() {
        return Err(Error::custom("select at least one weekday"));
    }
    Ok(days)
}

fn deserialize_month_day<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let day = u32::deserialize(deserializer)?;
    if !(1..=31).contains(&day) {
        return Err(serde::de::Error::custom(
            "month day must be between 1 and 31",
        ));
    }
    Ok(day)
}

fn weekday_name(weekday: Weekday) -> &'static str {
    match weekday {
        Weekday::Mon => "monday",
        Weekday::Tue => "tuesday",
        Weekday::Wed => "wednesday",
        Weekday::Thu => "thursday",
        Weekday::Fri => "friday",
        Weekday::Sat => "saturday",
        Weekday::Sun => "sunday",
    }
}

fn weekday_abbreviation(weekday: Weekday) -> &'static str {
    match weekday {
        Weekday::Mon => "Mon",
        Weekday::Tue => "Tue",
        Weekday::Wed => "Wed",
        Weekday::Thu => "Thur",
        Weekday::Fri => "Fri",
        Weekday::Sat => "Sat",
        Weekday::Sun => "Sun",
    }
}

fn weekday_from_name(name: &str) -> Option<Weekday> {
    match name.to_ascii_lowercase().as_str() {
        "monday" => Some(Weekday::Mon),
        "tuesday" => Some(Weekday::Tue),
        "wednesday" => Some(Weekday::Wed),
        "thursday" => Some(Weekday::Thu),
        "friday" => Some(Weekday::Fri),
        "saturday" => Some(Weekday::Sat),
        "sunday" => Some(Weekday::Sun),
        _ => None,
    }
}

mod weekday_serde {
    use chrono::Weekday;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    use super::{weekday_from_name, weekday_name};

    pub fn serialize<S: Serializer>(weekday: &Weekday, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(weekday_name(*weekday))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Weekday, D::Error> {
        let name = String::deserialize(deserializer)?;
        weekday_from_name(&name)
            .ok_or_else(|| Error::custom(format!("invalid day of week: {name}")))
    }
}
