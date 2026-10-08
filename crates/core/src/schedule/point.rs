use std::ops::{Add, AddAssign, Sub, SubAssign};

use chrono::{
    DateTime, Datelike, Days, Local, Months, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, Utc,
    offset::LocalResult,
};
use chronoutil::RelativeDuration;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartPrecision {
    DateTime,
    Date,
    Unscheduled,
}

impl From<SchedulePoint> for StartPrecision {
    fn from(value: SchedulePoint) -> Self {
        match value {
            SchedulePoint::DateTime(_) => StartPrecision::DateTime,
            SchedulePoint::Date(_) => StartPrecision::Date,
        }
    }
}

impl From<Option<SchedulePoint>> for StartPrecision {
    fn from(value: Option<SchedulePoint>) -> Self {
        match value {
            Some(SchedulePoint::DateTime(_)) => StartPrecision::DateTime,
            Some(SchedulePoint::Date(_)) => StartPrecision::Date,
            None => StartPrecision::Unscheduled,
        }
    }
}

#[derive(Debug, Clone, Copy, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(untagged)]
pub enum SchedulePoint {
    DateTime(DateTime<Utc>),
    Date(NaiveDate),
}

impl SchedulePoint {
    pub fn with_date(&self, date: NaiveDate) -> Self {
        match self {
            SchedulePoint::DateTime(dt) => {
                SchedulePoint::DateTime(NaiveDateTime::new(date, dt.time()).and_utc())
            }
            SchedulePoint::Date(_) => SchedulePoint::Date(date),
        }
    }

    pub fn with_time(&self, time: NaiveTime) -> Self {
        SchedulePoint::DateTime(NaiveDateTime::new(self.date_naive(), time).and_utc())
    }

    pub fn without_time(&self) -> Self {
        match self {
            SchedulePoint::DateTime(dt) => SchedulePoint::Date(dt.date_naive()),
            SchedulePoint::Date(_) => *self,
        }
    }

    pub fn date_naive(&self) -> NaiveDate {
        match self {
            SchedulePoint::DateTime(dt) => dt.date_naive(),
            SchedulePoint::Date(date) => *date,
        }
    }

    pub fn time(&self) -> Option<NaiveTime> {
        match self {
            SchedulePoint::DateTime(dt) => Some(dt.time()),
            SchedulePoint::Date(_) => None,
        }
    }

    pub fn now() -> Self {
        SchedulePoint::DateTime(Utc::now())
    }

    pub fn today() -> Self {
        SchedulePoint::Date(Local::now().date_naive())
    }

    pub fn timestamp(&self) -> i64 {
        DateTime::<Utc>::from(*self).timestamp()
    }
}

impl From<SchedulePoint> for NaiveDate {
    fn from(value: SchedulePoint) -> Self {
        match value {
            SchedulePoint::Date(date) => date,
            SchedulePoint::DateTime(dt) => dt.with_timezone(&Local).date_naive(),
        }
    }
}

impl From<SchedulePoint> for DateTime<Utc> {
    fn from(value: SchedulePoint) -> Self {
        match value {
            SchedulePoint::DateTime(dt) => dt,
            SchedulePoint::Date(date) => date.and_time(NaiveTime::MIN).and_utc(),
        }
    }
}

impl From<SchedulePoint> for DateTime<Local> {
    fn from(value: SchedulePoint) -> Self {
        match value {
            SchedulePoint::DateTime(dt) => dt.with_timezone(&Local),
            SchedulePoint::Date(date) => {
                match date.and_time(NaiveTime::MIN).and_local_timezone(Local) {
                    LocalResult::Ambiguous(earliest, _latest) => earliest,
                    LocalResult::None => date
                        .and_time(NaiveTime::MIN)
                        .and_utc()
                        .with_timezone(&Local),
                    LocalResult::Single(dt) => dt,
                }
            }
        }
    }
}

impl From<NaiveDate> for SchedulePoint {
    fn from(value: NaiveDate) -> Self {
        SchedulePoint::Date(value)
    }
}

