use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate};
use gpui::{
    App, Bounds, ClickEvent, Context, DragMoveEvent, ElementId, FontWeight, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MousePressureEvent, MouseUpEvent,
    ParentElement, Pixels, StatefulInteractiveElement, Styled, Window, canvas, div, fill, point,
    prelude::FluentBuilder, px, relative, size,
};
use gpui_kit::foundation::StyledExt as _;
use gpui_kit_theme::ActiveTheme;
use subroutine_core::{AnyItem, SchedulePoint};

use crate::{
    color::ColorExt,
    components::{
        Button, ButtonVariants, DragData, Draggable, DraggedItems, Label, create_drag_data,
        item_context_menu, item_icon,
        transition::{self, WindowTransitionExt as _},
    },
    dates::InclusiveDateRange,
    presentation::UxColor,
};

use super::super::{format_item_time, is_calendar_item_on};
use super::{CalendarView, date_context_menu, date_range_context_menu};

pub(super) const EDGE_HORIZON: Pixels = px(200.);
const MIN_SUMMARY_WIDTH: Pixels = px(80.);
const MIN_FULL_SUMMARY_WIDTH: Pixels = px(120.);
const MONTH_BOUNDARY_WIDTH: Pixels = px(1.5);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MonthBoundarySegment {
    Horizontal { row: usize, column: usize },
    Vertical { row: usize, column: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HorizontalBoundaryRun {
    row: usize,
    start_column: usize,
    end_column: usize,
}

fn same_month(date: NaiveDate, other: NaiveDate) -> bool {
    (date.year(), date.month()) == (other.year(), other.month())
}

fn month_boundary_segments(start_date: NaiveDate, num_weeks: usize) -> Vec<MonthBoundarySegment> {
    let straddles_month = |date, other| !same_month(date, other);
    let mut segments = Vec::new();

    for row in 0..num_weeks {
        let week_start = start_date + chrono::Duration::weeks(row as i64);
        for column in 0..7 {
            let date = week_start + chrono::Duration::days(column as i64);

            if row == 0 && straddles_month(date, date - chrono::Duration::days(7)) {
                segments.push(MonthBoundarySegment::Horizontal { row, column });
            }
            if column < 6 && straddles_month(date, date + chrono::Duration::days(1)) {
                segments.push(MonthBoundarySegment::Vertical {
                    row,
                    column: column + 1,
                });
            }
            if straddles_month(date, date + chrono::Duration::days(7)) {
                segments.push(MonthBoundarySegment::Horizontal {
                    row: row + 1,
                    column,
                });
            }
        }
    }

    segments
}

fn month_boundary_segment_dates(
    start_date: NaiveDate,
    segment: MonthBoundarySegment,
) -> (NaiveDate, NaiveDate) {
    match segment {
        MonthBoundarySegment::Horizontal { row, column } => {
            let below = start_date
                + chrono::Duration::weeks(row as i64)
                + chrono::Duration::days(column as i64);
            (below - chrono::Duration::days(7), below)
        }
        MonthBoundarySegment::Vertical { row, column } => {
            let right = start_date
                + chrono::Duration::weeks(row as i64)
                + chrono::Duration::days(column as i64);
            (right - chrono::Duration::days(1), right)
        }
    }
}

fn month_boundary_groups(
    start_date: NaiveDate,
    num_weeks: usize,
) -> BTreeMap<NaiveDate, Vec<MonthBoundarySegment>> {
    let mut groups = BTreeMap::<_, Vec<_>>::new();
    for segment in month_boundary_segments(start_date, num_weeks) {
        let (_, after) = month_boundary_segment_dates(start_date, segment);
        groups
            .entry(after.with_day(1).unwrap())
            .or_default()
            .push(segment);
    }
    groups
}

fn boundary_borders_month(boundary: NaiveDate, month: NaiveDate) -> bool {
    same_month(boundary, month) || same_month(boundary - chrono::Duration::days(1), month)
}

fn month_boundary_opacity(
    boundary: NaiveDate,
    month_in_view: NaiveDate,
    window: &mut Window,
    cx: &mut App,
) -> f32 {
    let fade = window.keyed_transition(
        (
            "calendar-month-boundary",
            boundary.num_days_from_ce() as u32,
        ),
        cx,
        transition::QUICK,
        || 0.0,
    );
    fade.set(
        if boundary_borders_month(boundary, month_in_view) {
            1.0
        } else {
            0.0
        },
        cx,
    );
    fade.animate(window, cx)
}

fn month_background_tints(
    range_start: NaiveDate,
    range_end: NaiveDate,
    month_in_view: NaiveDate,
    enabled: bool,
    window: &mut Window,
    cx: &mut App,
) -> BTreeMap<NaiveDate, f32> {
    let mut tints = BTreeMap::new();
    let mut month = range_start.with_day(1).unwrap();
    while month < range_end {
        let fade = window.keyed_transition(
            ("calendar-month-tint", month.num_days_from_ce() as u32),
            cx,
            transition::QUICK,
            || 0.0,
        );
        fade.set(
            if enabled && same_month(month, month_in_view) {
                1.0
            } else {
                0.0
            },
            cx,
        );
        tints.insert(month, fade.animate(window, cx));
        let Some(next) = month.checked_add_months(chrono::Months::new(1)) else {
            break;
        };
        month = next;
    }
    tints
}

fn month_boundary_is_obscured(
    start_date: NaiveDate,
    segment: MonthBoundarySegment,
    selected_range: Option<InclusiveDateRange>,
    drop_target: Option<NaiveDate>,
) -> bool {
    let (first, second) = month_boundary_segment_dates(start_date, segment);
    selected_range.is_some_and(|range| range.contains(first) || range.contains(second))
        || drop_target.is_some_and(|date| date == first || date == second)
}

fn horizontal_boundary_runs(
    segments: &[MonthBoundarySegment],
    num_weeks: usize,
) -> Vec<HorizontalBoundaryRun> {
    let mut runs = Vec::new();

    for row in 0..=num_weeks {
        let mut start_column = None;
        for column in 0..=7 {
            let continues =
                column < 7 && segments.contains(&MonthBoundarySegment::Horizontal { row, column });
            match (start_column, continues) {
                (None, true) => start_column = Some(column),
                (Some(start), false) => {
                    runs.push(HorizontalBoundaryRun {
                        row,
                        start_column: start,
                        end_column: column,
                    });
                    start_column = None;
                }
                _ => {}
            }
        }
    }

    runs
}

fn horizontal_boundary_touches_vertex(
    segments: &[MonthBoundarySegment],
    row: usize,
    column: usize,
) -> bool {
    (column < 7 && segments.contains(&MonthBoundarySegment::Horizontal { row, column }))
        || (column > 0
            && segments.contains(&MonthBoundarySegment::Horizontal {
                row,
                column: column - 1,
            }))
}

fn vertical_boundary_touches_vertex(
    segments: &[MonthBoundarySegment],
    row: usize,
    column: usize,
) -> bool {
    segments.contains(&MonthBoundarySegment::Vertical { row, column })
        || (row > 0
            && segments.contains(&MonthBoundarySegment::Vertical {
                row: row - 1,
                column,
            }))
}

fn calendar_salience(item: &AnyItem) -> Option<u8> {
    match item {
        AnyItem::Marker(_) => Some(110),
        AnyItem::Event(_) => Some(100),
        AnyItem::Action(action) => {
            if matches!(action.start, Some(SchedulePoint::Date(_))) {
                Some(90)
            } else if action.pinned {
                Some(80)
            } else {
                None
            }
        }
        AnyItem::Signal(_) => Some(60),
        AnyItem::Routine(routine) if routine.recurrence.is_none() => Some(50),
        AnyItem::Routine(_) | AnyItem::ActionTemplate(_) | AnyItem::EventTemplate(_) => None,
    }
}

fn calendar_dragged_items(
    entries: &[(AnyItem, bool)],
    source_date: NaiveDate,
    preferred_primary: Option<uuid::Uuid>,
) -> Option<DraggedItems> {
    let mut items: Vec<_> = entries
        .iter()
        .filter(|(_, projected)| !*projected)
        .map(|(item, _)| item.clone())
        .collect();
    items.sort_by_key(|item| (item.start().map(|start| start.timestamp()), item.id()));
    items.dedup_by_key(|item| item.id());
    let primary = preferred_primary
        .and_then(|id| items.iter().find(|item| item.id() == id))
        .cloned()
        .or_else(|| items.first().cloned())?;
    Some(DraggedItems {
        primary,
        source_anchor: Some(SchedulePoint::Date(source_date)),
        items,
        saved_item_ids: None,
        materialized_item_ids: Vec::new(),
    })
}

fn calendar_summaries(entries: &[AnyItem], day_width: Pixels) -> (Vec<AnyItem>, usize) {
    if day_width < MIN_SUMMARY_WIDTH {
        return (Vec::new(), entries.len());
    }
    let limit = if day_width < MIN_FULL_SUMMARY_WIDTH {
        1
    } else {
        2
    };
    let mut candidates: Vec<_> = entries
        .iter()
        .filter_map(|item| calendar_salience(item).map(|salience| (salience, item)))
        .collect();
    candidates.sort_by_key(|(salience, item)| {
        (
            std::cmp::Reverse(*salience),
            item.start()
                .map(|start| start.timestamp())
                .unwrap_or(i64::MAX),
            item.id(),
        )
    });
    let summaries: Vec<_> = candidates
        .into_iter()
        .take(limit)
        .map(|(_, item)| item.clone())
        .collect();
    let hidden_count = entries.len().saturating_sub(summaries.len());
    (summaries, hidden_count)
}

fn render_calendar_summary(
    item: AnyItem,
    date: NaiveDate,
    day_width: Pixels,
    cx: &App,
) -> impl IntoElement {
    let item_type = item.item_type();
    let is_marker = matches!(&item, AnyItem::Marker(_));
    let neutral = cx.theme().colors.text_muted;
    let when = if is_marker {
        String::new()
    } else {
        match item.start() {
            Some(SchedulePoint::Date(_)) => "Any time".to_string(),
            Some(start) => format_item_time(start.into()),
            None => String::new(),
        }
    };
    let has_time_label = !when.is_empty() && day_width >= MIN_FULL_SUMMARY_WIDTH;
    let id: ElementId =
        format!("calendar-summary-{}-{}", date.format("%Y-%m-%d"), item.id()).into();
    let persisted = crate::stores::AppDatabaseStore::global(cx)
        .read(cx)
        .get_item(item.id())
        .is_some();
    let menu_item = item.clone();

    div()
        .id(id)
        .row()
        .h(px(20.))
        .w_full()
        .min_w_0()
        .overflow_hidden()
        .flex_none()
        .items_center()
        .gap_1()
        .px_1p5()
        .when(day_width < px(60.), |this| this.px_0p5())
        .when(persisted, |this| {
            this.on_mouse_down(MouseButton::Right, move |event, window, cx| {
                cx.stop_propagation();
                crate::components::menu::open_context_menu(
                    item_context_menu(&menu_item, None, false, cx),
                    event.position,
                    window,
                    cx,
                );
            })
        })
        .when(day_width >= px(60.), |this| {
            this.when_else(
                is_marker,
                |this| this.child(div().w(px(2.)).h_3().flex_none().rounded_full().bg(neutral)),
                |this| {
                    this.child(
                        item_icon(item_type)
                            .size_3()
                            .flex_none()
                            .text_color(neutral),
                    )
                },
            )
        })
        .when(has_time_label, |this| {
            this.child(
                Label::new(when)
                    .text_xs()
                    .flex_none()
                    .text_color(cx.theme().colors.text_muted),
            )
        })
        .child(
            Label::new(item.title().to_string())
                .text_xs()
                .min_w_0()
                .flex_1()
                .truncate(),
        )
}

fn render_calendar_activity(count: usize, date: NaiveDate, cx: &App) -> impl IntoElement {
    div()
        .id(("calendar-activity", date.num_days_from_ce() as u32))
        .row()
        .h_6()
        .w_full()
        .min_w_0()
        .flex_none()
        .items_center()
        .gap_1()
        .px_1()
        .overflow_hidden()
        .child(
            div()
                .size_1()
                .flex_none()
                .rounded_full()
                .bg(cx.theme().colors.text_muted),
        )
        .child(
            Label::new(count.to_string())
                .text_xs()
                .min_w_0()
                .flex_1()
                .truncate()
                .text_color(cx.theme().colors.text_muted),
        )
}

fn render_calendar_disclosure(
    hidden_count: usize,
    date: NaiveDate,
    day_width: Pixels,
    cx: &App,
) -> impl IntoElement {
    let id: ElementId = format!("calendar-more-{}", date.format("%Y-%m-%d")).into();

    div()
        .id(id)
        .row()
        .h(px(20.))
        .w_full()
        .min_w_0()
        .overflow_hidden()
        .flex_none()
        .items_center()
        .px_1p5()
        .when(day_width < px(60.), |this| this.px_0p5())
        .child(
            Label::new(if day_width < MIN_FULL_SUMMARY_WIDTH {
                format!("+{hidden_count}")
            } else {
                format!("+{hidden_count} more")
            })
            .text_xs()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_color(cx.theme().colors.text_muted),
        )
}

impl CalendarView {
    pub fn render_grid(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.apply_scroll_transition(window, cx);
        self.apply_edge_scroll(window, cx);
        self.sync_month_in_view(cx);

        let bounds = self.bounds.unwrap_or_default();
        let day_width = self.day_width();
        let narrow = day_width < px(60.);
        let horizon = bounds.size.height / 2. + EDGE_HORIZON;
        let scroll_position = self.scroll_offset * -1.;
        let start = scroll_position - horizon;
        let end = scroll_position + horizon;
        let interval = self.week_height;
        let first_row = (start / interval).floor() as i64;
        let first_row_top = interval * first_row as f32 - start;
        let start_week = self.week_at_row(first_row);
        let num_weeks = ((end - start) / interval).ceil() as usize;
        let num_weeks = num_weeks.min(100);

        let today = self.current_date;

        let range_start = start_week.first_day();
        let range_end = range_start + chrono::Duration::weeks(num_weeks as i64);
        let projected = self.projected_markers(range_start, range_end);
        let projected_items = self.projected_items(range_start, range_end);
        let projected_signals = self.projected_signals(range_start, range_end);
        let selected_range = self.selected_date_range;
        let selected_drag_entries =
            selected_range
                .filter(|range| range.day_count() > 1)
                .map(|range| {
                    self.markers_in_range(range)
                        .into_iter()
                        .map(|(marker, projected)| (AnyItem::Marker(marker), projected))
                        .chain(self.items_in_range(range))
                        .collect::<Vec<_>>()
                });
        let calendar_appearance = crate::settings::Settings::global(cx).calendar_appearance;
        let month_tints = month_background_tints(
            range_start,
            range_end,
            self.month_in_view,
            calendar_appearance.current_month_shading,
            window,
            cx,
        );
        let month_boundaries: Vec<_> = month_boundary_groups(range_start, num_weeks)
            .into_iter()
            .filter_map(|(boundary, segments)| {
                let opacity = month_boundary_opacity(boundary, self.month_in_view, window, cx);
                if opacity <= 0.0 {
                    return None;
                }
                let segments: Vec<_> = segments
                    .into_iter()
                    .filter(|segment| {
                        !month_boundary_is_obscured(
                            range_start,
                            *segment,
                            selected_range,
                            self.drop_target,
                        )
                    })
                    .collect();
                let horizontal = horizontal_boundary_runs(&segments, num_weeks);
                Some((segments, horizontal, opacity))
            })
            .collect();
        let theme = cx.theme();
        let month_boundary_color = theme
            .colors
            .hairline_strong
            .mix(theme.colors.hairline, 0.25);
        let entity = cx.entity();
        let range_drag_engaged = self.date_range_drag.is_some_and(|drag| drag.engaged);
        let has_horizontal_gutters = self.has_horizontal_gutters;

        div()
            .absolute()
            .inset_0()
            .id("calendar-grid")
            .children((0..num_weeks).into_iter().map(|i| {
                let y = first_row_top + i as f32 * interval;
                let week_start = start_week.first_day() + chrono::Duration::weeks(i as i64);

                div()
                    .row()
                    .absolute()
                    .top(y)
                    .w_full()
                    .h(interval)
                    .children((0..7).map(|i| {
                        let date = week_start + chrono::Duration::days(i as i64);
                        let mut date_label = if date.day() == 1 && !narrow {
                            Label::new(date.format("%b %-d").to_string())
                                .px_1p5()
                                .text_color(cx.theme().colors.text)
                                .font_weight(FontWeight::LIGHT)
                        } else {
                            Label::new(date.format("%-d").to_string())
                                .px_1p5()
                                .text_color(cx.theme().colors.text)
                                .font_weight(FontWeight::LIGHT)
                        };
                        if date == today {
                            date_label = date_label
                                .bg(UxColor::CurrentTime.color(cx.theme()))
                                .text_color(UxColor::CurrentTime.on_fill(cx.theme()))
                                .px_2()
                                .rounded_full()
                                .font_weight(FontWeight::SEMIBOLD);
                        }
                        date_label = date_label
                            .min_w_0()
                            .max_w_full()
                            .when(narrow, |this| this.px_1().text_xs())
                            .truncate();
                        let selected = selected_range.is_some_and(|range| range.contains(date));
                        let drop_active = self.drop_target == Some(date);
                        let id: ElementId = format!("calendar-{}", date.format("%Y-%m-%d")).into();
                        let entries: Vec<(AnyItem, bool)> = self
                            .markers
                            .iter()
                            .chain(self.draft_markers.iter())
                            .filter(|marker| marker.covers(date))
                            .cloned()
                            .map(|marker| (AnyItem::Marker(marker), false))
                            .chain(
                                projected
                                    .iter()
                                    .filter(|marker| marker.covers(date))
                                    .cloned()
                                    .map(|marker| (AnyItem::Marker(marker), true)),
                            )
                            .chain(
                                self.items
                                    .iter()
                                    .filter(|item| {
                                        !matches!(item, AnyItem::Marker(_))
                                            && is_calendar_item_on(item, date)
                                    })
                                    .cloned()
                                    .map(|item| (item, false)),
                            )
                            .chain(
                                projected_items
                                    .iter()
                                    .filter(|item| is_calendar_item_on(item, date))
                                    .cloned()
                                    .map(|item| (item, true)),
                            )
                            .chain(
                                self.signals
                                    .iter()
                                    .filter(|signal| {
                                        signal.datetime.with_timezone(&chrono::Local).date_naive()
                                            == date
                                    })
                                    .cloned()
                                    .map(|signal| (AnyItem::Signal(signal), false)),
                            )
                            .chain(
                                projected_signals
                                    .iter()
                                    .filter(|signal| {
                                        signal.datetime.with_timezone(&chrono::Local).date_naive()
                                            == date
                                    })
                                    .cloned()
                                    .map(|signal| (AnyItem::Signal(signal), true)),
                            )
                            .collect();
                        let has_entries = !entries.is_empty();
                        let preferred_primary = entries
                            .iter()
                            .find(|(_, projected)| !projected)
                            .map(|(item, _)| item.id());
                        let dragged_entries = selected_range
                            .filter(|range| range.contains(date))
                            .and(selected_drag_entries.as_deref())
                            .unwrap_or(&entries);
                        let dragged =
                            calendar_dragged_items(dragged_entries, date, preferred_primary);
                        let entries: Vec<_> = entries.into_iter().map(|(item, _)| item).collect();
                        let (summaries, hidden_count) = calendar_summaries(&entries, day_width);
                        let theme = cx.theme();
                        let mut bg_color = theme.colors.canvas;
                        let tint = month_tints[&date.with_day(1).unwrap()];
                        if tint > 0.0 {
                            bg_color = theme.colors.raised.mix(bg_color, 0.25 * tint);
                        }
                        if calendar_appearance.weekend_shading
                            && matches!(date.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun)
                        {
                            bg_color = theme.colors.raised.mix(bg_color, 0.3);
                        }
                        div()
                            .column()
                            .id(id)
                            .relative()
                            .h_full()
                            .w(relative(1. / 7.))
                            .flex_none()
                            .min_w_0()
                            .bg(bg_color)
                            .border(px(0.5))
                            .when(i == 0 && !has_horizontal_gutters, |this| {
                                this.border_l(px(0.))
                            })
                            .when(i == 6 && !has_horizontal_gutters, |this| {
                                this.border_r(px(0.))
                            })
                            .border_color(if drop_active || selected {
                                cx.theme().colors.focus
                            } else {
                                cx.theme().colors.hairline
                            })
                            .overflow_hidden()
                            .cursor_pointer()
                            .gap_1()
                            .when(selected, |this| {
                                this.child(
                                    div().absolute().inset_0().bg(cx.theme().colors.selected),
                                )
                            })
                            .when(drop_active, |this| {
                                this.child(
                                    div().absolute().inset_0().bg(cx.theme().colors.selected),
                                )
                            })
                            .on_drag_move::<DragData<DraggedItems>>(cx.listener(
                                move |view,
                                      event: &DragMoveEvent<DragData<DraggedItems>>,
                                      _,
                                      cx| {
                                    let in_grid = view.bounds.is_some_and(|bounds| {
                                        event.event.position.y
                                            >= bounds.origin.y + view.visible_top()
                                    });
                                    if in_grid
                                        && event.bounds.contains(&event.event.position)
                                        && !event.drag(cx).data.items.is_empty()
                                    {
                                        view.set_drop_target(date, event.event.position, cx);
                                    }
                                },
                            ))
                            .on_drop::<DragData<DraggedItems>>(cx.listener(
                                move |view, data: &DragData<DraggedItems>, window, cx| {
                                    view.commit_date_drop(&data.data, date, window, cx);
                                    cx.stop_propagation();
                                },
                            ))
                            .when(!selected && !range_drag_engaged, |this| {
                                this.active(|s| s.bg(cx.theme().colors.active))
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                                    if crate::keys::force_click_modifier(&event.modifiers) {
                                        view.force_clicked_date(
                                            date,
                                            gpui::PressureStage::Force,
                                            window,
                                            cx,
                                        );
                                    } else {
                                        view.begin_date_range_drag(
                                            date,
                                            event.position,
                                            window,
                                            cx,
                                        );
                                    }
                                }),
                            )
                            .on_mouse_move(cx.listener(
                                move |view, event: &MouseMoveEvent, _, cx| {
                                    if event.pressed_button == Some(MouseButton::Left) {
                                        view.drag_date_range_to(date, event.position, cx);
                                    }
                                },
                            ))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |view, event: &MouseUpEvent, window, cx| {
                                    view.finish_date_range_drag(date, cx);
                                    if crate::keys::force_click_modifier(&event.modifiers) {
                                        view.force_clicked_date(
                                            date,
                                            gpui::PressureStage::Zero,
                                            window,
                                            cx,
                                        );
                                    }
                                }),
                            )
                            .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                                view.date_clicked(date, event.click_count(), window, cx);
                            }))
                            .on_mouse_down(MouseButton::Right, {
                                let entity = entity.clone();
                                move |event, window, cx| {
                                    cx.stop_propagation();
                                    let range = entity
                                        .read(cx)
                                        .selected_date_range
                                        .filter(|range| range.contains(date))
                                        .unwrap_or_else(|| InclusiveDateRange::single(date));
                                    entity.update(cx, |view, cx| {
                                        if view.selected_date_range != Some(range)
                                            || view.date_selection_cursor.is_none()
                                        {
                                            view.set_date_selection(range.start(), range.end());
                                        }
                                        cx.notify();
                                    });
                                    let menu = if range.day_count() > 1 {
                                        date_range_context_menu(range, entity.clone())
                                    } else {
                                        date_context_menu(date, false, entity.clone())
                                    };
                                    crate::components::menu::open_context_menu(
                                        menu,
                                        event.position,
                                        window,
                                        cx,
                                    );
                                }
                            })
                            .on_mouse_pressure(cx.listener(
                                move |view, event: &MousePressureEvent, window, cx| {
                                    view.force_clicked_date(date, event.stage, window, cx);
                                },
                            ))
                            .when(!selected, |this| {
                                this.hover(|style| style.bg(cx.theme().colors.hover))
                            })
                            .child(
                                div()
                                    .row()
                                    .pt_1p5()
                                    .px_1()
                                    .when(narrow, |this| this.px_0p5())
                                    .w_full()
                                    .min_w_0()
                                    .justify_end()
                                    .child(date_label),
                            )
                            .child(
                                div()
                                    .column()
                                    .flex_1()
                                    .min_w_0()
                                    .min_h_0()
                                    .overflow_hidden()
                                    .px_1()
                                    .when(narrow, |this| this.px_0p5())
                                    .py(px(theme.effects.focus_ring_width))
                                    .w_full()
                                    .when(has_entries, |this| {
                                        this.child(
                                            Button::new((
                                                "calendar-entries",
                                                date.num_days_from_ce() as u32,
                                            ))
                                            .ghost()
                                            .relative()
                                            .column()
                                            .h_auto()
                                            .w_full()
                                            .min_w_0()
                                            .items_stretch()
                                            .justify_start()
                                            .gap_0p5()
                                            .p_0()
                                            .rounded_sm()
                                            .tooltip(format!(
                                                "{} · {} items · Open day",
                                                date.format("%A, %B %-d"),
                                                entries.len(),
                                            ))
                                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                                cx.stop_propagation();
                                            })
                                            .on_mouse_up(MouseButton::Left, |_, _, cx| {
                                                cx.stop_propagation();
                                            })
                                            .on_click(cx.listener(move |view, _, window, cx| {
                                                cx.stop_propagation();
                                                if view.inspector_date.is_some() {
                                                    view.close_day_inspector(window, cx);
                                                }
                                                view.open_day_inspector(date, window, cx);
                                            }))
                                            .when_else(
                                                day_width < MIN_SUMMARY_WIDTH,
                                                |this| {
                                                    this.child(render_calendar_activity(
                                                        entries.len(),
                                                        date,
                                                        cx,
                                                    ))
                                                },
                                                |this| {
                                                    this.children(summaries.into_iter().map(
                                                        |item| {
                                                            render_calendar_summary(
                                                                item, date, day_width, cx,
                                                            )
                                                        },
                                                    ))
                                                    .when(hidden_count > 0, |this| {
                                                        this.child(render_calendar_disclosure(
                                                            hidden_count,
                                                            date,
                                                            day_width,
                                                            cx,
                                                        ))
                                                    })
                                                },
                                            )
                                            .when_some(dragged, |this, dragged| {
                                                let preview_width = (bounds.size.width / 7.)
                                                    .max(px(120.))
                                                    .min(px(220.));
                                                let drag_data = create_drag_data(
                                                    dragged,
                                                    size(preview_width, px(32.)),
                                                    cx,
                                                );
                                                this.child(
                                                    Draggable::new(
                                                        (
                                                            "calendar-entries-draggable",
                                                            date.num_days_from_ce() as u32,
                                                        ),
                                                        drag_data,
                                                    )
                                                    .absolute()
                                                    .size_full(),
                                                )
                                            }),
                                        )
                                    }),
                            )
                    }))
            }))
            .children(month_boundaries.into_iter().map(
                |(month_boundaries, horizontal_boundaries, opacity)| {
                    let month_boundary_color = month_boundary_color.opacity(opacity);
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            let day_width = bounds.size.width / 7.;
                            let half_width = MONTH_BOUNDARY_WIDTH / 2.;

                            for run in &horizontal_boundaries {
                                let left_extension = if vertical_boundary_touches_vertex(
                                    &month_boundaries,
                                    run.row,
                                    run.start_column,
                                ) {
                                    half_width
                                } else {
                                    Default::default()
                                };
                                let right_extension = if vertical_boundary_touches_vertex(
                                    &month_boundaries,
                                    run.row,
                                    run.end_column,
                                ) {
                                    half_width
                                } else {
                                    Default::default()
                                };
                                let x = bounds.origin.x + day_width * run.start_column as f32;
                                let y = bounds.origin.y + first_row_top + interval * run.row as f32;
                                let width = day_width * (run.end_column - run.start_column) as f32
                                    + left_extension
                                    + right_extension;
                                window.paint_quad(fill(
                                    Bounds::new(
                                        point(x - left_extension, y - half_width),
                                        size(width, MONTH_BOUNDARY_WIDTH),
                                    ),
                                    month_boundary_color,
                                ));
                            }

                            for segment in &month_boundaries {
                                let MonthBoundarySegment::Vertical { row, column } = *segment
                                else {
                                    continue;
                                };
                                let top_inset = if horizontal_boundary_touches_vertex(
                                    &month_boundaries,
                                    row,
                                    column,
                                ) {
                                    half_width
                                } else {
                                    Default::default()
                                };
                                let bottom_inset = if horizontal_boundary_touches_vertex(
                                    &month_boundaries,
                                    row + 1,
                                    column,
                                ) {
                                    half_width
                                } else {
                                    Default::default()
                                };
                                let x = bounds.origin.x + day_width * column as f32;
                                let y = bounds.origin.y
                                    + first_row_top
                                    + interval * row as f32
                                    + top_inset;
                                window.paint_quad(fill(
                                    Bounds::new(
                                        point(x - half_width, y),
                                        size(
                                            MONTH_BOUNDARY_WIDTH,
                                            interval - top_inset - bottom_inset,
                                        ),
                                    ),
                                    month_boundary_color,
                                ));
                            }
                        },
                    )
                    .absolute()
                    .inset_0()
                },
            ))
    }
}
