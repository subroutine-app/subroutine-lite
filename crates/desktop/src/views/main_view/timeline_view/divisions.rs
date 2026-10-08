use crate::color::ColorExt;
use chrono::{
    DateTime, Datelike, Days, Duration as ChronoDuration, Local, LocalResult, NaiveDate,
    NaiveDateTime, Offset, TimeZone, Timelike,
};
use gpui::{App, Div, ParentElement, Styled, div, prelude::FluentBuilder};
use gpui_kit::foundation::StyledExt as _;
use gpui_kit_theme::ActiveTheme;

use super::TimelineView;

pub enum TimeDivisionStyle {
    TickOnly,
    HourMinute,
    Hour,
    TimeOfDay,
    WeekDayDate,
    DateWeekStart,
    MonthName,
    YearQuarter,
    Year,
}

impl TimeDivisionStyle {
    pub(super) fn label(&self, datetime: DateTime<Local>, minimal: bool, cx: &App) -> Div {
        match self {
            TimeDivisionStyle::TickOnly => div(),
            TimeDivisionStyle::HourMinute => match datetime.minute() {
                0 => hour_label(datetime, cx),
                _ => hour_minute_label(datetime, minimal, false, cx).text_xs(),
            },
            TimeDivisionStyle::Hour => hour_label(datetime, cx),
            TimeDivisionStyle::TimeOfDay => time_of_day_label(datetime, cx),
            TimeDivisionStyle::WeekDayDate => weekday_date_label(datetime, false, cx),
            TimeDivisionStyle::DateWeekStart => week_start_label(datetime, cx),
            TimeDivisionStyle::MonthName => month_label(datetime, cx),
            TimeDivisionStyle::YearQuarter => year_quarter_label(datetime, cx),
            TimeDivisionStyle::Year => year_label(datetime, cx),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimeSubDivision {
    Second,
    FiveSeconds,
    FifteenSeconds,
    HalfMinute,
    Minute,
    FiveMinutes,
    TenMinutes,
    QuarterHour,
    HalfHour,
    Hour,
    EvenHour,
    FourHours,
    QuarterDay,
    HalfDay,
    Day,
    Week,
    Month,
    QuarterYear,
}

fn resolve_local_floor<Tz: TimeZone>(
    time: &DateTime<Tz>,
    mut naive: NaiveDateTime,
    retry_step: ChronoDuration,
) -> DateTime<Tz> {
    let timezone = time.timezone();
    loop {
        match timezone.from_local_datetime(&naive) {
            LocalResult::Single(boundary) => return boundary,
            LocalResult::Ambiguous(first, second) => {
                let input_offset = time.offset().fix();
                if first.offset().fix() == input_offset {
                    return first;
                }
                if second.offset().fix() == input_offset {
                    return second;
                }
                return first;
            }
            LocalResult::None => naive -= retry_step,
        }
    }
}

fn resolve_calendar_boundary<Tz: TimeZone>(
    timezone: &Tz,
    mut naive: NaiveDateTime,
) -> DateTime<Tz> {
    loop {
        match timezone.from_local_datetime(&naive) {
            LocalResult::Single(boundary) | LocalResult::Ambiguous(boundary, _) => return boundary,
            LocalResult::None => naive += ChronoDuration::minutes(1),
        }
    }
}

fn next_wall_boundary<Tz: TimeZone>(start: &DateTime<Tz>, step: ChronoDuration) -> DateTime<Tz> {
    let timezone = start.timezone();
    if let LocalResult::Ambiguous(first, second) =
        timezone.from_local_datetime(&start.naive_local())
        && first == *start
    {
        return second;
    }

    let mut naive = start.naive_local() + step;
    loop {
        match timezone.from_local_datetime(&naive) {
            LocalResult::Single(boundary) | LocalResult::Ambiguous(boundary, _) => return boundary,
            LocalResult::None => naive += step,
        }
    }
}

impl TimeSubDivision {
    const ALL: [Self; 18] = [
        Self::Second,
        Self::FiveSeconds,
        Self::FifteenSeconds,
        Self::HalfMinute,
        Self::Minute,
        Self::FiveMinutes,
        Self::TenMinutes,
        Self::QuarterHour,
        Self::HalfHour,
        Self::Hour,
        Self::EvenHour,
        Self::FourHours,
        Self::QuarterDay,
        Self::HalfDay,
        Self::Day,
        Self::Week,
        Self::Month,
        Self::QuarterYear,
    ];

    pub(super) fn style(&self) -> TimeDivisionStyle {
        match self {
            TimeSubDivision::Second => TimeDivisionStyle::TickOnly,
            TimeSubDivision::FiveSeconds => TimeDivisionStyle::TickOnly,
            TimeSubDivision::FifteenSeconds => TimeDivisionStyle::TickOnly,
            TimeSubDivision::HalfMinute => TimeDivisionStyle::TickOnly,
            TimeSubDivision::Minute => TimeDivisionStyle::HourMinute,
            TimeSubDivision::FiveMinutes => TimeDivisionStyle::HourMinute,
            TimeSubDivision::TenMinutes => TimeDivisionStyle::HourMinute,
            TimeSubDivision::QuarterHour => TimeDivisionStyle::HourMinute,
            TimeSubDivision::HalfHour => TimeDivisionStyle::HourMinute,
            TimeSubDivision::Hour => TimeDivisionStyle::Hour,
            TimeSubDivision::EvenHour => TimeDivisionStyle::Hour,
            TimeSubDivision::FourHours => TimeDivisionStyle::Hour,
            TimeSubDivision::QuarterDay => TimeDivisionStyle::TimeOfDay,
            TimeSubDivision::HalfDay => TimeDivisionStyle::TimeOfDay,
            TimeSubDivision::Day => TimeDivisionStyle::WeekDayDate,
            TimeSubDivision::Week => TimeDivisionStyle::DateWeekStart,
            TimeSubDivision::Month => TimeDivisionStyle::MonthName,
            TimeSubDivision::QuarterYear => TimeDivisionStyle::YearQuarter,
        }
    }

    pub(super) fn duration(&self) -> ChronoDuration {
        match self {
            TimeSubDivision::Second => ChronoDuration::seconds(1),
            TimeSubDivision::FiveSeconds => ChronoDuration::seconds(5),
            TimeSubDivision::FifteenSeconds => ChronoDuration::seconds(15),
            TimeSubDivision::HalfMinute => ChronoDuration::seconds(30),
            TimeSubDivision::Minute => ChronoDuration::minutes(1),
            TimeSubDivision::FiveMinutes => ChronoDuration::minutes(5),
            TimeSubDivision::TenMinutes => ChronoDuration::minutes(10),
            TimeSubDivision::QuarterHour => ChronoDuration::minutes(15),
            TimeSubDivision::HalfHour => ChronoDuration::minutes(30),
            TimeSubDivision::Hour => ChronoDuration::hours(1),
            TimeSubDivision::EvenHour => ChronoDuration::hours(2),
            TimeSubDivision::FourHours => ChronoDuration::hours(4),
            TimeSubDivision::QuarterDay => ChronoDuration::hours(6),
            TimeSubDivision::HalfDay => ChronoDuration::hours(12),
            TimeSubDivision::Day => ChronoDuration::days(1),
            TimeSubDivision::Week => ChronoDuration::weeks(1),
            TimeSubDivision::Month => ChronoDuration::weeks(4),
            TimeSubDivision::QuarterYear => ChronoDuration::weeks(13),
        }
    }

    pub(super) fn next_boundary<Tz: TimeZone>(&self, start: DateTime<Tz>) -> DateTime<Tz> {
        let floor = self.floor_boundary(start);
        match self {
            TimeSubDivision::Second => floor + ChronoDuration::seconds(1),
            TimeSubDivision::FiveSeconds => floor + ChronoDuration::seconds(5),
            TimeSubDivision::FifteenSeconds => floor + ChronoDuration::seconds(15),
            TimeSubDivision::HalfMinute => floor + ChronoDuration::seconds(30),
            TimeSubDivision::Minute => floor + ChronoDuration::minutes(1),
            TimeSubDivision::FiveMinutes => floor + ChronoDuration::minutes(5),
            TimeSubDivision::TenMinutes => floor + ChronoDuration::minutes(10),
            TimeSubDivision::QuarterHour => floor + ChronoDuration::minutes(15),
            TimeSubDivision::HalfHour => floor + ChronoDuration::minutes(30),
            TimeSubDivision::Hour => floor + ChronoDuration::hours(1),
            TimeSubDivision::EvenHour => next_wall_boundary(&floor, ChronoDuration::hours(2)),
            TimeSubDivision::FourHours => next_wall_boundary(&floor, ChronoDuration::hours(4)),
            TimeSubDivision::QuarterDay => next_wall_boundary(&floor, ChronoDuration::hours(6)),
            TimeSubDivision::HalfDay => next_wall_boundary(&floor, ChronoDuration::hours(12)),
            TimeSubDivision::Day => BaseTimeDivision::Day.next_boundary(floor),
            TimeSubDivision::Week => {
                let naive = (floor.date_naive() + Days::new(7))
                    .and_hms_opt(0, 0, 0)
                    .unwrap();
                resolve_calendar_boundary(&floor.timezone(), naive)
            }
            TimeSubDivision::Month => BaseTimeDivision::Month.next_boundary(floor),
            TimeSubDivision::QuarterYear => {
                let month = floor.month();
                let next_quarter_month = if month <= 3 {
                    4
                } else if month <= 6 {
                    7
                } else if month <= 9 {
                    10
                } else {
                    1
                };
                let (next_year, next_month) = if next_quarter_month == 1 {
                    (floor.year() + 1, 1)
                } else {
                    (floor.year(), next_quarter_month)
                };
                let naive = NaiveDate::from_ymd_opt(next_year, next_month, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap();
                resolve_calendar_boundary(&floor.timezone(), naive)
            }
        }
    }

    pub(super) fn floor_boundary<Tz: TimeZone>(&self, time: DateTime<Tz>) -> DateTime<Tz> {
        let same_day_boundary = |hour, minute, second, retry_step| {
            let naive = time.date_naive().and_hms_opt(hour, minute, second).unwrap();
            resolve_local_floor(&time, naive, retry_step)
        };

        match self {
            TimeSubDivision::Second => same_day_boundary(
                time.hour(),
                time.minute(),
                time.second(),
                ChronoDuration::seconds(1),
            ),
            TimeSubDivision::FiveSeconds => same_day_boundary(
                time.hour(),
                time.minute(),
                (time.second() / 5) * 5,
                ChronoDuration::seconds(5),
            ),
            TimeSubDivision::FifteenSeconds => same_day_boundary(
                time.hour(),
                time.minute(),
                (time.second() / 15) * 15,
                ChronoDuration::seconds(15),
            ),
            TimeSubDivision::HalfMinute => same_day_boundary(
                time.hour(),
                time.minute(),
                (time.second() / 30) * 30,
                ChronoDuration::seconds(30),
            ),
            TimeSubDivision::Minute => {
                same_day_boundary(time.hour(), time.minute(), 0, ChronoDuration::minutes(1))
            }
            TimeSubDivision::FiveMinutes => same_day_boundary(
                time.hour(),
                (time.minute() / 5) * 5,
                0,
                ChronoDuration::minutes(5),
            ),
            TimeSubDivision::TenMinutes => same_day_boundary(
                time.hour(),
                (time.minute() / 10) * 10,
                0,
                ChronoDuration::minutes(10),
            ),
            TimeSubDivision::QuarterHour => same_day_boundary(
                time.hour(),
                (time.minute() / 15) * 15,
                0,
                ChronoDuration::minutes(15),
            ),
            TimeSubDivision::HalfHour => same_day_boundary(
                time.hour(),
                (time.minute() / 30) * 30,
                0,
                ChronoDuration::minutes(30),
            ),
            TimeSubDivision::Hour => BaseTimeDivision::Hour.floor_boundary(time),
            TimeSubDivision::EvenHour => {
                same_day_boundary((time.hour() / 2) * 2, 0, 0, ChronoDuration::hours(2))
            }
            TimeSubDivision::FourHours => {
                same_day_boundary((time.hour() / 4) * 4, 0, 0, ChronoDuration::hours(4))
            }
            TimeSubDivision::QuarterDay => {
                same_day_boundary((time.hour() / 6) * 6, 0, 0, ChronoDuration::hours(6))
            }
            TimeSubDivision::HalfDay => same_day_boundary(
                if time.hour() < 12 { 0 } else { 12 },
                0,
                0,
                ChronoDuration::hours(12),
            ),
            TimeSubDivision::Day => BaseTimeDivision::Day.floor_boundary(time),
            TimeSubDivision::Week => {
                let days_since_monday = time.weekday().num_days_from_monday() as u64;
                let naive = (time.date_naive() - Days::new(days_since_monday))
                    .and_hms_opt(0, 0, 0)
                    .unwrap();
                resolve_calendar_boundary(&time.timezone(), naive)
            }
            TimeSubDivision::Month => BaseTimeDivision::Month.floor_boundary(time),
            TimeSubDivision::QuarterYear => {
                let month = time.month();
                let quarter_start_month = if month <= 3 {
                    1
                } else if month <= 6 {
                    4
                } else if month <= 9 {
                    7
                } else {
                    10
                };
                let naive = NaiveDate::from_ymd_opt(time.year(), quarter_start_month, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap();
                resolve_calendar_boundary(&time.timezone(), naive)
            }
        }
    }

    pub(super) fn ceil_boundary<Tz: TimeZone>(&self, time: DateTime<Tz>) -> DateTime<Tz> {
        let floor = self.floor_boundary(time.clone());
        if floor == time {
            time
        } else {
            self.next_boundary(floor)
        }
    }

    pub(super) fn nearest_boundary<Tz: TimeZone>(&self, time: DateTime<Tz>) -> DateTime<Tz> {
        let floor = self.floor_boundary(time.clone());
        let ceil = self.ceil_boundary(time.clone());
        if ceil == floor {
            return floor;
        }
        let distance_to_floor = time.clone() - floor.clone();
        let distance_to_ceil = ceil.clone() - time;
        if distance_to_floor <= distance_to_ceil {
            floor
        } else {
            ceil
        }
    }

    pub(super) fn exact_duration<Tz: TimeZone>(
        &self,
        division_start: DateTime<Tz>,
    ) -> ChronoDuration {
        self.next_boundary(division_start.clone()) - division_start
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BaseTimeDivision {
    Minute,
    FiveMinutes,
    Hour,
    Day,
    Month,
    Year,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct OuterPeriodLabel {
    pub(super) start: DateTime<Local>,
    pub(super) end: DateTime<Local>,
    pub(super) text: String,
}

impl BaseTimeDivision {
    const ALL: [Self; 6] = [
        Self::Minute,
        Self::FiveMinutes,
        Self::Hour,
        Self::Day,
        Self::Month,
        Self::Year,
    ];

    fn containing(subdivision: TimeSubDivision) -> Self {
        Self::ALL
            .into_iter()
            .find(|base| base.approximate_duration() > subdivision.duration())
            .unwrap_or(Self::Year)
    }

    pub(super) fn short_label(&self) -> &'static str {
        match self {
            Self::Minute => "1m",
            Self::FiveMinutes => "5m",
            Self::Hour => "1h",
            Self::Day => "1d",
            Self::Month => "1mo",
            Self::Year => "1y",
        }
    }
    pub(super) fn exact_duration<Tz: TimeZone>(
        &self,
        division_start: DateTime<Tz>,
    ) -> ChronoDuration {
        match self {
            BaseTimeDivision::Minute => ChronoDuration::minutes(1),
            BaseTimeDivision::FiveMinutes => ChronoDuration::minutes(5),
            BaseTimeDivision::Hour => ChronoDuration::hours(1),
            BaseTimeDivision::Day | BaseTimeDivision::Month | BaseTimeDivision::Year => {
                self.next_boundary(division_start.clone()) - division_start
            }
        }
    }

    pub(super) fn next_boundary<Tz: TimeZone>(&self, datetime: DateTime<Tz>) -> DateTime<Tz> {
        let start = self.floor_boundary(datetime);
        match self {
            BaseTimeDivision::Minute | BaseTimeDivision::FiveMinutes | BaseTimeDivision::Hour => {
                start.clone() + self.exact_duration(start)
            }
            BaseTimeDivision::Day => {
                let next = start.date_naive() + Days::new(1);
                let naive = next.and_hms_opt(0, 0, 0).unwrap();
                resolve_calendar_boundary(&start.timezone(), naive)
            }
            BaseTimeDivision::Month => {
                let (year, month) = if start.month() == 12 {
                    (start.year() + 1, 1)
                } else {
                    (start.year(), start.month() + 1)
                };
                let naive = NaiveDate::from_ymd_opt(year, month, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap();
                resolve_calendar_boundary(&start.timezone(), naive)
            }
            BaseTimeDivision::Year => {
                let naive = NaiveDate::from_ymd_opt(start.year() + 1, 1, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap();
                resolve_calendar_boundary(&start.timezone(), naive)
            }
        }
    }

    pub(super) fn floor_boundary<Tz: TimeZone>(&self, time: DateTime<Tz>) -> DateTime<Tz> {
        let local_midnight = |date: NaiveDate| {
            let naive = date.and_hms_opt(0, 0, 0).unwrap();
            resolve_calendar_boundary(&time.timezone(), naive)
        };

        match self {
            BaseTimeDivision::Minute => {
                let naive = time
                    .date_naive()
                    .and_hms_opt(time.hour(), time.minute(), 0)
                    .unwrap();
                resolve_local_floor(&time, naive, ChronoDuration::minutes(1))
            }
            BaseTimeDivision::FiveMinutes => {
                let naive = time
                    .date_naive()
                    .and_hms_opt(time.hour(), (time.minute() / 5) * 5, 0)
                    .unwrap();
                resolve_local_floor(&time, naive, ChronoDuration::minutes(5))
            }
            BaseTimeDivision::Hour => {
                let naive = time.date_naive().and_hms_opt(time.hour(), 0, 0).unwrap();
                resolve_local_floor(&time, naive, ChronoDuration::hours(1))
            }
            BaseTimeDivision::Day => local_midnight(time.date_naive()),
            BaseTimeDivision::Month => {
                local_midnight(NaiveDate::from_ymd_opt(time.year(), time.month(), 1).unwrap())
            }
            BaseTimeDivision::Year => {
                local_midnight(NaiveDate::from_ymd_opt(time.year(), 1, 1).unwrap())
            }
        }
    }

    pub(super) fn ceil_boundary<Tz: TimeZone>(&self, time: DateTime<Tz>) -> DateTime<Tz> {
        let floor = self.floor_boundary(time.clone());
        if floor == time {
            time
        } else {
            self.next_boundary(floor)
        }
    }

    pub(super) fn nearest_boundary<Tz: TimeZone>(&self, time: DateTime<Tz>) -> DateTime<Tz> {
        let floor = self.floor_boundary(time.clone());
        let next = self.ceil_boundary(time.clone());
        let distance_to_floor = time.clone() - floor.clone();
        let distance_to_next = next.clone() - time;
        if distance_to_floor <= distance_to_next {
            floor
        } else {
            next
        }
    }

    pub(super) fn approximate_duration(&self) -> ChronoDuration {
        match self {
            BaseTimeDivision::Minute => ChronoDuration::minutes(1),
            BaseTimeDivision::FiveMinutes => ChronoDuration::minutes(5),
            BaseTimeDivision::Hour => ChronoDuration::hours(1),
            BaseTimeDivision::Day => ChronoDuration::days(1),
            BaseTimeDivision::Month => ChronoDuration::weeks(4),
            BaseTimeDivision::Year => ChronoDuration::weeks(52),
        }
    }

    pub(super) fn base_label_style(&self) -> TimeDivisionStyle {
        match self {
            BaseTimeDivision::Minute => TimeDivisionStyle::HourMinute,
            BaseTimeDivision::FiveMinutes => TimeDivisionStyle::HourMinute,
            BaseTimeDivision::Hour => TimeDivisionStyle::Hour,
            BaseTimeDivision::Day => TimeDivisionStyle::WeekDayDate,
            BaseTimeDivision::Month => TimeDivisionStyle::MonthName,
            BaseTimeDivision::Year => TimeDivisionStyle::Year,
        }
    }

    fn outer_label_text(&self, time: DateTime<Local>) -> Option<String> {
        match self {
            BaseTimeDivision::Minute => Some(time.format("%-I %p").to_string()),
            BaseTimeDivision::FiveMinutes => Some(time.format("%-I %p").to_string()),
            BaseTimeDivision::Hour => Some(time.format("%a %-d").to_string()),
            BaseTimeDivision::Day => Some(time.format("%B").to_string()),
            BaseTimeDivision::Month => Some(time.format("%Y").to_string()),
            BaseTimeDivision::Year => None,
        }
    }

    pub(super) fn outer_period_label_at(&self, time: DateTime<Local>) -> Option<OuterPeriodLabel> {
        let outer = self.outer_division()?;
        let start = outer.floor_boundary(time);
        Some(OuterPeriodLabel {
            start,
            end: outer.next_boundary(start),
            text: self.outer_label_text(start)?,
        })
    }

    pub(super) fn outer_division(&self) -> Option<BaseTimeDivision> {
        match self {
            BaseTimeDivision::Minute => Some(BaseTimeDivision::Hour),
            BaseTimeDivision::FiveMinutes => Some(BaseTimeDivision::Hour),
            BaseTimeDivision::Hour => Some(BaseTimeDivision::Day),
            BaseTimeDivision::Day => Some(BaseTimeDivision::Month),
            BaseTimeDivision::Month => Some(BaseTimeDivision::Year),
            BaseTimeDivision::Year => None,
        }
    }
}

const TARGET_SUBDIVISION_HEIGHT_PX: f32 = 64.0;

fn relative_duration_error(duration: ChronoDuration, target_seconds: f32) -> f32 {
    (duration.as_seconds_f32() / target_seconds).ln().abs()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimeDivisionState {
    pub base_division: BaseTimeDivision,
    pub subdivision: Option<TimeSubDivision>,
}

impl TimeDivisionState {
    fn for_pixel_duration(pixel_duration: ChronoDuration) -> Self {
        let target_duration_seconds =
            pixel_duration.as_seconds_f32() * TARGET_SUBDIVISION_HEIGHT_PX;
        debug_assert!(target_duration_seconds.is_finite() && target_duration_seconds > 0.0);

        let subdivision = TimeSubDivision::ALL
            .into_iter()
            .min_by(|left, right| {
                relative_duration_error(left.duration(), target_duration_seconds).total_cmp(
                    &relative_duration_error(right.duration(), target_duration_seconds),
                )
            })
            .unwrap();

        let subdivision_error =
            relative_duration_error(subdivision.duration(), target_duration_seconds);
        let year_error = relative_duration_error(
            BaseTimeDivision::Year.approximate_duration(),
            target_duration_seconds,
        );

        if year_error < subdivision_error {
            Self {
                base_division: BaseTimeDivision::Year,
                subdivision: None,
            }
        } else {
            Self {
                base_division: BaseTimeDivision::containing(subdivision),
                subdivision: Some(subdivision),
            }
        }
    }

    pub(super) fn current_subdivision(&self) -> Option<TimeSubDivision> {
        self.subdivision
    }

    pub(super) fn auto_floor_boundary(&self, time: DateTime<Local>) -> DateTime<Local> {
        self.current_subdivision()
            .map(|s| s.floor_boundary(time))
            .unwrap_or(self.base_division.floor_boundary(time))
    }

    pub(super) fn auto_boundary_with_floor_fraction(
        &self,
        time: DateTime<Local>,
        floor_fraction: f64,
    ) -> DateTime<Local> {
        debug_assert!(floor_fraction > 0.0 && floor_fraction < 1.0);
        let floor = self.auto_floor_boundary(time);
        let next = self.auto_next_boundary(floor);
        let interval_ns = (next - floor)
            .num_nanoseconds()
            .expect("a timeline division must fit in chrono nanoseconds");
        let floor_region =
            ChronoDuration::nanoseconds((interval_ns as f64 * floor_fraction).round() as i64);
        if time < floor + floor_region {
            floor
        } else {
            next
        }
    }

    pub(super) fn auto_next_boundary(&self, time: DateTime<Local>) -> DateTime<Local> {
        self.current_subdivision()
            .map(|s| s.next_boundary(time))
            .unwrap_or_else(|| self.base_division.next_boundary(time))
    }

    pub(super) fn auto_previous_boundary(&self, time: DateTime<Local>) -> DateTime<Local> {
        self.auto_floor_boundary(time - ChronoDuration::nanoseconds(1))
    }

    pub(super) fn division_duration(&self) -> ChronoDuration {
        self.current_subdivision()
            .map(|s| s.duration())
            .unwrap_or(self.base_division.approximate_duration())
    }
}

fn hour_minute_label(datetime: DateTime<Local>, muted: bool, pm: bool, cx: &App) -> Div {
    let format = if pm { "%-I:%M %p" } else { "%-I:%M" };
    let str = datetime.format(format).to_string();
    div()
        .child(str)
        .text_sm()
        .when(muted, |this| this.text_color(cx.theme().colors.text_muted))
}

fn hour_label(datetime: DateTime<Local>, cx: &App) -> Div {
    let hour = datetime.hour();
    let primary = match hour {
        0 => "12".to_string(),
        12 => "Noon".to_string(),
        _ if hour < 13 => format!("{}", hour),
        _ => format!("{}", hour - 12),
    };

    let muted = cx.theme().colors.text_muted;
    let primary_color = cx.theme().colors.text.mix(muted, 0.5);

    let secondary = match hour {
        12 => None,
        _ if hour < 13 => Some("AM"),
        _ => Some("PM"),
    };

    div()
        .row()
        .gap_0p5()
        .items_end()
        .child(div().child(primary).text_sm().text_color(primary_color))
        .when_some(secondary, |this, str| {
            this.child(div().child(str).text_xs().text_color(muted))
        })
}

fn time_of_day_label(datetime: DateTime<Local>, cx: &App) -> Div {
    let hour = datetime.hour();
    let label = match hour {
        0 => "Midnight",
        12 => "Noon",
        _ if hour < 12 => "Morning",
        _ if hour < 18 => "Afternoon",
        _ => "Evening",
    };
    div()
        .child(label)
        .text_sm()
        .text_color(cx.theme().colors.text_muted)
}

fn weekday_date_label(datetime: DateTime<Local>, minimal: bool, cx: &App) -> Div {
    let format = match minimal {
        true => "%a",
        false => "%a %e",
    };
    let str = datetime.format(format).to_string();
    div()
        .child(str)
        .text_sm()
        .text_color(cx.theme().colors.text_muted)
}

fn week_start_label(datetime: DateTime<Local>, cx: &App) -> Div {
    let week = datetime.iso_week().week();
    let str = format!("W{}", week);
    div()
        .child(str)
        .text_sm()
        .text_color(cx.theme().colors.text_muted)
}

fn month_label_text(datetime: DateTime<Local>) -> String {
    datetime.format("%b").to_string()
}

fn month_label(datetime: DateTime<Local>, cx: &App) -> Div {
    div()
        .child(month_label_text(datetime))
        .text_sm()
        .text_color(cx.theme().colors.text_muted)
}

fn year_quarter_label(datetime: DateTime<Local>, cx: &App) -> Div {
    let month = datetime.month();
    let quarter = match month {
        1..=3 => "Q1",
        4..=6 => "Q2",
        7..=9 => "Q3",
        _ => "Q4",
    };
    div()
        .child(quarter)
        .text_sm()
        .text_color(cx.theme().colors.text_muted)
}

fn year_label(datetime: DateTime<Local>, cx: &App) -> Div {
    let str = datetime.format("%Y").to_string();
    div()
        .child(str)
        .text_sm()
        .text_color(cx.theme().colors.text_muted)
}

impl TimelineView {
    pub(super) fn current_division_state(&self) -> TimeDivisionState {
        TimeDivisionState::for_pixel_duration(self.pixel_duration)
    }

    pub(super) fn sub_divisions(
        &self,
        base_floor: DateTime<Local>,
    ) -> Option<Vec<DateTime<Local>>> {
        let division_state = self.current_division_state();
        division_state.current_subdivision().map(|s| {
            let mut current = s.next_boundary(base_floor);
            let end = division_state.base_division.next_boundary(base_floor);
            let mut sub_divisions = vec![];
            const MAX_ITER: usize = 200;
            while current < end && sub_divisions.len() < MAX_ITER {
                sub_divisions.push(current);
                current = s.next_boundary(current);
            }
            if sub_divisions.len() >= MAX_ITER {
                eprintln!(
                    "sub_divisions loop hit safety limit ({} iterations). base_floor={:?}, end={:?}, sub={:?}, pixel_duration={:?}",
                    MAX_ITER,
                    base_floor,
                    end,
                    s,
                    self.pixel_duration,
                );
            }
            sub_divisions

        })
    }
}