impl From<NaiveDateTime> for SchedulePoint {
    fn from(value: NaiveDateTime) -> Self {
        SchedulePoint::DateTime(value.and_utc())
    }
}

impl From<DateTime<Utc>> for SchedulePoint {
    fn from(value: DateTime<Utc>) -> Self {
        SchedulePoint::DateTime(value)
    }
}

impl From<DateTime<Local>> for SchedulePoint {
    fn from(value: DateTime<Local>) -> Self {
        SchedulePoint::DateTime(value.with_timezone(&Utc))
    }
}

impl Datelike for SchedulePoint {
    fn year(&self) -> i32 {
        match self {
            SchedulePoint::DateTime(dt) => dt.year(),
            SchedulePoint::Date(date) => date.year(),
        }
    }

    fn month(&self) -> u32 {
        match self {
            SchedulePoint::DateTime(dt) => dt.month(),
            SchedulePoint::Date(date) => date.month(),
        }
    }

    fn month0(&self) -> u32 {
        match self {
            SchedulePoint::DateTime(dt) => dt.month0(),
            SchedulePoint::Date(date) => date.month0(),
        }
    }

    fn day(&self) -> u32 {
        match self {
            SchedulePoint::DateTime(dt) => dt.day(),
            SchedulePoint::Date(date) => date.day(),
        }
    }

    fn day0(&self) -> u32 {
        match self {
            SchedulePoint::DateTime(dt) => dt.day0(),
            SchedulePoint::Date(date) => date.day0(),
        }
    }

    fn ordinal(&self) -> u32 {
        match self {
            SchedulePoint::DateTime(dt) => dt.ordinal(),
            SchedulePoint::Date(date) => date.ordinal(),
        }
    }

    fn ordinal0(&self) -> u32 {
        match self {
            SchedulePoint::DateTime(dt) => dt.ordinal0(),
            SchedulePoint::Date(date) => date.ordinal0(),
        }
    }

    fn weekday(&self) -> chrono::prelude::Weekday {
        match self {
            SchedulePoint::DateTime(dt) => dt.weekday(),
            SchedulePoint::Date(date) => date.weekday(),
        }
    }

    fn iso_week(&self) -> chrono::IsoWeek {
        match self {
            SchedulePoint::DateTime(dt) => dt.iso_week(),
            SchedulePoint::Date(date) => date.iso_week(),
        }
    }

    fn with_year(&self, year: i32) -> Option<Self> {
        match self {
            SchedulePoint::DateTime(dt) => dt.with_year(year).map(SchedulePoint::DateTime),
            SchedulePoint::Date(date) => date.with_year(year).map(SchedulePoint::Date),
        }
    }

    fn with_month(&self, month: u32) -> Option<Self> {
        match self {
            SchedulePoint::DateTime(dt) => dt.with_month(month).map(SchedulePoint::DateTime),
            SchedulePoint::Date(date) => date.with_month(month).map(SchedulePoint::Date),
        }
    }

    fn with_month0(&self, month0: u32) -> Option<Self> {
        match self {
            SchedulePoint::DateTime(dt) => dt.with_month0(month0).map(SchedulePoint::DateTime),
            SchedulePoint::Date(date) => date.with_month0(month0).map(SchedulePoint::Date),
        }
    }

    fn with_day(&self, day: u32) -> Option<Self> {
        match self {
            SchedulePoint::DateTime(dt) => dt.with_day(day).map(SchedulePoint::DateTime),
            SchedulePoint::Date(date) => date.with_day(day).map(SchedulePoint::Date),
        }
    }

    fn with_day0(&self, day0: u32) -> Option<Self> {
        match self {
            SchedulePoint::DateTime(dt) => dt.with_day0(day0).map(SchedulePoint::DateTime),
            SchedulePoint::Date(date) => date.with_day0(day0).map(SchedulePoint::Date),
        }
    }

    fn with_ordinal(&self, ordinal: u32) -> Option<Self> {
        match self {
            SchedulePoint::DateTime(dt) => dt.with_ordinal(ordinal).map(SchedulePoint::DateTime),
            SchedulePoint::Date(date) => date.with_ordinal(ordinal).map(SchedulePoint::Date),
        }
    }

