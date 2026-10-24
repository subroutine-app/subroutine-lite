use std::collections::{HashMap, HashSet};

use chrono::{Datelike, NaiveDate};
use gpui::{
    AnyElement, App, AppContext, ClickEvent, Context, DragMoveEvent, Focusable, IntoElement,
    MouseButton, MouseUpEvent, ParentElement, Pixels, Styled, Window, div,
    prelude::FluentBuilder as _, px, relative, size,
};
use gpui::{InteractiveElement, StatefulInteractiveElement};
use subroutine_core::{AnyItem, Marker, Recurrence, SchedulePoint};
use uuid::Uuid;

use crate::{
    components::{
        Button, ButtonVariants, DRAG_HOLD, DragData, DraggedItems, ItemCard, ItemCardPosition,
        create_drag_data, menu::MenuBuilder,
    },
    dates::InclusiveDateRange,
    item_manager::ItemManager,
    selection::{SelectionManager, SelectionOrder, SelectionScope},
};

use super::{CalendarView, marker_resize::MarkerResizeEdge};

pub(super) const MARKER_HEIGHT: Pixels = px(24.);
pub(super) const MARKER_PITCH: Pixels = px(26.);
const MARKER_CONTENT_GAP: Pixels = px(4.);

pub(super) fn marker_content_height(lanes: usize) -> Pixels {
    if lanes == 0 {
        px(0.)
    } else {
        MARKER_PITCH * (lanes - 1) as f32 + MARKER_HEIGHT + MARKER_CONTENT_GAP
    }
}

pub(super) fn marker_capacity(height: Pixels) -> usize {
    ((height - MARKER_CONTENT_GAP + MARKER_PITCH - MARKER_HEIGHT) / MARKER_PITCH)
        .floor()
        .max(0.) as usize
}

#[derive(PartialEq, Eq)]
struct MarkerGeometry {
    start: NaiveDate,
    end: Option<NaiveDate>,
    recurrence: Option<Recurrence>,
    lineage: Uuid,
}

#[derive(Default)]
pub(super) struct MarkerReveal {
    revealed: Option<Uuid>,
    geometry: HashMap<Uuid, MarkerGeometry>,
}

impl MarkerReveal {
    fn reconcile(&mut self, entries: &[(Marker, bool)]) {
        let geometry = entries
            .iter()
            .filter(|(_, projected)| !projected)
            .map(|(marker, _)| {
                (
                    marker.id,
                    MarkerGeometry {
                        start: marker.date,
                        end: marker.end_date,
                        recurrence: marker.recurrence,
                        lineage: marker.lineage_id,
                    },
                )
            })
            .collect();
        // Scrolling changes projected occurrences, not their stored sources.
        if self.geometry != geometry {
            self.geometry = geometry;
            self.revealed = None;
        }
    }

    pub(super) fn reveal(&mut self, id: Uuid) {
        self.revealed = Some(id);
    }
}
const END_INSET: Pixels = px(4.);

pub(super) struct CalendarMarker {
    pub marker: Marker,
    pub projected: bool,
}

impl CalendarMarker {
    fn last_date(&self) -> NaiveDate {
        self.marker
            .end_date
            .unwrap_or(self.marker.date)
            .max(self.marker.date)
    }

    fn day_count(&self) -> i64 {
        (self.last_date() - self.marker.date).num_days() + 1
    }
}

#[derive(Clone, Copy)]
struct PlacedSpan {
    entry: usize,
    dates: InclusiveDateRange,
    lane: usize,
}

pub(super) struct MarkerLayout {
    markers: Vec<CalendarMarker>,
    weeks: HashMap<NaiveDate, Vec<PlacedSpan>>,
}

pub(super) struct MarkerSpan<'a> {
    entry: &'a CalendarMarker,
    dates: InclusiveDateRange,
    lane: usize,
}

impl MarkerSpan<'_> {
    pub(super) fn is_visible(&self, capacity: usize) -> bool {
        self.lane < capacity
    }
}

fn week_start(date: NaiveDate) -> NaiveDate {
    date - chrono::Duration::days(i64::from(date.weekday().num_days_from_sunday()))
}

