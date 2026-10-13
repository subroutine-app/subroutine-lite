use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{Datelike, NaiveDate};
use gpui::{
    AnyElement, App, AppContext, ClickEvent, Context, DragMoveEvent, Focusable, IntoElement,
    MouseButton, MouseUpEvent, ParentElement, Pixels, Styled, Window, div,
    prelude::FluentBuilder as _, px, relative, size,
};
use gpui::{InteractiveElement, StatefulInteractiveElement};
use subroutine_core::{AnyItem, Marker, SchedulePoint};
use uuid::Uuid;

use crate::{
    components::{DragData, DraggedItems, ItemCard, ItemCardPosition, create_drag_data},
    dates::InclusiveDateRange,
    item_manager::ItemManager,
    selection::{SelectionManager, SelectionOrder, SelectionScope},
};

use super::CalendarView;

pub(super) const MARKER_HEIGHT: Pixels = px(24.);
pub(super) const MARKER_PITCH: Pixels = px(26.);
const END_INSET: Pixels = px(4.);

pub(super) struct CalendarMarker {
    pub marker: Marker,
    pub projected: bool,
    lane: usize,
}

pub(super) struct MarkerLayout {
    markers: Vec<CalendarMarker>,
}

pub(super) struct MarkerSpan<'a> {
    entry: &'a CalendarMarker,
    dates: InclusiveDateRange,
}

#[derive(Default)]
struct LaneOccupancy(HashMap<usize, BTreeMap<NaiveDate, NaiveDate>>);

impl LaneOccupancy {
    fn reserve(&mut self, lane: usize, marker: &Marker) -> bool {
        let dates = self.0.entry(lane).or_default();
        let end = marker.end_date.unwrap_or(marker.date).max(marker.date);
        // Reserved intervals are disjoint; only the last start before this end can overlap.
        if dates
            .range(..=end)
            .next_back()
            .is_some_and(|(_, other_end)| *other_end >= marker.date)
        {
            return false;
        }
        dates.insert(marker.date, end);
        true
    }
}

impl MarkerLayout {
    pub(super) fn new(
        entries: impl IntoIterator<Item = (Marker, bool)>,
        cache: &mut HashMap<Uuid, usize>,
    ) -> Self {
        let mut entries: Vec<_> = entries.into_iter().collect();
        entries.sort_by_key(|(marker, projected)| {
            (
                marker.date,
                std::cmp::Reverse(marker.end_date.unwrap_or(marker.date)),
                *projected,
                marker.id,
            )
        });
        let mut lanes = LaneOccupancy::default();
        let mut markers = Vec::new();
        let mut pending = Vec::new();
        let mut seen = HashSet::new();
        for (marker, projected) in entries {
            if !seen.insert(marker.id) {
                continue;
            }
            if let Some(lane) = cache
                .get(&marker.id)
                .copied()
                .filter(|lane| lanes.reserve(*lane, &marker))
            {
                markers.push(CalendarMarker {
                    marker,
                    projected,
                    lane,
                });
            } else {
                pending.push((marker, projected));
            }
        }
        for (marker, projected) in pending {
            let mut lane = 0;
            while !lanes.reserve(lane, &marker) {
                lane += 1;
            }
            markers.push(CalendarMarker {
                marker,
                projected,
                lane,
            });
        }
        markers.sort_by_key(|entry| (entry.lane, entry.marker.date, entry.marker.id));
        cache.retain(|id, _| seen.contains(id));
        cache.extend(markers.iter().map(|entry| (entry.marker.id, entry.lane)));
        Self { markers }
    }

