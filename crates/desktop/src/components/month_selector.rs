use chrono::{Datelike, Local, Month};
use gpui::{
    App, ClickEvent, Context, Div, ElementId, Empty, Entity, EventEmitter, FocusHandle,
    InteractiveElement, IntoElement, ParentElement, Render, RenderOnce, SharedString, Stateful,
    StatefulInteractiveElement, StyleRefinement, Styled, Window, div, prelude::FluentBuilder as _,
    px, relative,
};

use crate::AppIcon;
use crate::components::Button;
use crate::components::ButtonVariants as _;
use crate::components::ext::StyledRefineExt as _;
use gpui_kit::foundation::{Disableable as _, FocusRing as _};

use gpui_kit::foundation::Sizable;
use gpui_kit::foundation::StyledExt as _;
use gpui_kit_theme::{ActiveTheme, ControlSize, Radius};

pub enum MonthSelectEvent {
    Selected(Month),
    Today,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Month,
    Year,
}

fn is_viewed_month(viewed_year: i32, browsed_year: i32, viewed_month: u8, month: u8) -> bool {
    viewed_year == browsed_year && viewed_month == month
}

fn is_actual_month(actual_year: i32, browsed_year: i32, actual_month: u8, month: u8) -> bool {
    actual_year == browsed_year && actual_month == month
}

impl ViewMode {
    fn is_month(&self) -> bool {
        matches!(self, Self::Month)
    }

    fn is_year(&self) -> bool {
        matches!(self, Self::Year)
    }
}

#[derive(IntoElement)]
pub struct MonthSelect {
    id: ElementId,
    size: ControlSize,
    state: Entity<MonthSelectState>,
    style: StyleRefinement,
    number_of_months: usize,
}

pub struct MonthSelectState {
    focus_handle: FocusHandle,
    view_mode: ViewMode,
    current_year: i32,
    current_month: u8,
    viewed_year: i32,
    years: Vec<Vec<i32>>,
    year_page: i32,
    number_of_months: usize,
}

impl MonthSelectState {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let today = Local::now().naive_local().date();
        Self {
            focus_handle: cx.focus_handle(),
            view_mode: ViewMode::Month,
            current_month: today.month() as u8,
            current_year: today.year(),
            viewed_year: today.year(),
            years: vec![],
            year_page: 0,
            number_of_months: 1,
        }
        .year_range((today.year() - 50, today.year() + 50))
    }

    pub fn set_month(&mut self, year: i32, month: Month, cx: &mut Context<Self>) {
        self.current_year = year;
        self.current_month = month.number_from_month() as u8;
        self.viewed_year = year;
        self.year_page = self
            .years
            .iter()
            .position(|years| years.contains(&year))
            .map(|page| page as i32)
            .unwrap_or(self.year_page);
        cx.notify();
    }

    pub fn current_year(&self) -> i32 {
        self.current_year
    }

    pub fn year_range(mut self, range: (i32, i32)) -> Self {
        self.apply_year_range(range);
        self
    }

    fn apply_year_range(&mut self, range: (i32, i32)) {
        self.years = (range.0..range.1)
            .collect::<Vec<_>>()
            .chunks(20)
            .map(|chunk| chunk.to_vec())
            .collect::<Vec<_>>();
        self.year_page = self
            .years
            .iter()
            .position(|years| years.contains(&self.current_year))
            .unwrap_or(0) as i32;
    }

    fn offset_year_month(&self, offset_month: usize) -> (i32, u32) {
        let mut month = self.current_month as i32 + offset_month as i32;
        let mut year = self.current_year;
        while month < 1 {
            month += 12;
            year -= 1;
        }
        while month > 12 {
            month -= 12;
            year += 1;
        }

        (year, month as u32)
    }

    fn has_prev_year_page(&self) -> bool {
        self.year_page > 0
    }

    fn has_next_year_page(&self) -> bool {
        self.year_page < self.years.len() as i32 - 1
    }

    fn prev_year_page(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.has_prev_year_page() {
            return;
        }

        self.year_page -= 1;
        cx.notify()
    }

    fn next_year_page(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.has_next_year_page() {
            return;
        }

        self.year_page += 1;
        cx.notify()
    }

    fn month_name(&self, offset_month: usize) -> SharedString {
        let (_, month) = self.offset_year_month(offset_month);
        match month {
            1 => "January",
            2 => "February",
            3 => "March",
            4 => "April",
            5 => "May",
            6 => "June",
            7 => "July",
            8 => "August",
            9 => "September",
            10 => "October",
            11 => "November",
            12 => "December",
            _ => "",
        }
        .into()
    }

    fn year_name(&self, offset_month: usize) -> SharedString {
        let (year, _) = self.offset_year_month(offset_month);
        year.to_string().into()
    }

    fn set_view_mode(&mut self, mode: ViewMode, _: &mut Window, cx: &mut Context<Self>) {
        self.view_mode = mode;
        cx.notify();
    }

    fn months(&self) -> Vec<SharedString> {
        [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ]
        .into_iter()
        .map(|s| s.into())
        .collect()
    }
}

