
use std::rc::Rc;

use chrono::{Datelike, Local, NaiveDate};
use gpui::SharedString;
use gpui_kit::datetime::{
    Clock, DateAdapter, Day, MonthCell, MonthGrid, MonthKey, Selectability, SharedDateAdapter,
    TimeOfDay,
};

const DATE_FORMAT: &str = "%m/%d/%Y";

const DATE_HINT: &str = "MM/DD/YYYY";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InclusiveDateRange {
    start: NaiveDate,
    end: NaiveDate,
}

impl InclusiveDateRange {
    pub fn new(a: NaiveDate, b: NaiveDate) -> Self {
        Self {
            start: a.min(b),
            end: a.max(b),
        }
    }

    pub fn single(date: NaiveDate) -> Self {
        Self::new(date, date)
    }

    pub fn start(self) -> NaiveDate {
        self.start
    }

    pub fn end(self) -> NaiveDate {
        self.end
    }

    pub fn contains(self, date: NaiveDate) -> bool {
        self.start <= date && date <= self.end
    }

    pub fn day_count(self) -> u32 {
        u32::try_from((self.end - self.start).num_days() + 1).unwrap_or(u32::MAX)
    }
}

#[derive(Debug, Default)]
pub struct ChronoDates;

impl ChronoDates {
    pub fn shared() -> SharedDateAdapter {
        Rc::new(Self)
    }

    fn to_date(day: Day) -> Option<NaiveDate> {
        i32::try_from(day.0)
            .ok()
            .and_then(NaiveDate::from_num_days_from_ce_opt)
    }

    fn from_date(date: NaiveDate) -> Day {
        Day(i64::from(date.num_days_from_ce()))
    }

    fn to_year_month(month: MonthKey) -> Option<(i32, u32)> {
        let year = i32::try_from(month.0.div_euclid(12)).ok()?;
        let index = u32::try_from(month.0.rem_euclid(12)).ok()?;
        Some((year, index + 1))
    }

    fn from_year_month(year: i32, month: u32) -> MonthKey {
        MonthKey(i64::from(year) * 12 + i64::from(month) - 1)
    }
}

impl DateAdapter for ChronoDates {
    fn today(&self) -> Option<Day> {
        Some(Self::from_date(Local::now().date_naive()))
    }

    fn month_of(&self, day: Day) -> MonthKey {
        Self::to_date(day)
            .map(|date| Self::from_year_month(date.year(), date.month()))
            .unwrap_or(MonthKey(0))
    }

    fn month_grid(&self, month: MonthKey) -> MonthGrid {
        let Some((year, month_number)) = Self::to_year_month(month) else {
            return MonthGrid::new(Vec::new());
        };
        let Some(first) = NaiveDate::from_ymd_opt(year, month_number, 1) else {
            return MonthGrid::new(Vec::new());
        };

        let lead = first.weekday().num_days_from_sunday() as i64;
        let start = first - chrono::Duration::days(lead);

        let weeks = (0..6)
            .map(|week| {
                (0..7)
                    .map(|weekday| {
                        let offset = week * 7 + weekday;
                        match start.checked_add_signed(chrono::Duration::days(offset)) {
                            Some(date) if date.month() == month_number && date.year() == year => {
                                MonthCell::Day(Self::from_date(date))
                            }
                            Some(date) => MonthCell::Adjacent(Self::from_date(date)),
                            None => MonthCell::Empty,
                        }
                    })
                    .collect()
            })
            .collect::<Vec<Vec<MonthCell>>>();

        MonthGrid::new(weeks)
    }

    fn month_label(&self, month: MonthKey) -> SharedString {
        Self::to_year_month(month)
            .and_then(|(year, month_number)| NaiveDate::from_ymd_opt(year, month_number, 1))
            .map(|date| SharedString::from(date.format("%B %Y").to_string()))
            .unwrap_or_else(|| SharedString::from("—"))
    }

    fn weekday_labels(&self) -> Vec<SharedString> {
        ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
            .into_iter()
            .map(SharedString::from)
            .collect()
    }

    fn format_day(&self, day: Day) -> SharedString {
        Self::to_date(day)
            .map(|date| SharedString::from(date.format(DATE_FORMAT).to_string()))
            .unwrap_or_default()
    }

    fn day_label(&self, day: Day) -> SharedString {
        Self::to_date(day)
            .map(|date| SharedString::from(date.day().to_string()))
            .unwrap_or_default()
    }

    fn parse_day(&self, text: &str) -> Result<Day, SharedString> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(SharedString::from(format!("Enter a date as {DATE_HINT}.")));
        }
        NaiveDate::parse_from_str(trimmed, DATE_FORMAT)
            .map(Self::from_date)
            .map_err(|_| SharedString::from(format!("{trimmed:?} is not a date. Use {DATE_HINT}.")))
    }

    fn shift_month(&self, month: MonthKey, delta: i32) -> Option<MonthKey> {
        let shifted = month.0.checked_add(i64::from(delta))?;
        let candidate = MonthKey(shifted);
        Self::to_year_month(candidate)
            .and_then(|(year, number)| NaiveDate::from_ymd_opt(year, number, 1))
            .map(|_| candidate)
    }

    fn is_selectable(&self, _day: Day) -> Selectability {
        Selectability::Selectable
    }

    fn days_in(&self, start: Day, end: Day) -> Option<Vec<Day>> {
        if end.0 < start.0 {
            return Some(Vec::new());
        }
        const MAX_SPAN: i64 = 4000;
        if end.0 - start.0 > MAX_SPAN {
            return None;
        }
        Some((start.0..=end.0).map(Day).collect())
    }

    fn clock(&self) -> Clock {
        Clock {
            hour_min: 1,
            hour_max: 12,
            minute_max: 59,
            second_max: 59,
            meridiem: Some((SharedString::from("AM"), SharedString::from("PM"))),
        }
    }

    fn format_time(&self, time: TimeOfDay) -> SharedString {
        let meridiem = time
            .meridiem
            .and_then(|index| self.clock().meridiem_label(index))
            .unwrap_or_default();
        SharedString::from(
            format!("{}:{:02} {meridiem}", time.hour, time.minute)
                .trim_end()
                .to_string(),
        )
    }
}

pub fn to_naive_date(day: Day) -> Option<NaiveDate> {
    ChronoDates::to_date(day)
}

pub fn from_naive_date(date: NaiveDate) -> Day {
    ChronoDates::from_date(date)
}