    pub(super) fn week(&self, start: NaiveDate) -> Vec<MarkerSpan<'_>> {
        let end = start + chrono::Duration::days(6);
        self.markers
            .iter()
            .filter_map(|entry| {
                let first = entry.marker.date.max(start);
                let last = entry.marker.end_date.unwrap_or(entry.marker.date).min(end);
                (first <= last).then(|| MarkerSpan {
                    entry,
                    dates: InclusiveDateRange::new(first, last),
                })
            })
            .collect()
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
                    && entry.marker.end_date.unwrap_or(entry.marker.date) >= start
            })
            .collect();
        entries.sort_by_key(|entry| (entry.marker.date, entry.lane, entry.marker.id));
        SelectionOrder::new(
            SelectionScope::Calendar,
            entries.into_iter().map(|entry| entry.marker.id),
        )
    }

    pub(super) fn lanes_on(&self, date: NaiveDate) -> usize {
        self.markers
            .iter()
            .filter(|entry| entry.marker.covers(date))
            .map(|entry| entry.lane + 1)
            .max()
            .unwrap_or(0)
    }

    pub(super) fn lanes_between(&self, start: NaiveDate, end: NaiveDate) -> usize {
        self.markers
            .iter()
            .filter(|entry| {
                entry.marker.date <= end
                    && entry.marker.end_date.unwrap_or(entry.marker.date) >= start
            })
            .map(|entry| entry.lane + 1)
            .max()
            .unwrap_or(0)
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
        let item = AnyItem::Marker(marker.clone());
        let edit_marker = marker.clone();
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
            .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                cx.stop_propagation();
                if event.click_count() >= 2 && !projected && !is_draft {
                    view.edit_grid_marker(&edit_marker, window, cx);
                }
            }))
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
        let drag_regions = if !projected && !is_draft && !is_editing {
            marker_drag_regions(span.dates, day_width, left_inset, right_inset)
        } else {
            Vec::new()
        };
        div()
            .absolute()
            .top(top + MARKER_PITCH * span.entry.lane as f32)
            .left(relative(first.weekday().num_days_from_sunday() as f32 / 7.))
            .right(relative(
                (6 - last.weekday().num_days_from_sunday()) as f32 / 7.,
            ))
            .h(MARKER_HEIGHT)
            .pl(left_inset)
            .pr(right_inset)
            .when(!projected && !is_draft && !is_editing, |band| {
                band.capture_any_mouse_down(move |_, window, cx| {
                    SelectionManager::claim_press(cx);
                    focus.focus(window, cx);
                })
            })
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
                    .on_drop::<DragData<DraggedItems>>(cx.listener(
                        |view, data: &DragData<DraggedItems>, window, cx| {
                            if let Some(date) = view.date_at_position(window.mouse_position()) {
                                view.commit_date_drop(&data.data, date, window, cx);
                                cx.stop_propagation();
                            }
                        },
                    ))
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
    }

    fn lane(layout: &MarkerLayout, id: Uuid) -> usize {
        layout
            .markers
            .iter()
            .find(|entry| entry.marker.id == id)
            .unwrap()
            .lane
    }

    fn assert_no_overlap(layout: &MarkerLayout) {
        for (index, entry) in layout.markers.iter().enumerate() {
            for other in &layout.markers[index + 1..] {
                if entry.lane == other.lane {
                    assert!(
                        entry.marker.end_date.unwrap_or(entry.marker.date) < other.marker.date
                            || other.marker.end_date.unwrap_or(other.marker.date)
                                < entry.marker.date
                    );
                }
            }
        }
    }

    #[test]
    fn drag_regions_register_the_pressed_day_across_week_boundaries() {
        let marker = Marker::new("Trip", date(10)).with_end_date(date(13));
        let layout = MarkerLayout::new([(marker.clone(), false)], &mut HashMap::new());
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
    fn surviving_markers_keep_lanes_when_projections_change() {
        let projected = Marker::new("Occurrence", date(5)).with_end_date(date(6));
        let marker = Marker::new("Long marker", date(6)).with_end_date(date(20));
        let next = Marker::new("Next occurrence", date(12)).with_end_date(date(13));
        let mut cache = HashMap::new();
        let first = MarkerLayout::new(
            [(projected.clone(), true), (marker.clone(), false)],
            &mut cache,
        );
        assert_eq!(lane(&first, marker.id), 1);

        let scrolled =
            MarkerLayout::new([(marker.clone(), false), (next.clone(), true)], &mut cache);
        assert_eq!(lane(&scrolled, marker.id), lane(&first, marker.id));
        assert_eq!(lane(&scrolled, next.id), 0);
        assert!(!cache.contains_key(&projected.id));
        assert_no_overlap(&scrolled);

        let returned = MarkerLayout::new(
            [
                (next.clone(), true),
                (projected, true),
                (marker.clone(), false),
            ],
            &mut cache,
        );
        assert_eq!(lane(&returned, marker.id), lane(&first, marker.id));
        assert_eq!(lane(&returned, next.id), lane(&scrolled, next.id));
        assert_no_overlap(&returned);
    }

    #[test]
    fn cached_lanes_are_repaired_before_new_markers_are_placed() {
        let a = Marker::new("A", date(5)).with_end_date(date(6));
        let b = Marker::new("B", date(8)).with_end_date(date(9));
        let c = Marker::new("C", date(6)).with_end_date(date(8));
        let mut cache = HashMap::new();
        let first = MarkerLayout::new(
            [(a.clone(), false), (b.clone(), false), (c.clone(), false)],
            &mut cache,
        );
        assert_eq!(
            (lane(&first, a.id), lane(&first, b.id), lane(&first, c.id)),
            (0, 0, 1)
        );
        assert_no_overlap(&first);

        let a = a.with_end_date(date(10));
        let changed = MarkerLayout::new(
            [(a.clone(), false), (b.clone(), false), (c.clone(), false)],
            &mut cache,
        );
        assert_eq!(
            (
                lane(&changed, a.id),
                lane(&changed, b.id),
                lane(&changed, c.id)
            ),
            (0, 2, 1)
        );
        assert_no_overlap(&changed);

        let added = Marker::new("Earlier new marker", date(4)).with_end_date(date(12));
        let with_new = MarkerLayout::new(
            [
                (added.clone(), false),
                (c.clone(), false),
                (b.clone(), false),
                (a.clone(), false),
            ],
            &mut cache,
        );
        for id in [a.id, b.id, c.id] {
            assert_eq!(lane(&with_new, id), lane(&changed, id));
        }
        assert_eq!(lane(&with_new, added.id), 3);
        assert_no_overlap(&with_new);
    }

    #[test]
    fn marker_has_one_connected_span_per_week() {
        let marker = Marker::new("Trip", date(9)).with_end_date(date(19));
        let layout = MarkerLayout::new(
            [(marker.clone(), false), (marker.clone(), false)],
            &mut HashMap::new(),
        );
        for (week, first, last) in [(4, 9, 10), (11, 11, 17), (18, 18, 19)] {
            let spans = layout.week(date(week));
            assert_eq!(spans.len(), 1);
            assert_eq!(
                spans[0].dates,
                InclusiveDateRange::new(date(first), date(last))
            );
            assert_eq!(spans[0].entry.marker.date, marker.date);
            assert_eq!(spans[0].entry.lane, 0);
        }
        assert!(layout.week(date(25)).is_empty());
    }
}
