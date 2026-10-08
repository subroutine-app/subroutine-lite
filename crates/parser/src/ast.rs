use chrono::{DateTime, Duration, NaiveDate, NaiveTime, Utc, Weekday};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum EntityKind {
    Action,
    ActionTemplate,
    Event,
    EventTemplate,
    Signal,
    RoutineStep,
    Marker,
}

impl EntityKind {
    pub(crate) fn is_template(&self) -> bool {
        matches!(self, Self::ActionTemplate | Self::EventTemplate)
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Hash)]
pub struct WeekdaySet(pub u8);

fn weekday_bit(day: Weekday) -> u8 {
    match day {
        Weekday::Mon => 1 << 0,
        Weekday::Tue => 1 << 1,
        Weekday::Wed => 1 << 2,
        Weekday::Thu => 1 << 3,
        Weekday::Fri => 1 << 4,
        Weekday::Sat => 1 << 5,
        Weekday::Sun => 1 << 6,
    }
}

impl WeekdaySet {
    pub const EMPTY: Self = Self(0);

    pub fn new(days: impl IntoIterator<Item = Weekday>) -> Self {
        let mut bits = 0u8;
        for d in days {
            bits |= weekday_bit(d);
        }
        Self(bits)
    }

    pub fn weekdays() -> Self {
        Self::new([
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
        ])
    }

    pub fn weekends() -> Self {
        Self::new([Weekday::Sat, Weekday::Sun])
    }

    pub fn every_day() -> Self {
        Self::new([
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
            Weekday::Sat,
            Weekday::Sun,
        ])
    }

    pub fn contains(&self, day: Weekday) -> bool {
        self.0 & weekday_bit(day) != 0
    }

    pub fn len(&self) -> usize {
        self.0.count_ones() as usize
    }

    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = Weekday> + '_ {
        [
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
            Weekday::Sat,
            Weekday::Sun,
        ]
        .into_iter()
        .filter(|d| self.contains(*d))
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum RecurrenceSpec {
    EveryDays(i64),
    EveryWeeks(i64),
    EveryMonths(i64),
    EveryYears(i64),
    OnMonthDay(u32),
    OnWeekdays(WeekdaySet),
}

impl RecurrenceSpec {
    pub fn daily() -> Self {
        Self::EveryDays(1)
    }

    pub fn weekly() -> Self {
        Self::EveryWeeks(1)
    }

    pub fn monthly() -> Self {
        Self::EveryMonths(1)
    }

    pub fn yearly() -> Self {
        Self::EveryYears(1)
    }

    pub fn weekdays() -> Self {
        Self::OnWeekdays(WeekdaySet::weekdays())
    }

    pub fn weekends() -> Self {
        Self::OnWeekdays(WeekdaySet::weekends())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum HighlightKind {
    Title,
    When,
    Duration,
    Recurrence,
    Tag,
    Location,
    People,
    Priority,
    Sigil,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum WhenSpec {
    DateTime(DateTime<Utc>),
    NaiveDate(NaiveDate),
}

impl WhenSpec {
    pub fn as_datetime(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::DateTime(dt) => Some(*dt),
            Self::NaiveDate(_) => None,
        }
    }

    pub fn date(&self) -> NaiveDate {
        match self {
            Self::DateTime(dt) => dt.date_naive(),
            Self::NaiveDate(d) => *d,
        }
    }

    pub fn has_time(&self) -> bool {
        matches!(self, Self::DateTime(_))
    }

    pub fn unwrap_datetime(self) -> DateTime<Utc> {
        match self {
            Self::DateTime(dt) => dt,
            Self::NaiveDate(d) => {
                panic!("called unwrap_datetime() on a WhenSpec::NaiveDate({d})")
            }
        }
    }

    pub fn unwrap_naive_date(self) -> NaiveDate {
        match self {
            Self::NaiveDate(d) => d,
            Self::DateTime(dt) => {
                panic!("called unwrap_naive_date() on a WhenSpec::DateTime({dt})")
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ParseDraft {
    pub raw: String,
    pub kind: EntityKind,
    pub title: String,

    pub when: Option<WhenSpec>,
    pub naive_time: Option<NaiveTime>,
    pub duration: Option<Duration>,
    pub recurrence: Option<RecurrenceSpec>,
    pub recurrence_end_date: Option<NaiveDate>,
    pub recurrence_remaining: Option<u32>,

    pub content: Option<String>,
    pub warnings: Vec<String>,
    pub highlights: Vec<(std::ops::Range<usize>, HighlightKind)>,
}

impl ParseDraft {
    pub fn new(kind: EntityKind, raw: impl Into<String>) -> Self {
        Self {
            raw: raw.into(),
            kind,
            title: String::new(),
            when: None,
            naive_time: None,
            duration: None,
            recurrence: None,
            recurrence_end_date: None,
            recurrence_remaining: None,
            content: None,
            warnings: Vec::new(),
            highlights: Vec::new(),
        }
    }
}
