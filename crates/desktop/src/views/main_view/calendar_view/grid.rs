use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate};
use gpui::{
    AnyElement, App, ClickEvent, Context, DragMoveEvent, ElementId, FontWeight, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MousePressureEvent, MouseUpEvent,
    ParentElement, Pixels, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder,
    px, relative, size,
};
use gpui_kit::foundation::StyledExt as _;
use gpui_kit_theme::ActiveTheme;
use subroutine_core::{AnyItem, Marker, SchedulePoint};

use crate::{
    AppIcon,
    color::ColorExt,
    components::{
        Button, ButtonVariants, DragData, Draggable, DraggedItems, Label, create_drag_data,
        ext::ElementExt as _,
        item_context_menu, item_icon,
        transition::{self, WindowTransitionExt as _},
    },
    dates::InclusiveDateRange,
    icons::Icon,
    item_manager::{DraftView, ItemManager},
    presentation::UxColor,
    selection::SelectionManager,
    stores::{AppDatabaseStore, WorkspaceContent},
};

use super::super::{format_item_time, is_calendar_item_on};
use super::{
    CalendarView, GridCreation,
    borders::GridBorders,
    date_context_menu, date_range_context_menu,
    markers::{MarkerLayout, marker_capacity, marker_content_height, marker_preview},
};

pub(super) const EDGE_HORIZON: Pixels = px(200.);
const MIN_SUMMARY_WIDTH: Pixels = px(80.);
const MIN_FULL_SUMMARY_WIDTH: Pixels = px(120.);

const CREATE_ROW_HEIGHT: Pixels = px(24.);
const DATE_BADGE_SIZE: Pixels = px(28.);