impl MarkerLayout {
    pub(super) fn new(
        entries: impl IntoIterator<Item = (Marker, bool)>,
        first_week: NaiveDate,
        weeks: usize,
        pinned: &[Uuid],
        reveal: &mut MarkerReveal,
    ) -> Self {
        let entries: Vec<_> = entries.into_iter().collect();
        reveal.reconcile(&entries);
        let mut seen = HashSet::new();
        let markers: Vec<_> = entries
            .into_iter()
            .filter(|(marker, _)| seen.insert(marker.id))
            .map(|(marker, projected)| CalendarMarker { marker, projected })
            .collect();
        let rank = |entry: &CalendarMarker| {
            if pinned.contains(&entry.marker.id) {
                0
            } else if reveal.revealed == Some(entry.marker.id) {
                1
            } else {
                2
            }
        };

        let mut layout = HashMap::new();
        for week in 0..weeks {
            let start = first_week + chrono::Duration::weeks(week as i64);
            let end = start + chrono::Duration::days(6);
            let mut candidates: Vec<_> = markers
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| {
                    let first = entry.marker.date.max(start);
                    let last = entry.last_date().min(end);
                    (first <= last).then(|| (index, InclusiveDateRange::new(first, last)))
                })
                .collect();
            candidates.sort_by_key(|(index, _)| {
                let entry = &markers[*index];
                (
                    rank(entry),
                    std::cmp::Reverse(entry.day_count()),
                    entry.marker.date,
                    entry.projected,
                    entry.marker.id,
                )
            });
            let mut rows: Vec<u8> = Vec::new();
            let spans = candidates
                .into_iter()
                .map(|(entry, dates)| {
                    let days = (dates.start() - start).num_days() as u32
                        ..=(dates.end() - start).num_days() as u32;
                    let mask = days.fold(0u8, |mask, day| mask | 1 << day);
                    let lane = rows
                        .iter()
                        .position(|row| row & mask == 0)
                        .unwrap_or_else(|| {
                            rows.push(0);
                            rows.len() - 1
                        });
                    rows[lane] |= mask;
                    PlacedSpan { entry, dates, lane }
                })
                .collect();
            layout.insert(start, spans);
        }
        Self {
            markers,
            weeks: layout,
        }
    }

    fn spans_on(&self, date: NaiveDate) -> impl Iterator<Item = &PlacedSpan> {
        self.weeks
            .get(&week_start(date))
            .into_iter()
            .flatten()
            .filter(move |span| span.dates.contains(date))
    }

    pub(super) fn week(&self, start: NaiveDate) -> Vec<MarkerSpan<'_>> {
        self.weeks
            .get(&start)
            .into_iter()
            .flatten()
            .map(|span| MarkerSpan {
                entry: &self.markers[span.entry],
                dates: span.dates,
                lane: span.lane,
            })
            .collect()
    }

    pub(super) fn hidden_on(&self, date: NaiveDate, capacity: usize) -> Vec<Marker> {
        self.spans_on(date)
            .filter(|span| span.lane >= capacity)
            .map(|span| self.markers[span.entry].marker.clone())
            .collect()
    }

    pub(super) fn visible_lanes_on(&self, date: NaiveDate, capacity: usize) -> usize {
        self.spans_on(date)
            .filter(|span| span.lane < capacity)
            .map(|span| span.lane + 1)
            .max()
            .unwrap_or(0)
    }

    pub(super) fn lane_on(&self, id: Uuid, date: NaiveDate) -> Option<usize> {
        self.weeks
            .get(&week_start(date))?
            .iter()
            .find(|span| self.markers[span.entry].marker.id == id)
            .map(|span| span.lane)
    }

    pub(super) fn is_hidden_anywhere(&self, id: Uuid, capacity: usize) -> bool {
        self.weeks
            .values()
            .flatten()
            .any(|span| span.lane >= capacity && self.markers[span.entry].marker.id == id)
    }

    pub(super) fn order(&self, start: NaiveDate, end: NaiveDate, cx: &App) -> SelectionOrder {
        let manager = ItemManager::global(cx);
        let mut entries: Vec<_> = self
            .markers
            .iter()
            .filter(|entry| {
                !entry.projected
                    && !manager.read(cx).is_draft(entry.marker.id)
                    && entry.marker.date < end
                    && entry.last_date() >= start
            })
            .map(|entry| {
                let first = entry.marker.date.max(start);
                let lane = self.lane_on(entry.marker.id, first).unwrap_or(0);
                (entry.marker.date, lane, entry.marker.id)
            })
            .collect();
        entries.sort();
        SelectionOrder::new(
            SelectionScope::Calendar,
            entries.into_iter().map(|(_, _, id)| id),
        )
    }
}