    fn with_ordinal0(&self, ordinal0: u32) -> Option<Self> {
        match self {
            SchedulePoint::DateTime(dt) => dt.with_ordinal0(ordinal0).map(SchedulePoint::DateTime),
            SchedulePoint::Date(date) => date.with_ordinal0(ordinal0).map(SchedulePoint::Date),
        }
    }
}

impl Add<TimeDelta> for SchedulePoint {
    type Output = Self;

    fn add(self, rhs: TimeDelta) -> Self::Output {
        match self {
            SchedulePoint::DateTime(dt) => SchedulePoint::DateTime(dt + rhs),
            SchedulePoint::Date(date) => SchedulePoint::Date(date + rhs),
        }
    }
}

impl Add<RelativeDuration> for SchedulePoint {
    type Output = Self;

    fn add(self, rhs: RelativeDuration) -> Self::Output {
        match self {
            SchedulePoint::DateTime(dt) => SchedulePoint::DateTime(dt + rhs),
            SchedulePoint::Date(date) => SchedulePoint::Date(date + rhs),
        }
    }
}

impl AddAssign<TimeDelta> for SchedulePoint {
    fn add_assign(&mut self, rhs: TimeDelta) {
        *self = *self + rhs;
    }
}

impl AddAssign<RelativeDuration> for SchedulePoint {
    fn add_assign(&mut self, rhs: RelativeDuration) {
        *self = *self + rhs;
    }
}

impl Sub<TimeDelta> for SchedulePoint {
    type Output = Self;

    fn sub(self, rhs: TimeDelta) -> Self::Output {
        match self {
            SchedulePoint::DateTime(dt) => SchedulePoint::DateTime(dt - rhs),
            SchedulePoint::Date(date) => SchedulePoint::Date(date - rhs),
        }
    }
}

impl Sub<RelativeDuration> for SchedulePoint {
    type Output = Self;

    fn sub(self, rhs: RelativeDuration) -> Self::Output {
        match self {
            SchedulePoint::DateTime(dt) => SchedulePoint::DateTime(dt - rhs),
            SchedulePoint::Date(date) => SchedulePoint::Date(date - rhs),
        }
    }
}

impl SubAssign<TimeDelta> for SchedulePoint {
    fn sub_assign(&mut self, rhs: TimeDelta) {
        *self = *self - rhs;
    }
}

impl SubAssign<RelativeDuration> for SchedulePoint {
    fn sub_assign(&mut self, rhs: RelativeDuration) {
        *self = *self - rhs;
    }
}

impl Add<Months> for SchedulePoint {
    type Output = Self;

    fn add(self, rhs: Months) -> Self::Output {
        match self {
            SchedulePoint::DateTime(dt) => SchedulePoint::DateTime(dt + rhs),
            SchedulePoint::Date(date) => SchedulePoint::Date(date + rhs),
        }
    }
}

impl Sub<Months> for SchedulePoint {
    type Output = Self;

    fn sub(self, rhs: Months) -> Self::Output {
        match self {
            SchedulePoint::DateTime(dt) => SchedulePoint::DateTime(dt - rhs),
            SchedulePoint::Date(date) => SchedulePoint::Date(date - rhs),
        }
    }
}

impl Add<Days> for SchedulePoint {
    type Output = Self;

    fn add(self, rhs: Days) -> Self::Output {
        match self {
            SchedulePoint::DateTime(dt) => SchedulePoint::DateTime(dt + rhs),
            SchedulePoint::Date(date) => SchedulePoint::Date(date + rhs),
        }
    }
}

impl Sub<Days> for SchedulePoint {
    type Output = Self;

    fn sub(self, rhs: Days) -> Self::Output {
        match self {
            SchedulePoint::DateTime(dt) => SchedulePoint::DateTime(dt - rhs),
            SchedulePoint::Date(date) => SchedulePoint::Date(date - rhs),
        }
    }
}