fn same_month(date: NaiveDate, other: NaiveDate) -> bool {
    (date.year(), date.month()) == (other.year(), other.month())
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

fn calendar_salience(item: &AnyItem) -> Option<u8> {
    match item {
        AnyItem::Marker(_) => None,
        AnyItem::Event(_) => Some(100),
        AnyItem::Action(action) => {
            if action.start.is_some() {
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

fn calendar_summaries(
    entries: &[AnyItem],
    day_width: Pixels,
    rows: usize,
) -> (Vec<AnyItem>, usize) {
    if day_width < MIN_SUMMARY_WIDTH {
        return (Vec::new(), entries.len());
    }
    let limit: usize = if day_width < MIN_FULL_SUMMARY_WIDTH {
        1
    } else {
        2
    };
    let limit = limit.min(rows.saturating_sub(usize::from(entries.len() > rows)));
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

fn calendar_summary_time(start: Option<SchedulePoint>, day_width: Pixels) -> Option<String> {
    match start? {
        SchedulePoint::DateTime(time) if day_width >= MIN_FULL_SUMMARY_WIDTH => {
            Some(format_item_time(time.into()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod preview_tests {
    use super::*;
    use subroutine_core::Action;

    #[test]
    fn creation_uses_the_full_selection_without_moving_its_endpoints() {
        let first = NaiveDate::from_ymd_opt(2026, 10, 12).unwrap();
        let last = first + chrono::Duration::days(4);
        let hovered = first + chrono::Duration::days(2);
        for range in [
            InclusiveDateRange::new(first, last),
            InclusiveDateRange::new(last, first),
        ] {
            let marker = GridCreation::new(hovered, Some(range)).marker();
            assert_eq!(marker.date, first);
            assert_eq!(marker.end_date, Some(last));
            for offset in 0..range.day_count() {
                assert!(marker.covers(first + chrono::Duration::days(i64::from(offset))));
            }
            assert!(!marker.covers(first.pred_opt().unwrap()));
            assert!(!marker.covers(last.succ_opt().unwrap()));
        }
        for selection in [
            None,
            Some(InclusiveDateRange::single(hovered)),
            Some(InclusiveDateRange::single(first)),
        ] {
            let marker = GridCreation::new(hovered, selection).marker();
            assert_eq!(marker.date, hovered);
            assert_eq!(marker.end_date, None);
            assert!(marker.covers(hovered));
            assert!(!marker.covers(hovered.succ_opt().unwrap()));
        }
    }

    #[test]
    fn timed_actions_receive_previews_within_cell_limits() {
        let date = NaiveDate::from_ymd_opt(2026, 10, 12).unwrap();
        let items: Vec<_> = [14, 9]
            .into_iter()
            .map(|hour| {
                AnyItem::Action(Action::new(format!("Action at {hour}")).with_start(Some(
                    SchedulePoint::DateTime(date.and_hms_opt(hour, 0, 0).unwrap().and_utc()),
                )))
            })
            .collect();
        let (summaries, hidden) = calendar_summaries(&items, px(160.), 3);
        assert_eq!(
            summaries.iter().map(AnyItem::id).collect::<Vec<_>>(),
            vec![items[1].id(), items[0].id()]
        );
        assert_eq!(hidden, 0);

        let (summaries, hidden) = calendar_summaries(&items, px(100.), 3);
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id(), items[1].id());
        assert_eq!(hidden, 1);

        for (width, rows) in [(px(60.), 3), (px(160.), 1)] {
            let (summaries, hidden) = calendar_summaries(&items, width, rows);
            assert!(summaries.is_empty());
            assert_eq!(hidden, items.len());
        }
    }

    #[test]
    fn preview_time_requires_a_timed_start_and_a_wide_cell() {
        let date = NaiveDate::from_ymd_opt(2026, 10, 12).unwrap();
        let time = date
            .and_hms_opt(9, 30, 0)
            .unwrap()
            .and_local_timezone(chrono::Local)
            .single()
            .unwrap();
        let start = Some(SchedulePoint::DateTime(time.into()));
        assert_eq!(
            calendar_summary_time(start, px(120.)).as_deref(),
            Some("9:30am")
        );
        assert_eq!(calendar_summary_time(start, px(119.)), None);
        assert_eq!(
            calendar_summary_time(Some(SchedulePoint::Date(date)), px(160.)),
            None
        );
        assert_eq!(calendar_summary_time(None, px(160.)), None);
    }

    #[test]
    fn busy_dates_leave_room_for_creation_and_report_hidden_items() {
        let date = NaiveDate::from_ymd_opt(2026, 10, 12).unwrap();
        let items: Vec<_> = (0..4)
            .map(|index| {
                AnyItem::Action(
                    Action::new(format!("Action {index}"))
                        .with_start(Some(SchedulePoint::Date(date))),
                )
            })
            .collect();
        for (rows, expected) in [(1, 0), (2, 1), (3, 2)] {
            let (summaries, hidden) = calendar_summaries(&items, px(160.), rows);
            assert_eq!(summaries.len(), expected);
            assert_eq!(summaries.len() + hidden, items.len());
            assert!(summaries.len() + usize::from(hidden > 0) <= rows);
        }
        let (summaries, hidden) = calendar_summaries(&items[..1], px(160.), 1);
        assert_eq!(summaries.len(), 1);
        assert_eq!(hidden, 0);
    }
}

fn render_calendar_summary(
    item: AnyItem,
    date: NaiveDate,
    day_width: Pixels,
    cx: &App,
) -> impl IntoElement {
    let item_type = item.item_type();
    let neutral = cx.theme().colors.text_muted;
    let when = calendar_summary_time(item.start(), day_width);
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
            this.child(
                item_icon(item_type)
                    .size_3()
                    .flex_none()
                    .text_color(neutral),
            )
        })
        .children(when.map(|when| {
            Label::new(when)
                .text_xs()
                .flex_none()
                .text_color(cx.theme().colors.text_muted)
        }))
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
    fn render_grid_create(
        &self,
        date: NaiveDate,
        visible: bool,
        hidden_markers: Vec<Marker>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let row = div()
            .w_full()
            .h(CREATE_ROW_HEIGHT)
            .flex_none()
            .flex()
            .items_center()
            .justify_center();

        let button = if hidden_markers.is_empty() {
            visible.then(|| self.render_grid_create_button(date, cx))
        } else {
            Some(self.render_marker_overflow(date, hidden_markers, cx))
        };
        row.children(button).into_any_element()
    }

    fn render_grid_create_button(&self, date: NaiveDate, cx: &Context<Self>) -> Button {
        let colors = &cx.theme().colors;
        let hover_background = colors.control_hover.alpha(0.8);
        let hover_foreground = colors.text;
        let creation = GridCreation::new(date, self.selected_date_range);
        Button::new(("calendar-create-item", date.num_days_from_ce() as u32))
            .ghost()
            .compact()
            .size(CREATE_ROW_HEIGHT)
            .rounded_full()
            .bg(colors.control.alpha(0.55))
            .text_color(colors.text_muted.alpha(0.7))
            .hover(move |style| style.bg(hover_background).text_color(hover_foreground))
            .icon(Icon::new(AppIcon::Plus).size_4())
            .role(gpui::Role::Button)
            .aria_label(creation.label())
            .tooltip("New marker")
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                SelectionManager::claim_press(cx);
                cx.stop_propagation();
            })
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_pressure(|_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                cx.stop_propagation();
                if event.click_count() == 1 {
                    view.begin_grid_draft(date, window, cx);
                }
            }))
    }

    pub fn render_grid(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.validate_marker_resize(cx);
        self.apply_scroll_transition(window, cx);
        self.apply_edge_scroll(window, cx);
        self.sync_month_in_view(cx);

        let bounds = self.bounds.unwrap_or_default();
        let day_width = self.day_width();
        let narrow = day_width < px(60.);
        let focus_ring_width = px(cx.theme().effects.focus_ring_width);
        let cell_inset =
            (window.rem_size() * if narrow { 0.125 } else { 0.25 }).max(focus_ring_width);
        let horizon = bounds.size.height / 2. + EDGE_HORIZON;

        let scroll_position = self.scroll_offset * -1.;
        let start = scroll_position - horizon;
        let end = scroll_position + horizon;
        let interval = self.week_height;
        let first_row = (start / interval).floor() as i64;
        let first_row_top = interval * first_row as f32 - start;
        let start_week = self.week_at_row(first_row);
        let last_row = (end / interval).ceil() as i64;
        let num_weeks = (last_row - first_row).clamp(0, 100) as usize;

        let today = self.current_date;

        let range_start = start_week.first_day();
        let range_end = range_start + chrono::Duration::weeks(num_weeks as i64);
        self.day_button_layouts
            .retain(|date, _| range_start <= *date && *date < range_end);
        let content_top = DATE_BADGE_SIZE + px(10.) + focus_ring_width;
        let preview_height =
            interval - DATE_BADGE_SIZE - px(11.) - focus_ring_width * 2. - CREATE_ROW_HEIGHT;
        let marker_limit = marker_capacity(preview_height);
        let manager = ItemManager::global(cx);
        let draft = match manager.read(cx).view_draft(DraftView::Calendar) {
            Some(AnyItem::Marker(marker)) => Some(marker.clone()),
            _ => None,
        };
        let preferred =
            self.marker_resize
                .as_ref()
                .map(|resize| resize.item_id)
                .or_else(|| {
                    manager.read(cx).editing_item.as_ref().and_then(|editing| {
                        match &editing.original {
                            crate::item_subject::ItemSubject::Live(AnyItem::Marker(marker)) => {
                                Some(marker.id)
                            }
                            _ => None,
                        }
                    })
                });
        let marker_entries = self
            .stored_markers()
            .cloned()
            .map(|marker| (self.resizing_marker(marker), false))
            .chain(
                self.projected_markers(range_start, range_end)
                    .into_iter()
                    .map(|marker| (marker, true)),
            )
            .chain(draft.into_iter().map(|marker| (marker, false)))
            .map(|(marker, projected)| (marker_preview(marker, cx), projected))
            .collect::<Vec<_>>();
        let mut markers = MarkerLayout::new(marker_entries, &mut self.marker_lanes);
        if let Some(id) = preferred
            && self
                .marker_lanes
                .lane(id)
                .is_some_and(|lane| lane >= marker_limit)
        {
            markers.promote(id, &mut self.marker_lanes);
        }
        self.reveal_grid_marker_edit(content_top, cx);
        let marker_order = markers.order(range_start, range_end, cx);

        let projected_items = self.projected_items(range_start, range_end);
        let projected_signals = self.projected_signals(range_start, range_end);
        let selected_range = self.selected_date_range;
        let selected_drag_entries = selected_range
            .filter(|range| range.day_count() > 1)
            .map(|range| self.items_in_range(range));
        let calendar_appearance = crate::settings::Settings::global(cx).calendar_appearance;
        let month_tints = month_background_tints(
            range_start,
            range_end,
            self.month_in_view,
            calendar_appearance.current_month_shading,
            window,
            cx,
        );
        let borders = GridBorders::new(
            range_start,
            num_weeks,
            selected_range,
            self.drop_target,
            |boundary| month_boundary_opacity(boundary, self.month_in_view, window, cx),
        );
        let entity = cx.entity();
        let range_drag_engaged = self.date_range_drag.is_some_and(|drag| drag.engaged);

        let can_create = self.loaded
            && AppDatabaseStore::global(cx).read(cx).workspace_content() == WorkspaceContent::Ready
            && !manager.read(cx).is_editing()
            && !self.item_drag_active
            && self.marker_resize.is_none()
            && !cx.has_active_drag()
            && self.date_range_drag.is_none()
            && !self.marquee.is_active()
            && self.inspector_date.is_none()
            && !self.month_selector_open;

        let empty_create_inset =
            (interval * 0.45 - content_top - CREATE_ROW_HEIGHT / 2.).max(px(0.));

        div()
            .absolute()
            .inset_0()
            .id("calendar-grid")
            .children((0..num_weeks).map(|row| {
                let y = first_row_top + row as f32 * interval;
                let week_start = start_week.first_day() + chrono::Duration::weeks(row as i64);

                div()
                    .row()
                    .absolute()
                    .top(y)
                    .w_full()
                    .h(interval)
                    .children((0..7).map(|column| {
                        let date = week_start + chrono::Duration::days(column as i64);

                        let date_label = div()
                            .row()
                            .gap_0p5()
                            .text_base()
                            .text_color(cx.theme().colors.text)
                            .font_weight(FontWeight::LIGHT)
                            .min_w_0()
                            .max_w_full()
                            .when(narrow, |this| this.text_sm())
                            .when(date.day() == 1 && !narrow, |this| {
                                this.child(
                                    Label::new(date.format("%b").to_string())
                                        .min_w_0()
                                        .flex_shrink_1()
                                        .truncate(),
                                )
                            })
                            .child(
                                Label::new(date.format("%-d").to_string())
                                    .size(DATE_BADGE_SIZE)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when(date == today, |this| {
                                        this.bg(UxColor::CurrentTime.color(cx.theme()))
                                            .text_color(UxColor::CurrentTime.on_fill(cx.theme()))
                                            .rounded_full()
                                    }),
                            );
                        let selected = selected_range.is_some_and(|range| range.contains(date));
                        let drop_active = self.drop_target == Some(date);
                        let id: ElementId = format!("calendar-{}", date.format("%Y-%m-%d")).into();
                        let marker_height =
                            marker_content_height(markers.visible_lanes_on(date, marker_limit));
                        let preview_rows =
                            ((preview_height - marker_height) / px(22.)).floor().max(0.) as usize;
                        let entries: Vec<(AnyItem, bool)> = self
                            .items
                            .iter()
                            .filter(|item| is_calendar_item_on(item, date))
                            .cloned()
                            .map(|item| (item, false))
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
                        let (summaries, hidden_count) =
                            calendar_summaries(&entries, day_width, preview_rows);

                        let show_create = can_create
                            && self.hovered_date == Some(date)
                            && selected_range
                                .filter(|range| range.day_count() > 1)
                                .is_none_or(|range| range.contains(date));
                        let create_row = self.render_grid_create(
                            date,
                            show_create,
                            markers.hidden_on(date, marker_limit),
                            cx,
                        );
                        let theme = cx.theme();
                        let mut bg_color = theme.colors.canvas.alpha(0.0);
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
                            .p(px(0.5))
                            .overflow_hidden()
                            .cursor_pointer()
                            .on_hover(cx.listener(move |view, hovered, _, cx| {
                                let next = if *hovered {
                                    Some(date)
                                } else if view.hovered_date == Some(date) {
                                    None
                                } else {
                                    return;
                                };
                                if view.hovered_date != next {
                                    view.hovered_date = next;
                                    cx.notify();
                                }
                            }))
                            .gap_1()
                            .when(selected || drop_active, |this| {
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
                                    .px(cell_inset)
                                    .w_full()
                                    .min_w_0()
                                    .justify_end()
                                    .child(
                                        Button::new((
                                            "calendar-open-day",
                                            date.num_days_from_ce() as u32,
                                        ))
                                        .ghost()
                                        .compact()
                                        .h_auto()
                                        .w_full()
                                        .p_0()
                                        .justify_end()
                                        .min_w_0()
                                        .max_w_full()
                                        .rounded_md()
                                        .child(date_label)
                                        .on_prepaint({
                                            let entity = entity.clone();
                                            move |bounds, _, cx| {
                                                entity.update(cx, |view, _| {
                                                    view.measure_day_button(date, bounds)
                                                });
                                            }
                                        })
                                        .role(gpui::Role::Button)
                                        .aria_label(format!(
                                            "Open {}",
                                            date.format("%A, %B %-d, %Y")
                                        ))
                                        .tooltip("Open day · Events and actions")
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            SelectionManager::claim_press(cx);
                                            cx.stop_propagation();
                                        })
                                        .on_mouse_up(
                                            MouseButton::Left,
                                            cx.listener(move |view, _, _, cx| {
                                                view.finish_date_range_drag(date, cx);
                                                cx.stop_propagation();
                                            }),
                                        )
                                        .on_mouse_pressure(|_, _, cx| cx.stop_propagation())
                                        .on_click(
                                            cx.listener(move |view, _, window, cx| {
                                                cx.stop_propagation();
                                                view.open_day_inspector(date, window, cx);
                                            }),
                                        ),
                                    ),
                            )
                            .child(
                                div()
                                    .column()
                                    .flex_1()
                                    .min_w_0()
                                    .min_h_0()
                                    .overflow_hidden()
                                    .px(cell_inset)
                                    .py(focus_ring_width)
                                    .w_full()
                                    .child(div().h(marker_height).flex_none())
                                    .child(
                                        div()
                                            .column()
                                            .w_full()
                                            .min_w_0()
                                            .min_h_0()
                                            .flex_shrink_1()
                                            .when(has_entries && preview_rows > 0, |this| {
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
                                                    .on_click(cx.listener(
                                                        move |view, _, window, cx| {
                                                            cx.stop_propagation();
                                                            if view.inspector_date.is_some() {
                                                                view.close_day_inspector(
                                                                    window, cx,
                                                                );
                                                            }
                                                            view.open_day_inspector(
                                                                date, window, cx,
                                                            );
                                                        },
                                                    ))
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
                                                            this.children(
                                                                summaries.into_iter().map(|item| {
                                                                    render_calendar_summary(
                                                                        item, date, day_width, cx,
                                                                    )
                                                                }),
                                                            )
                                                            .when(hidden_count > 0, |this| {
                                                                this.child(
                                                                    render_calendar_disclosure(
                                                                        hidden_count,
                                                                        date,
                                                                        day_width,
                                                                        cx,
                                                                    ),
                                                                )
                                                            })
                                                        },
                                                    )
                                                    .when_some(dragged, |this, dragged| {
                                                        let preview_width = (bounds.size.width
                                                            / 7.)
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
                                    .when(!has_entries && marker_height == px(0.), |content| {
                                        content.child(div().h(empty_create_inset).flex_none())
                                    })
                                    .child(create_row),
                            )
                    }))
            }))
            .child(borders.render(
                first_row_top,
                interval,
                self.layout_state.read(cx).left_panel_right(),
                cx,
            ))
            .children((0..num_weeks).flat_map(|row| {
                let week = range_start + chrono::Duration::weeks(row as i64);
                let top = first_row_top + interval * row as f32 + content_top;
                markers
                    .week(week)
                    .into_iter()
                    .filter(|span| span.is_visible(marker_limit))
                    .map(|span| {
                        self.render_marker_span(span, &marker_order, top, day_width, window, cx)
                    })
                    .collect::<Vec<_>>()
            }))
    }
}