impl Render for MonthSelectState {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl MonthSelect {
    pub fn new(state: &Entity<MonthSelectState>) -> Self {
        Self {
            id: ("calendar", state.entity_id()).into(),
            size: ControlSize::default(),
            state: state.clone(),
            style: StyleRefinement::default(),
            number_of_months: 1,
        }
    }

    fn render_header(&self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = self.state.read(cx);
        let current_year = state.current_year;
        let view_mode = state.view_mode;
        let multiple_months = self.number_of_months > 1;
        let icon_size = match self.size {
            ControlSize::Sm => ControlSize::Sm,
            ControlSize::Lg => ControlSize::Md,
            _ => ControlSize::Md,
        };

        div()
            .row()
            .gap_0p5()
            .justify_between()
            .items_center()
            .when_else(
                !view_mode.is_month(),
                |this| {
                    this.child(
                        Button::new("prev")
                            .icon(AppIcon::ArrowLeft)
                            .ghost()
                            .control_size(icon_size)
                            .when(view_mode.is_year(), |this| {
                                this.when(!state.has_prev_year_page(), |this| this.disabled(true))
                                    .on_click(window.listener_for(
                                        &self.state,
                                        MonthSelectState::prev_year_page,
                                    ))
                            }),
                    )
                },
                |this| this.child(div()),
            )
            .when(!multiple_months, |this| {
                this.child(
                    div()
                        .row()
                        .justify_center()
                        .gap_3()
                        .child(
                            Button::new("month")
                                .ghost()
                                .label(state.month_name(0))
                                .control_size(self.size)
                                .current(view_mode.is_month())
                                .on_click(window.listener_for(
                                    &self.state,
                                    move |view, _, window, cx| {
                                        if view_mode.is_month() {
                                        } else {
                                            view.set_view_mode(ViewMode::Month, window, cx);
                                        }
                                        cx.notify();
                                    },
                                )),
                        )
                        .child(
                            Button::new("year")
                                .ghost()
                                .label(current_year.to_string())
                                .control_size(self.size)
                                .current(view_mode.is_year())
                                .on_click(window.listener_for(
                                    &self.state,
                                    |view, _, window, cx| {
                                        if view.view_mode.is_year() {
                                        } else {
                                            view.set_view_mode(ViewMode::Year, window, cx);
                                        }
                                        cx.notify();
                                    },
                                )),
                        ),
                )
            })
            .when(multiple_months, |this| {
                this.child(div().row().flex_1().justify_around().children(
                    (0..self.number_of_months).map(|n| {
                        div()
                            .row()
                            .justify_center()
                            .map(|this| match self.size {
                                ControlSize::Sm => this.gap_2(),
                                ControlSize::Lg => this.gap_4(),
                                _ => this.gap_3(),
                            })
                            .child(state.month_name(n))
                            .child(state.year_name(n))
                    }),
                ))
            })
            .when_else(
                !view_mode.is_month(),
                |this| {
                    this.child(
                        Button::new("next")
                            .icon(AppIcon::ArrowRight)
                            .ghost()
                            .control_size(icon_size)
                            .when(view_mode.is_year(), |this| {
                                this.when(!state.has_next_year_page(), |this| this.disabled(true))
                                    .on_click(window.listener_for(
                                        &self.state,
                                        MonthSelectState::next_year_page,
                                    ))
                            }),
                    )
                },
                |this| this.child(div()),
            )
    }

    #[allow(clippy::too_many_arguments)]
    fn item_button(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        active: bool,
        secondary_active: bool,
        muted: bool,
        disabled: bool,
        _: &mut Window,
        cx: &mut App,
    ) -> Stateful<Div> {
        div()
            .row()
            .id(id.into())
            .map(|this| match self.size {
                ControlSize::Sm => this.size_7().rounded(px(cx.theme().radii.control) / 2.),
                ControlSize::Lg => this.size_10().rounded(px(cx.theme().radii.control) * 2.),
                _ => this.size_9().rounded(px(cx.theme().radii.control)),
            })
            .justify_center()
            .text_color(cx.theme().colors.text)
            .when(muted, |this| {
                this.text_color(if disabled {
                    cx.theme().colors.text_muted.opacity(0.3)
                } else {
                    cx.theme().colors.text_muted
                })
            })
            .when(secondary_active, |this| {
                this.bg(if muted {
                    cx.theme().colors.selected.opacity(0.5)
                } else {
                    cx.theme().colors.selected
                })
                .text_color(cx.theme().colors.text)
            })
            .when(!active && !disabled, |this| {
                this.hover(|this| {
                    this.bg(cx.theme().colors.hover)
                        .text_color(cx.theme().colors.text)
                })
            })
            .when(active, |this| {
                this.bg(cx.theme().colors.accent)
                    .text_color(cx.theme().colors.text_on_accent)
            })
            .role(gpui::Role::Button)
            .when(!disabled, |this| {
                this.tab_index(0).cursor_pointer().focus_ring(cx.theme())
            })
            .child(label.into())
    }