pub(super) fn marker_preview(mut marker: Marker, cx: &App) -> Marker {
    let manager = ItemManager::global(cx);
    if let Some(editing) = manager
        .read(cx)
        .editing_item
        .as_ref()
        .filter(|editing| editing.id() == marker.id)
    {
        marker.title = editing
            .parsed
            .as_ref()
            .ok()
            .and_then(Option::as_ref)
            .map_or_else(
                || editing.value(cx).to_string(),
                |parsed| parsed.title.clone(),
            );
    }
    marker
}

struct MarkerDragRegion {
    date: NaiveDate,
    left: Pixels,
    width: Pixels,
}

impl MarkerDragRegion {
    fn payload(&self, marker: &Marker) -> DraggedItems {
        let mut dragged = DraggedItems::single(AnyItem::Marker(marker.clone()));
        dragged.source_anchor = Some(SchedulePoint::Date(self.date));
        dragged
    }
}

fn marker_drag_regions(
    dates: InclusiveDateRange,
    day_width: Pixels,
    left_inset: Pixels,
    right_inset: Pixels,
) -> Vec<MarkerDragRegion> {
    (0..dates.day_count())
        .filter_map(|offset| {
            let left = day_width * offset as f32 + if offset == 0 { left_inset } else { px(0.) };
            let right = day_width * (offset + 1) as f32
                - if offset + 1 == dates.day_count() {
                    right_inset
                } else {
                    px(0.)
                };
            (right > left).then(|| MarkerDragRegion {
                date: dates.start() + chrono::Duration::days(i64::from(offset)),
                left,
                width: right - left,
            })
        })
        .collect()
}

