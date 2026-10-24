use std::ops::Range;

use chrono::{
    Datelike, Days, Local, Months, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, Timelike,
};
use gpui::{
    App, Context, Div, ElementId, Entity, EventEmitter, FocusHandle, InteractiveElement,
    IntoElement, ParentElement, Pixels, RenderOnce, SharedString, Stateful,
    StatefulInteractiveElement, StyleRefinement, Styled, Window, div, prelude::FluentBuilder as _,
    px, relative,
};

use crate::AppIcon;
use crate::components::Button;
use crate::components::ButtonVariants as _;
use crate::components::ext::StyledRefineExt as _;
use crate::presentation::UxColor;
use gpui_kit::foundation::{Disableable as _, FocusRing as _, Sizable, StyledExt as _};
use gpui_kit::overlay::{GlassExt as _, GlassPreset};
use gpui_kit_theme::{ActiveTheme, ControlSize, Radius, Surface};

const YEARS_PER_PAGE: usize = 20;
const YEAR_SPAN: i32 = 50;
const TABS: [TimeUnit; 4] = [
    TimeUnit::Month,
    TimeUnit::Day,
    TimeUnit::Year,
    TimeUnit::Hour,
];
const WEEKDAYS: [&str; 7] = ["S", "M", "T", "W", "T", "F", "S"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TimeUnit {
    Hour,
    Day,
    Month,
    Year,
}

impl TimeUnit {
    fn finer(self) -> Option<Self> {
        match self {
            Self::Hour => None,
            Self::Day => Some(Self::Hour),
            Self::Month => Some(Self::Day),
            Self::Year => Some(Self::Month),
        }
    }

    fn floor(self, time: NaiveDateTime) -> NaiveDateTime {
        let midnight = time.date().and_time(NaiveTime::MIN);
        match self {
            Self::Hour => midnight + TimeDelta::hours(time.hour().into()),
            Self::Day => midnight,
            Self::Month => midnight - Days::new(time.day0().into()),
            Self::Year => midnight - Days::new(time.ordinal0().into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimePeriod {
    unit: TimeUnit,
    start: NaiveDateTime,
}

impl TimePeriod {
    pub fn containing(unit: TimeUnit, time: NaiveDateTime) -> Self {
        Self {
            unit,
            start: unit.floor(time),
        }
    }

    pub fn unit(self) -> TimeUnit {
        self.unit
    }

    pub fn start(self) -> NaiveDateTime {
        self.start
    }

    pub fn end(self) -> NaiveDateTime {
        match self.unit {
            TimeUnit::Hour => self.start + TimeDelta::hours(1),
            TimeUnit::Day => self.start + Days::new(1),
            TimeUnit::Month => self.start + Months::new(1),
            TimeUnit::Year => self.start + Months::new(12),
        }
    }

    fn contains(self, time: NaiveDateTime) -> bool {
        (self.start..self.end()).contains(&time)
    }

    fn locate(self, time: NaiveDateTime) -> NaiveDateTime {
        let date = match self.unit {
            TimeUnit::Year => {
                day_of_month(self.start.date() + Months::new(time.month0()), time.day())
            }
            TimeUnit::Month => day_of_month(self.start.date(), time.day()),
            TimeUnit::Day | TimeUnit::Hour => self.start.date(),
        };
        let hour = match self.unit {
            TimeUnit::Hour => self.start.hour(),
            TimeUnit::Day | TimeUnit::Month | TimeUnit::Year => time.hour(),
        };
        date.and_time(NaiveTime::MIN) + TimeDelta::hours(hour.into())
    }
}

fn day_of_month(first: NaiveDate, day: u32) -> NaiveDate {
    first + Days::new((day.min(first.num_days_in_month().into()) - 1).into())
}

pub enum TimeSelectEvent {
    Selected(TimePeriod),
    Now,
}

pub struct TimeSelectState {
    focus_handle: FocusHandle,
    finest: TimeUnit,
    mode: TimeUnit,
    viewed: NaiveDateTime,
    browsed: NaiveDateTime,
    years: Range<i32>,
    year_page: usize,
}

impl EventEmitter<TimeSelectEvent> for TimeSelectState {}

impl TimeSelectState {
    pub fn new(finest: TimeUnit, cx: &mut Context<Self>) -> Self {
        let now = Local::now().naive_local();
        let viewed = finest.floor(now);
        let mut state = Self {
            focus_handle: cx.focus_handle(),
            finest,
            mode: finest,
            viewed,
            browsed: viewed,
            years: now.year() - YEAR_SPAN..now.year() + YEAR_SPAN,
            year_page: 0,
        };
        state.browse(viewed);
        state
    }

    pub fn show(&mut self, unit: TimeUnit, viewed: NaiveDateTime, cx: &mut Context<Self>) {
        self.mode = unit.max(self.finest);
        self.viewed = self.finest.floor(viewed);
        self.browse(self.viewed);
        cx.notify();
    }

    pub fn set_viewed(&mut self, time: NaiveDateTime, cx: &mut Context<Self>) {
        let viewed = self.finest.floor(time);
        if viewed != self.viewed {
            self.viewed = viewed;
            self.browse(viewed);
            cx.notify();
        }
    }

    fn browse(&mut self, time: NaiveDateTime) {
        self.browsed = time;
        self.reveal_year(time.year());
    }

    fn reveal_year(&mut self, year: i32) {
        if self.years.contains(&year) {
            self.year_page = (year - self.years.start) as usize / YEARS_PER_PAGE;
        }
    }

    fn set_mode(&mut self, unit: TimeUnit, cx: &mut Context<Self>) {
        self.mode = unit;
        self.reveal_year(self.browsed.year());
        cx.notify();
    }

    fn select(&mut self, period: TimePeriod, cx: &mut Context<Self>) {
        self.browse(period.locate(self.browsed));
        if let Some(finer) = period.unit.finer().filter(|unit| *unit >= self.finest) {
            self.mode = finer;
        }
        cx.emit(TimeSelectEvent::Selected(period));
        cx.notify();
    }

    fn page_years(&self) -> Range<i32> {
        let start = self.years.start + (self.year_page * YEARS_PER_PAGE) as i32;
        start..(start + YEARS_PER_PAGE as i32).min(self.years.end)
    }

    fn turned_year_page(&self, step: isize) -> Option<usize> {
        self.year_page
            .checked_add_signed(step)
            .filter(|page| *page < self.years.len().div_ceil(YEARS_PER_PAGE))
    }

    fn turn_year_page(&mut self, step: isize, cx: &mut Context<Self>) {
        if let Some(page) = self.turned_year_page(step) {
            self.year_page = page;
            cx.notify();
        }
    }

    fn tab_label(&self, unit: TimeUnit) -> SharedString {
        let format = match unit {
            TimeUnit::Hour => "%-I %p",
            TimeUnit::Day => "%-d",
            TimeUnit::Month if self.finest <= TimeUnit::Day => "%b",
            TimeUnit::Month => "%B",
            TimeUnit::Year => "%Y",
        };
        self.browsed.format(format).to_string().into()
    }
}

#[derive(IntoElement)]
pub struct TimeSelect {
    size: ControlSize,
    state: Entity<TimeSelectState>,
    style: StyleRefinement,
}

impl TimeSelect {
    pub fn new(state: &Entity<TimeSelectState>) -> Self {
        Self {
            size: ControlSize::default(),
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }

    pub fn popover(self) -> Stateful<Div> {
        let id = self.state.entity_id();
        div()
            .id(("time-select-popover", id))
            .min_h_0()
            .overflow_scroll()
            .shadow_lg()
            .child(
                div()
                    .id(("time-select-glass", id))
                    .bg_glass()
                    .glass_surface(Surface::Overlay)
                    .glass_radius(Radius::Dialog)
                    .glass(|glass| glass.protect_text_contrast(false))
                    .when(cfg!(not(target_os = "macos")), |frame| {
                        frame.glass_preset(GlassPreset::Frosted)
                    })
                    .child(self),
            )
    }

    fn width(&self, finest: TimeUnit) -> Pixels {
        let wide = finest <= TimeUnit::Day;
        px(match (self.size, wide) {
            (ControlSize::Sm, false) => 208.,
            (ControlSize::Sm, true) => 248.,
            (ControlSize::Lg, false) => 292.,
            (ControlSize::Lg, true) => 344.,
            (_, false) => 264.,
            (_, true) => 304.,
        })
    }

    fn sized<E: Styled>(&self, element: E) -> E {
        match self.size {
            ControlSize::Sm => element.size_7(),
            ControlSize::Lg => element.size_10(),
            _ => element.size_9(),
        }
    }

    fn page(&self, width: Pixels) -> Div {
        div().w(width).map(|this| match self.size {
            ControlSize::Sm => this.mt_2().gap_y_2(),
            ControlSize::Lg => this.mt_4().gap_y_4(),
            _ => this.mt_3().gap_y_3(),
        })
    }

    fn grid(&self, width: Pixels) -> Div {
        self.page(width)
            .row()
            .flex_wrap()
            .justify_between()
            .gap_x_0p5()
    }

    fn render_header(&self, window: &mut Window, cx: &App) -> impl IntoElement {
        let state = self.state.read(cx);
        let mode = state.mode;
        let paging = mode == TimeUnit::Year;
        let icon_size = match self.size {
            ControlSize::Sm => ControlSize::Sm,
            _ => ControlSize::Md,
        };
        let page_button = |id: &'static str, icon: AppIcon, step: isize| {
            Button::new(id)
                .icon(icon)
                .ghost()
                .control_size(icon_size)
                .disabled(state.turned_year_page(step).is_none())
                .on_click(window.listener_for(&self.state, move |state, _, _, cx| {
                    state.turn_year_page(step, cx)
                }))
        };
        let tabs: Vec<_> = TABS
            .into_iter()
            .filter(|unit| *unit >= state.finest)
            .collect();

        div()
            .row()
            .gap_0p5()
            .justify_between()
            .items_center()
            .when_else(
                paging,
                |this| this.child(page_button("prev", AppIcon::ArrowLeft, -1)),
                |this| this.child(div()),
            )
            .child(
                div()
                    .row()
                    .justify_center()
                    .map(|this| {
                        if tabs.len() > 2 {
                            this.gap_1()
                        } else {
                            this.gap_3()
                        }
                    })
                    .children(tabs.into_iter().map(|unit| {
                        Button::new(("time-select-unit", unit as usize))
                            .ghost()
                            .label(state.tab_label(unit))
                            .control_size(self.size)
                            .current(mode == unit)
                            .on_click(window.listener_for(&self.state, move |state, _, _, cx| {
                                state.set_mode(unit, cx)
                            }))
                    })),
            )
            .when_else(
                paging,
                |this| this.child(page_button("next", AppIcon::ArrowRight, 1)),
                |this| this.child(div()),
            )
    }

    fn item_button(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        active: bool,
        actual: bool,
        cx: &App,
    ) -> Stateful<Div> {
        let colors = &cx.theme().colors;
        self.sized(div().row().id(id.into()))
            .map(|this| match self.size {
                ControlSize::Sm => this.rounded(px(cx.theme().radii.control) / 2.),
                ControlSize::Lg => this.rounded(px(cx.theme().radii.control) * 2.),
                _ => this.rounded(px(cx.theme().radii.control)),
            })
            .justify_center()
            .text_color(colors.text)
            .when(actual, |this| this.bg(colors.selected))
            .when(!active, |this| {
                this.hover(|this| this.bg(colors.hover).text_color(colors.text))
            })
            .when(active, |this| {
                this.bg(colors.accent).text_color(colors.text_on_accent)
            })
            .role(gpui::Role::Button)
            .tab_index(0)
            .cursor_pointer()
            .focus_ring(cx.theme())
            .child(label.into())
    }

    fn period_button(
        &self,
        period: TimePeriod,
        label: impl Into<SharedString>,
        now: NaiveDateTime,
        window: &Window,
        cx: &App,
    ) -> Stateful<Div> {
        let viewed = self.state.read(cx).viewed;
        self.item_button(
            (
                "time-select-period",
                period.start.and_utc().timestamp() as u64,
            ),
            label,
            period.contains(viewed),
            period.contains(now),
            cx,
        )
        .on_click(window.listener_for(&self.state, move |state, _, _, cx| state.select(period, cx)))
    }

    fn render_years(&self, width: Pixels, now: NaiveDateTime, window: &Window, cx: &App) -> Div {
        self.grid(width)
            .children(self.state.read(cx).page_years().filter_map(|year| {
                let start = NaiveDate::from_ymd_opt(year, 1, 1)?.and_time(NaiveTime::MIN);
                let period = TimePeriod::containing(TimeUnit::Year, start);
                Some(
                    self.period_button(period, year.to_string(), now, window, cx)
                        .w(relative(0.2)),
                )
            }))
    }

    fn render_months(&self, width: Pixels, now: NaiveDateTime, window: &Window, cx: &App) -> Div {
        let year = TimeUnit::Year.floor(self.state.read(cx).browsed);
        self.grid(width).children((0..12).map(|month| {
            let period = TimePeriod::containing(TimeUnit::Month, year + Months::new(month));
            self.period_button(
                period,
                period.start.format("%B").to_string(),
                now,
                window,
                cx,
            )
            .w(relative(0.3))
            .text_sm()
        }))
    }

    fn render_days(&self, width: Pixels, now: NaiveDateTime, window: &Window, cx: &App) -> Div {
        let month = TimeUnit::Month.floor(self.state.read(cx).browsed).date();
        let lead = month.weekday().num_days_from_sunday() as usize;
        let days: Vec<_> = std::iter::repeat_n(None, lead)
            .chain(
                month
                    .iter_days()
                    .take(month.num_days_in_month().into())
                    .map(Some),
            )
            .collect();
        let weekdays = div().row().justify_between().children(WEEKDAYS.map(|day| {
            self.sized(div().row())
                .h_5()
                .justify_center()
                .text_xs()
                .text_color(cx.theme().colors.text_muted)
                .child(day)
        }));
        let weeks = days.chunks(7).map(|week| {
            div().row().justify_between().children((0..7).map(|ix| {
                match week.get(ix).copied().flatten() {
                    Some(date) => {
                        let period =
                            TimePeriod::containing(TimeUnit::Day, date.and_time(NaiveTime::MIN));
                        self.period_button(period, date.format("%-d").to_string(), now, window, cx)
                            .text_sm()
                            .into_any_element()
                    }
                    None => self.sized(div()).into_any_element(),
                }
            }))
        });

        self.page(width)
            .column()
            .gap_y_1()
            .child(weekdays)
            .children(weeks)
    }

    fn render_hours(&self, width: Pixels, now: NaiveDateTime, window: &Window, cx: &App) -> Div {
        let day = TimeUnit::Day.floor(self.state.read(cx).browsed);
        self.grid(width).gap_y_1().children((0..24).map(|hour| {
            let period = TimePeriod::containing(TimeUnit::Hour, day + TimeDelta::hours(hour));
            self.period_button(
                period,
                period.start.format("%-I %p").to_string(),
                now,
                window,
                cx,
            )
            .w(relative(0.24))
            .text_sm()
        }))
    }

    fn render_now(&self, now: NaiveDateTime, window: &Window, cx: &App) -> impl IntoElement {
        let (icon, label) = match self.state.read(cx).finest {
            TimeUnit::Hour => (
                AppIcon::Clock,
                format!("Now · {}", now.format("%b %-d, %Y · %-I:%M %p")),
            ),
            TimeUnit::Day | TimeUnit::Month | TimeUnit::Year => (
                AppIcon::Calendar,
                format!("Today · {}", now.format("%b %-d, %Y")),
            ),
        };
        div()
            .mt_3()
            .pt_3()
            .border_t_1()
            .border_color(cx.theme().colors.hairline)
            .child(
                Button::new("time-select-now")
                    .ghost()
                    .w_full()
                    .justify_start()
                    .icon(icon)
                    .label(label)
                    .text_color(UxColor::CurrentTime.text(cx.theme()))
                    .on_click(window.listener_for(&self.state, |_, _, _, cx| {
                        cx.emit(TimeSelectEvent::Now);
                    })),
            )
    }
}

impl Sizable for TimeSelect {
    fn control_size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }
}

impl Styled for TimeSelect {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TimeSelect {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = self.state.read(cx);
        let mode = state.mode;
        let width = self.width(state.finest);
        let focus_handle = state.focus_handle.clone();
        let now = Local::now().naive_local();
        let page = match mode {
            TimeUnit::Year => self.render_years(width, now, window, cx),
            TimeUnit::Month => self.render_months(width, now, window, cx),
            TimeUnit::Day => self.render_days(width, now, window, cx),
            TimeUnit::Hour => self.render_hours(width, now, window, cx),
        };

        div()
            .column()
            .id(("time-select", self.state.entity_id()))
            .track_focus(&focus_handle)
            .block_mouse_except_scroll()
            .border_1()
            .border_color(cx.theme().colors.hairline)
            .rounded(px(cx.theme().radius(Radius::Dialog)))
            .p_3()
            .gap_0p5()
            .refine_style(&self.style)
            .child(self.render_header(window, cx))
            .child(page)
            .child(self.render_now(now, window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(year: i32, month: u32, day: u32, hour: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_hms_opt(hour, 30, 0)
            .unwrap()
    }

    #[test]
    fn periods_cover_their_whole_unit() {
        let time = at(2024, 2, 29, 23);
        let bounds = |unit| {
            let period = TimePeriod::containing(unit, time);
            (period.start(), period.end())
        };
        let midnight = |y, m, d| at(y, m, d, 0) - TimeDelta::minutes(30);
        assert_eq!(
            bounds(TimeUnit::Hour),
            (
                at(2024, 2, 29, 23) - TimeDelta::minutes(30),
                midnight(2024, 3, 1)
            )
        );
        assert_eq!(
            bounds(TimeUnit::Day),
            (midnight(2024, 2, 29), midnight(2024, 3, 1))
        );
        assert_eq!(
            bounds(TimeUnit::Month),
            (midnight(2024, 2, 1), midnight(2024, 3, 1))
        );
        assert_eq!(
            bounds(TimeUnit::Year),
            (midnight(2024, 1, 1), midnight(2025, 1, 1))
        );
    }

    #[test]
    fn locating_keeps_finer_fields_inside_the_chosen_period() {
        let browsed = at(2024, 1, 31, 15);
        let locate = |unit, time| TimePeriod::containing(unit, time).locate(browsed);
        let hour = |time: NaiveDateTime| TimeUnit::Hour.floor(time);
        assert_eq!(
            locate(TimeUnit::Month, at(2023, 2, 1, 0)),
            hour(at(2023, 2, 28, 15))
        );
        assert_eq!(
            locate(TimeUnit::Year, at(2030, 6, 1, 0)),
            hour(at(2030, 1, 31, 15))
        );
        assert_eq!(
            locate(TimeUnit::Day, at(2024, 3, 9, 0)),
            hour(at(2024, 3, 9, 15))
        );
        assert_eq!(
            locate(TimeUnit::Hour, at(2024, 3, 9, 4)),
            hour(at(2024, 3, 9, 4))
        );
    }
}