    fn render_months(&self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = self.state.read(cx);
        let months = state.months();
        let current_month = state.current_month;
        let viewed_year = state.viewed_year;
        let current_year = state.current_year;
        let today = Local::now().date_naive();
        let actual_year = today.year();
        let actual_month = today.month() as u8;

        div()
            .row()
            .mt_3()
            .gap_0p5()
            .gap_y_3()
            .map(|this| match self.size {
                ControlSize::Sm => this.mt_2().gap_y_2().w(px(208.)),
                ControlSize::Lg => this.mt_4().gap_y_4().w(px(292.)),
                _ => this.mt_3().gap_y_3().w(px(264.)),
            })
            .justify_between()
            .flex_wrap()
            .children(
                months
                    .iter()
                    .enumerate()
                    .map(|(ix, month)| {
                        let month_number = (ix + 1) as u8;
                        let active =
                            is_viewed_month(viewed_year, current_year, current_month, month_number);
                        let actual =
                            is_actual_month(actual_year, current_year, actual_month, month_number);

                        self.item_button(
                            ix,
                            month.to_string(),
                            active,
                            actual,
                            false,
                            false,
                            window,
                            cx,
                        )
                        .w(relative(0.3))
                        .text_sm()
                        .on_click(window.listener_for(
                            &self.state,
                            move |view, _, _window, cx| {
                                view.current_month = (ix + 1) as u8;
                                cx.emit(MonthSelectEvent::Selected(
                                    Month::try_from(view.current_month).unwrap_or(Month::January),
                                ));
                                cx.notify();
                            },
                        ))
                    })
                    .collect::<Vec<_>>(),
            )
    }

    fn render_today(&self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let today = Local::now().date_naive();
        div()
            .mt_3()
            .pt_3()
            .border_t_1()
            .border_color(cx.theme().colors.hairline)
            .child(
                Button::new("calendar-month-selector-today")
                    .ghost()
                    .w_full()
                    .justify_start()
                    .icon(AppIcon::Calendar)
                    .label(format!("Today · {}", today.format("%b %-d, %Y")))
                    .text_color(cx.theme().colors.accent)
                    .on_click(window.listener_for(&self.state, |_, _, _, cx| {
                        cx.emit(MonthSelectEvent::Today);
                    })),
            )
    }

    fn render_years(&self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = self.state.read(cx);
        let current_year = state.current_year;
        let current_page_years = &self.state.read(cx).years[state.year_page as usize].clone();

        div()
            .row()
            .id("years")
            .gap_0p5()
            .map(|this| match self.size {
                ControlSize::Sm => this.mt_2().gap_y_2().w(px(208.)),
                ControlSize::Lg => this.mt_4().gap_y_4().w(px(292.)),
                _ => this.mt_3().gap_y_3().w(px(264.)),
            })
            .justify_between()
            .flex_wrap()
            .children(
                current_page_years
                    .iter()
                    .enumerate()
                    .map(|(ix, year)| {
                        let year = *year;
                        let active = year == current_year;

                        self.item_button(
                            ix,
                            year.to_string(),
                            active,
                            false,
                            false,
                            false,
                            window,
                            cx,
                        )
                        .w(relative(0.2))
                        .on_click(window.listener_for(
                            &self.state,
                            move |view, _, window, cx| {
                                view.current_year = year;
                                view.set_view_mode(ViewMode::Month, window, cx);
                                cx.notify();
                            },
                        ))
                    })
                    .collect::<Vec<_>>(),
            )
    }
}

impl Sizable for MonthSelect {
    fn control_size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }
}

impl Styled for MonthSelect {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl EventEmitter<MonthSelectEvent> for MonthSelectState {}
impl RenderOnce for MonthSelect {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let view_mode = self.state.read(cx).view_mode;
        let number_of_months = self.number_of_months;
        self.state.update(cx, |state, _| {
            state.number_of_months = number_of_months;
        });

        div()
            .column()
            .id(self.id.clone())
            .track_focus(&self.state.read(cx).focus_handle)
            .block_mouse_except_scroll()
            .border_1()
            .border_color(cx.theme().colors.hairline)
            .rounded(px(cx.theme().radius(Radius::Dialog)))
            .p_3()
            .gap_0p5()
            .refine_style(&self.style)
            .child(self.render_header(window, cx))
            .child(
                div()
                    .column()
                    .when(view_mode.is_month(), |this| {
                        this.child(self.render_months(window, cx))
                    })
                    .when(view_mode.is_year(), |this| {
                        this.child(self.render_years(window, cx))
                    }),
            )
            .child(self.render_today(window, cx))
    }
}