impl CalendarView {
    pub(super) fn render_marker_span(
        &self,
        span: MarkerSpan<'_>,
        order: &SelectionOrder,
        top: Pixels,
        day_width: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let marker = &span.entry.marker;
        let first = span.dates.start();
        let last = span.dates.end();
        let continues_before = first > marker.date;
        let continues_after = last < marker.end_date.unwrap_or(marker.date);
        let left_inset = if continues_before { px(0.) } else { END_INSET };
        let right_inset = if continues_after { px(0.) } else { END_INSET };
        let projected = span.entry.projected;
        let manager = ItemManager::global(cx);
        let is_draft = manager.read(cx).is_draft(marker.id);
        let is_editing = manager.read(cx).is_being_edited(marker.id);
        let is_resizing = self
            .marker_resize
            .as_ref()
            .is_some_and(|resize| resize.item_id == marker.id);
        let can_resize = !projected && !is_draft && !is_editing && self.marker_resize.is_none();
        let item = AnyItem::Marker(marker.clone());
        let card = ItemCard::new_with_id(
            format!("calendar-marker-{}-{first}", marker.id),
            &item,
            None,
            window,
            cx,
        );
        let focus = Focusable::focus_handle(&card, cx);
        let card = card
            .title_only(true)
            .compact(true)
            .horizontal_position(ItemCardPosition::from_neighbors(
                continues_before,
                continues_after,
            ))
            .actionable(!projected && !is_draft)
            .when(!projected && !is_draft, |card| {
                card.selectable(order.clone())
            })
            .editable(!projected && !continues_before)
            .editor_width((day_width - END_INSET - px(12.)).max(px(0.)))
            .size_full()
            .when(projected, |card| card.opacity(0.55))
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_pressure(|_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .on_drag_move::<DragData<DraggedItems>>(cx.listener(
                |view, event: &DragMoveEvent<DragData<DraggedItems>>, _, cx| {
                    if let Some(date) = view.date_at_position(event.event.position) {
                        view.set_drop_target(date, event.event.position, cx);
                    }
                },
            ))
            .on_drop::<DragData<DraggedItems>>(cx.listener(
                |view, data: &DragData<DraggedItems>, window, cx| {
                    if let Some(date) = view.date_at_position(window.mouse_position()) {
                        view.commit_date_drop(&data.data, date, window, cx);
                        cx.stop_propagation();
                    }
                },
            ));
        let drag_regions = if !projected && !is_draft && !is_editing && self.marker_resize.is_none()
        {
            marker_drag_regions(span.dates, day_width, left_inset, right_inset)
        } else {
            Vec::new()
        };
        div()
            .absolute()
            .top(top + MARKER_PITCH * span.lane as f32)
            .left(relative(first.weekday().num_days_from_sunday() as f32 / 7.))
            .right(relative(
                (6 - last.weekday().num_days_from_sunday()) as f32 / 7.,
            ))
            .h(MARKER_HEIGHT)
            .pl(left_inset)
            .pr(right_inset)
            .when(
                !projected && !is_draft && !is_editing && !is_resizing,
                |band| {
                    band.capture_any_mouse_down(move |_, window, cx| {
                        SelectionManager::claim_press(cx);
                        focus.focus(window, cx);
                    })
                },
            )
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .capture_any_mouse_up(cx.listener(|view, event: &MouseUpEvent, _, cx| {
                if event.button == MouseButton::Left {
                    if view.date_range_drag.is_some() {
                        view.finish_date_range_drag_at(event.position, cx);
                    }
                    if view.item_drag_active {
                        view.clear_drop_target(cx);
                    }
                }
            }))
            .child(card)
            // Siblings use the full band geometry, not the card's padded content box.
            .children(drag_regions.into_iter().map(|region| {
                let drag = create_drag_data(
                    region.payload(marker),
                    size(day_width.max(px(120.)), MARKER_HEIGHT),
                    cx,
                );
                div()
                    .id(format!(
                        "calendar-marker-drag-{}-{}",
                        marker.id, region.date
                    ))
                    .absolute()
                    .top_0()
                    .left(region.left)
                    .w(region.width)
                    .h_full()
                    .cursor(gpui::CursorStyle::OpenHand)
                    .on_drag(drag, |drag, position, _, cx| {
                        cx.new(|_| drag.clone().with_position(position))
                    })
                    .drag_hold(DRAG_HOLD)
                    .on_drop::<DragData<DraggedItems>>(cx.listener(
                        |view, data: &DragData<DraggedItems>, window, cx| {
                            if let Some(date) = view.date_at_position(window.mouse_position()) {
                                view.commit_date_drop(&data.data, date, window, cx);
                                cx.stop_propagation();
                            }
                        },
                    ))
            }))
            .when(can_resize && !continues_before, |band| {
                band.child(Self::render_marker_resize_handle(
                    marker.id,
                    MarkerResizeEdge::Start,
                    cx,
                ))
            })
            .when(can_resize && !continues_after, |band| {
                band.child(Self::render_marker_resize_handle(
                    marker.id,
                    MarkerResizeEdge::End,
                    cx,
                ))
            })
            .into_any_element()
    }

    pub(super) fn render_marker_overflow(
        &self,
        date: NaiveDate,
        hidden: Vec<Marker>,
        cx: &Context<Self>,
    ) -> Button {
        Button::new(("calendar-marker-overflow", date.num_days_from_ce() as u32))
            .ghost()
            .compact()
            .h(MARKER_HEIGHT)
            .min_w_0()
            .px_1()
            .label(if self.day_width() < px(80.) {
                format!("+{}", hidden.len())
            } else {
                format!("+{} markers", hidden.len())
            })
            .tooltip("Show hidden markers")
            .aria_label(format!(
                "Show {} hidden markers on {}",
                hidden.len(),
                date.format("%B %-d")
            ))
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
            .on_click(cx.listener(move |_, event: &ClickEvent, window, cx| {
                cx.stop_propagation();
                let mut menu = MenuBuilder::new().label("Hidden markers");
                for marker in &hidden {
                    let id = marker.id;
                    let view = cx.entity();
                    menu = menu.item(marker.title.clone(), move |_, cx| {
                        view.update(cx, |view, cx| {
                            view.marker_reveal.reveal(id);
                            cx.notify();
                        });
                    });
                }
                crate::components::menu::open_context_menu(menu, event.position(), window, cx);
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
    }

    fn layout(
        entries: &[(Marker, bool)],
        pinned: &[Uuid],
        reveal: &mut MarkerReveal,
    ) -> MarkerLayout {
        MarkerLayout::new(entries.to_vec(), date(4), 3, pinned, reveal)
    }

    fn single(title: &str, day: u32) -> (Marker, bool) {
        (Marker::new(title, date(day)), false)
    }

    fn span(title: &str, first: u32, last: u32) -> (Marker, bool) {
        (
            Marker::new(title, date(first)).with_end_date(date(last)),
            false,
        )
    }

    fn assert_no_overlap(layout: &MarkerLayout) {
        for spans in layout.weeks.values() {
            for (index, span) in spans.iter().enumerate() {
                for other in &spans[index + 1..] {
                    assert!(
                        span.lane != other.lane
                            || span.dates.end() < other.dates.start()
                            || other.dates.end() < span.dates.start()
                    );
                }
            }
        }
    }

    #[test]
    fn marker_budget_reserves_a_gap_for_both_previews_and_footer_controls() {
        assert_eq!(marker_content_height(0), px(0.));
        assert_eq!(marker_content_height(1), MARKER_HEIGHT + px(4.));
        assert_eq!(
            marker_content_height(3),
            MARKER_PITCH * 2. + MARKER_HEIGHT + px(4.)
        );
        for height in [0., 27., 28., 29., 53., 54., 99.] {
            let capacity = marker_capacity(px(height));
            assert!(marker_content_height(capacity) <= px(height));
            assert!(marker_content_height(capacity + 1) > px(height));
        }
    }

    #[test]
    fn crowded_single_days_never_hide_a_multi_day_marker() {
        let trip = span("Trip", 12, 16);
        let mut entries: Vec<_> = (0..5)
            .map(|index| single(&format!("Day {index}"), 14))
            .collect();
        entries.push(trip.clone());
        let layout = layout(&entries, &[], &mut MarkerReveal::default());
        let capacity = 2;
        for day in 12..=16 {
            assert_eq!(layout.lane_on(trip.0.id, date(day)), Some(0));
        }
        assert_eq!(layout.visible_lanes_on(date(12), capacity), 1);
        assert_eq!(layout.visible_lanes_on(date(14), capacity), 2);
        let hidden = layout.hidden_on(date(14), capacity);
        assert_eq!(hidden.len(), 4);
        assert!(hidden.iter().all(|marker| marker.id != trip.0.id));
        assert!(layout.hidden_on(date(12), capacity).is_empty());
        assert_no_overlap(&layout);
    }

    #[test]
    fn rows_are_packed_independently_in_each_week() {
        let long = span("Long", 5, 10);
        let crossing = span("Crossing", 9, 13);
        let layout = layout(&[crossing.clone(), long], &[], &mut MarkerReveal::default());
        assert_eq!(layout.lane_on(crossing.0.id, date(9)), Some(1));
        assert_eq!(layout.lane_on(crossing.0.id, date(12)), Some(0));
        assert_eq!(layout.visible_lanes_on(date(12), 3), 1);
        assert_no_overlap(&layout);
    }

    #[test]
    fn revealing_lifts_a_marker_only_in_its_own_week() {
        let mut entries: Vec<_> = (0..5)
            .map(|index| span(&format!("Marker {index}"), 11, 15))
            .collect();
        let elsewhere = span("Elsewhere", 19, 21);
        entries.push(elsewhere.clone());
        let mut reveal = MarkerReveal::default();
        let capacity = 1;
        let hidden = layout(&entries, &[], &mut reveal).hidden_on(date(13), capacity)[0].id;

        reveal.reveal(hidden);
        let revealed = layout(&entries, &[], &mut reveal);
        assert_eq!(revealed.lane_on(hidden, date(13)), Some(0));
        assert_eq!(revealed.hidden_on(date(13), capacity).len(), 4);
        assert_eq!(revealed.lane_on(elsewhere.0.id, date(20)), Some(0));
        assert_no_overlap(&revealed);

        let extended = entries
            .iter()
            .position(|(marker, _)| marker.id != hidden && marker.id != elsewhere.0.id)
            .unwrap();
        entries[extended].0 = entries[extended].0.clone().with_end_date(date(17));
        let changed = layout(&entries, &[], &mut reveal);
        assert_eq!(changed.lane_on(entries[extended].0.id, date(13)), Some(0));
        assert!(changed.is_hidden_anywhere(hidden, capacity));
    }

    #[test]
    fn pinned_markers_lead_their_weeks() {
        let short = single("Editing", 14);
        let long = span("Long", 12, 16);
        let unpinned = layout(
            &[short.clone(), long.clone()],
            &[],
            &mut MarkerReveal::default(),
        );
        assert!(unpinned.is_hidden_anywhere(short.0.id, 1));

        let pinned = layout(
            &[short.clone(), long.clone()],
            &[short.0.id],
            &mut MarkerReveal::default(),
        );
        assert_eq!(pinned.lane_on(short.0.id, date(14)), Some(0));
        assert_eq!(pinned.lane_on(long.0.id, date(14)), Some(1));
        assert!(!pinned.is_hidden_anywhere(short.0.id, 1));
    }

    #[test]
    fn drag_regions_register_the_pressed_day_across_week_boundaries() {
        let marker = Marker::new("Trip", date(10)).with_end_date(date(13));
        let layout = layout(
            &[(marker.clone(), false)],
            &[],
            &mut MarkerReveal::default(),
        );
        let first = marker_drag_regions(layout.week(date(4))[0].dates, px(100.), END_INSET, px(0.));
        assert_eq!(
            (first[0].date, first[0].left, first[0].width),
            (date(10), px(4.), px(96.))
        );

        let regions =
            marker_drag_regions(layout.week(date(11))[0].dates, px(100.), px(0.), END_INSET);
        assert_eq!(
            regions
                .iter()
                .map(|region| (region.date, region.left, region.width))
                .collect::<Vec<_>>(),
            vec![
                (date(11), px(0.), px(100.)),
                (date(12), px(100.), px(100.)),
                (date(13), px(200.), px(96.)),
            ]
        );
        let at = |x: Pixels| {
            regions
                .iter()
                .find(|region| region.left <= x && x < region.left + region.width)
                .unwrap()
        };
        assert_eq!(at(px(99.5)).date, date(11));
        assert_eq!(at(px(100.)).date, date(12));
        assert_eq!(at(px(200.)).date, date(13));

        let pressed = at(px(199.5)).payload(&marker);
        let preview = DragData::new(pressed.clone()).with_position(gpui::point(px(200.5), px(8.)));
        assert_eq!(pressed.source_anchor, Some(SchedulePoint::Date(date(12))));
        assert_eq!(preview.data.source_anchor, pressed.source_anchor);
        for (dropped, expected_start, expected_end) in
            [(date(16), date(14), date(17)), (date(8), date(6), date(9))]
        {
            let delta = dropped - NaiveDate::from(pressed.source_anchor().unwrap());
            assert_eq!(
                pressed.primary.start_date().unwrap() + delta,
                expected_start
            );
            assert_eq!(marker.end_date.unwrap() + delta, expected_end);
        }
    }

    #[test]
    fn mixed_markers_never_overlap() {
        let entries = [
            span("A", 5, 6),
            span("B", 8, 9),
            span("C", 6, 8),
            span("D", 4, 12),
            single("E", 7),
            span("F", 10, 19),
            (
                Marker::new("Projected", date(7)).with_end_date(date(8)),
                true,
            ),
        ];
        let layout = layout(&entries, &[], &mut MarkerReveal::default());
        assert_eq!(layout.lane_on(entries[5].0.id, date(10)), Some(0));
        assert_eq!(layout.lane_on(entries[3].0.id, date(7)), Some(1));
        assert_no_overlap(&layout);
    }

    #[test]
    fn marker_has_one_connected_span_per_week() {
        let marker = Marker::new("Trip", date(9)).with_end_date(date(19));
        let layout = MarkerLayout::new(
            [(marker.clone(), false), (marker.clone(), false)],
            date(4),
            4,
            &[],
            &mut MarkerReveal::default(),
        );
        for (week, first, last) in [(4, 9, 10), (11, 11, 17), (18, 18, 19)] {
            let spans = layout.week(date(week));
            assert_eq!(spans.len(), 1);
            assert_eq!(
                spans[0].dates,
                InclusiveDateRange::new(date(first), date(last))
            );
            assert_eq!(spans[0].entry.marker.date, marker.date);
            assert_eq!(spans[0].lane, 0);
        }
        assert!(layout.week(date(25)).is_empty());
    }
}
