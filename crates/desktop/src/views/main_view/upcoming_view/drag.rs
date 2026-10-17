use chrono::Local;
use chronoutil::RelativeDuration;
use gpui::{
    AnyElement, App, Bounds, Context, DragMoveEvent, IntoElement, ParentElement, Pixels, Point,
    Styled, div, prelude::FluentBuilder as _, px,
};
use gpui_kit_theme::ActiveTheme;
use subroutine_core::{AnyItem, SchedulePoint};

use crate::{
    components::{DragData, DraggedItems},
    settings::Settings,
    views::{LIST_VIEW_MAX_WIDTH, LIST_VIEW_MIN_WIDTH},
};

use super::{
    UpcomingDropTarget, UpcomingSection, UpcomingView,
    agenda::{Agenda, AgendaLayout, AgendaLayoutRow, DropSlot, UpcomingRow, row_top_inset},
    dropped_starts, effective_duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DropGap {
    before: usize,
    section: UpcomingSection,
}

pub(super) struct UpcomingDrag {
    pub data: DraggedItems,
    heights: Vec<Pixels>,
    grab_y: Pixels,
    origin: Option<DropGap>,
    gap: Option<DropGap>,
}

impl UpcomingDrag {
    fn contains(&self, item: &AnyItem) -> bool {
        self.data
            .items
            .iter()
            .any(|dragged| dragged.id() == item.id())
    }

    fn retarget(&mut self, hit: Option<DropHit>) -> Option<UpcomingDropTarget> {
        let hit = hit?;
        if let Some(gap) = hit.gap {
            self.gap = Some(gap);
            if self.origin == Some(gap) {
                return None;
            }
        }
        Some(hit.target)
    }

    fn slots(&self) -> Vec<DropSlot> {
        let Some(gap) = self.gap else {
            return Vec::new();
        };
        self.heights
            .iter()
            .enumerate()
            .map(|(index, height)| DropSlot {
                before: gap.before,
                section: gap.section,
                height: *height,
                index,
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DropHit {
    target: UpcomingDropTarget,
    gap: Option<DropGap>,
}

fn day_hit(section: UpcomingSection) -> Option<DropHit> {
    match section {
        UpcomingSection::Day(date) => Some(DropHit {
            target: UpcomingDropTarget::Day(date),
            gap: None,
        }),
        UpcomingSection::Missed => None,
    }
}

fn day_bounds(mut bounds: Bounds<Pixels>) -> Bounds<Pixels> {
    let width = bounds
        .size
        .width
        .clamp(LIST_VIEW_MIN_WIDTH, LIST_VIEW_MAX_WIDTH);
    bounds.origin.x += ((bounds.size.width - width) / 2.).max(px(0.));
    bounds.size.width = width;
    bounds
}

fn before_target(item: &AnyItem) -> Option<UpcomingDropTarget> {
    let Some(SchedulePoint::DateTime(start)) = item.start() else {
        return None;
    };
    Some(UpcomingDropTarget::Before {
        date: item.start_date()?,
        item: item.id(),
        anchor: Some(start),
    })
}

fn after_target(item: &AnyItem, duration: RelativeDuration) -> Option<UpcomingDropTarget> {
    let Some(SchedulePoint::DateTime(start)) = item.start() else {
        return None;
    };
    Some(UpcomingDropTarget::After {
        date: item.start_date()?,
        item: item.id(),
        anchor: Some(start + effective_duration(item, start, duration)),
    })
}

fn remaining_rows<'a>(
    agenda: &'a Agenda,
    layout: &'a AgendaLayout,
    drag: &'a UpcomingDrag,
) -> impl Iterator<Item = (usize, Pixels)> + 'a {
    layout
        .rows
        .iter()
        .zip(layout.sizes.iter())
        .filter_map(move |(row, size)| {
            let source = row.source()?;
            if matches!(&agenda.rows[source], UpcomingRow::Item { item, .. } if drag.contains(item))
            {
                return None;
            }
            Some((source, size.height))
        })
}

fn origin_gap(agenda: &Agenda, layout: &AgendaLayout, drag: &UpcomingDrag) -> Option<DropGap> {
    let (source, section) = layout.rows.iter().find_map(|row| {
        let source = row.source()?;
        match &agenda.rows[source] {
            UpcomingRow::Item { item, section, .. } if drag.contains(item) => {
                Some((source, *section))
            }
            _ => None,
        }
    })?;
    if !drag.data.items.iter().enumerate().all(|(index, dragged)| {
        matches!(agenda.rows.get(source + index), Some(UpcomingRow::Item { item, section: other, .. })
            if item.id() == dragged.id() && *other == section)
    }) {
        return None;
    }
    Some(DropGap {
        before: remaining_rows(agenda, layout, drag)
            .find(|(index, _)| *index > source)
            .map_or(agenda.rows.len(), |(index, _)| index),
        section,
    })
}

fn insertion_hit(
    agenda: &Agenda,
    layout: &AgendaLayout,
    drag: &UpcomingDrag,
    section: UpcomingSection,
    content_y: Pixels,
    duration: RelativeDuration,
) -> Option<DropHit> {
    let desired_top = content_y - drag.grab_y;
    let mut rows = remaining_rows(agenda, layout, drag).peekable();
    let mut top = px(0.);
    let mut nearest: Option<(Pixels, bool, DropHit)> = None;
    while let Some((source, height)) = rows.next() {
        if let UpcomingRow::Item {
            item,
            section: item_section,
            ..
        } = &agenda.rows[source]
            && *item_section == section
        {
            let before = before_target(item).map(|target| (top, source, true, target));
            let after = after_target(item, duration).map(|target| {
                (
                    top + height,
                    rows.peek().map_or(agenda.rows.len(), |(source, _)| *source),
                    false,
                    target,
                )
            });
            for (edge, before, leading, target) in before.into_iter().chain(after) {
                let distance = (desired_top - edge).abs();
                let preferred = leading == (desired_top >= edge);
                if nearest.as_ref().is_none_or(|(best, tie, _)| {
                    distance < *best || (distance == *best && preferred && !tie)
                }) {
                    nearest = Some((
                        distance,
                        preferred,
                        DropHit {
                            target,
                            gap: Some(DropGap { before, section }),
                        },
                    ));
                }
            }
        }
        top += height;
    }
    nearest.map(|(_, _, hit)| hit)
}

impl UpcomingView {
    pub(super) fn drop_slots(&self) -> Vec<DropSlot> {
        self.dragged_items
            .as_ref()
            .map_or_else(Vec::new, UpcomingDrag::slots)
    }

    fn drop_hit_at(&self, position: Point<Pixels>, cx: &App) -> Option<DropHit> {
        let drag = self.dragged_items.as_ref()?;
        let (ix, bounds) = self.scroll_handle.row_at_position(position)?;
        let mut bounds = day_bounds(bounds);
        let row = *self.layout.rows.get(ix)?;
        let content_y = position.y - bounds.top()
            + self
                .layout
                .sizes
                .iter()
                .take(ix)
                .map(|size| size.height)
                .sum::<Pixels>();
        let inset = row_top_inset(ix);
        bounds.origin.y += inset;
        bounds.size.height -= inset;
        if !bounds.contains(&position) {
            return None;
        }
        let section = row.section(&self.agenda);
        if position.x < bounds.left() + px(16.) || position.x >= bounds.right() - px(16.) {
            return day_hit(section);
        }
        match row {
            AgendaLayoutRow::Source(source) => match &self.agenda.rows[source] {
                UpcomingRow::Missed => return None,
                UpcomingRow::Day { .. } | UpcomingRow::Create { .. } => return day_hit(section),
                UpcomingRow::Item { item, .. } => {
                    if drag.contains(item) {
                        return None;
                    }
                    if !self.row_interactive(ix) {
                        return day_hit(section);
                    }
                }
            },
            AgendaLayoutRow::Vacancy(_) => return None,
            AgendaLayoutRow::Drop { .. } => {}
        }
        if drag.data.source_anchor.is_some()
            || drag
                .data
                .items
                .iter()
                .any(|item| matches!(item, AnyItem::Marker(_) | AnyItem::Routine(_)))
        {
            return day_hit(section);
        }
        insertion_hit(
            &self.agenda,
            &self.layout,
            drag,
            section,
            content_y,
            Settings::global(cx).schedule.default_action_duration,
        )
        .or_else(|| day_hit(section))
    }

    pub(super) fn retarget_drop(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let hit = self.drop_hit_at(position, cx);
        let Some(drag) = self.dragged_items.as_mut() else {
            return;
        };
        let previous = drag.gap;
        let target = drag.retarget(hit);
        if drag.gap != previous {
            cx.notify();
        }
        self.set_drop_target_option(target, cx);
    }

    pub(super) fn handle_drag_move(
        &mut self,
        event: &DragMoveEvent<DragData<DraggedItems>>,
        cx: &mut Context<Self>,
    ) {
        if self.dragged_items.is_none() {
            let data = event.drag(cx).data.clone();
            if data.items.is_empty() {
                return;
            }
            let heights: Vec<_> = data
                .items
                .iter()
                .map(|item| self.card_height(item))
                .collect();
            let primary = data
                .items
                .iter()
                .position(|item| item.id() == data.primary.id())
                .unwrap_or(0);
            let grab = event
                .drag(cx)
                .grab_position()
                .map(|position| position.y.clamp(px(0.), heights[primary]))
                .unwrap_or(heights[primary] / 2.);
            let grab_y = heights.iter().take(primary).copied().sum::<Pixels>() + grab;
            let mut drag = UpcomingDrag {
                data,
                heights,
                grab_y,
                origin: None,
                gap: None,
            };
            drag.origin = origin_gap(&self.agenda, &self.layout, &drag);
            self.dragged_items = Some(drag);
            cx.notify();
        }
        self.retarget_drop(event.event.position, cx);
        self.set_edge_scroll_speed(self.edge_scroll_speed_at(event.event.position), cx);
    }

    pub(super) fn render_drop_slot(&self, ix: usize, cx: &Context<Self>) -> AnyElement {
        let index = match self.layout.rows[ix] {
            AgendaLayoutRow::Drop { index, .. } => Some(index),
            _ => None,
        };
        let label = index.zip(self.drop_target).and_then(|(index, target)| {
            let (relation, id) = match target {
                UpcomingDropTarget::Before { item, .. } => ("Before", item),
                UpcomingDropTarget::After { item, .. } => ("After", item),
                UpcomingDropTarget::Day(_) => return None,
            };
            let item = self.agenda.rows.iter().find_map(|row| match row {
                UpcomingRow::Item { item, .. } if item.id() == id => Some(item),
                _ => None,
            })?;
            let dragged = &self.dragged_items.as_ref()?.data;
            let starts = dropped_starts(
                &dragged.items,
                dragged.source_anchor,
                target,
                Settings::global(cx).schedule.default_action_duration,
            );
            let time = match starts.get(index)? {
                SchedulePoint::DateTime(start) => {
                    start.with_timezone(&Local).format("%H:%M").to_string()
                }
                SchedulePoint::Date(date) => date.format("%b %-d").to_string(),
            };
            Some(format!("{time} · {relation} {}", item.title()))
        });
        let (above, below) = self.layout.card_edges(&self.agenda, ix);
        let color = if matches!(self.drop_target, Some(UpcomingDropTarget::Day(_))) {
            cx.theme().colors.hairline
        } else {
            cx.theme().colors.focus
        };
        div()
            .size_full()
            .px_4()
            .child(
                div()
                    .size_full()
                    .border_2()
                    .border_dashed()
                    .border_color(color)
                    .when(!above, |row| row.rounded_t_xl())
                    .when(!below, |row| row.rounded_b_xl())
                    .px_3()
                    .py_1()
                    .text_xs()
                    .text_color(color)
                    .when_some(label, |row, label| {
                        row.child(
                            div()
                                .w_full()
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(label),
                        )
                    }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone, Utc};
    use subroutine_core::Action;

    fn fixture() -> (Agenda, UpcomingDrag, UpcomingSection) {
        let date = NaiveDate::from_ymd_opt(2026, 10, 16).unwrap();
        let at = |hour| Utc.with_ymd_and_hms(2026, 10, 16, hour, 0, 0).unwrap();
        let section = UpcomingSection::Day(date);
        let items = (9..13)
            .map(|hour| {
                AnyItem::Action(
                    Action::new("Scheduled").with_start(Some(SchedulePoint::DateTime(at(hour)))),
                )
            })
            .collect::<Vec<_>>();
        let data = DraggedItems::single(items[0].clone());
        let rows = std::iter::once(UpcomingRow::Day {
            date,
            markers: vec![],
            relative: None,
        })
        .chain(items.into_iter().map(|item| UpcomingRow::Item {
            item,
            projected: false,
            section,
        }))
        .chain(std::iter::once(UpcomingRow::Create {
            date,
            kind: super::super::agenda::CreateKind::Append,
        }))
        .collect();
        (
            Agenda { rows },
            UpcomingDrag {
                data,
                heights: vec![px(40.)],
                grab_y: px(20.),
                origin: None,
                gap: None,
            },
            section,
        )
    }

    #[test]
    fn every_gap_remains_reachable_and_stationary_pointer_survives_preview_reflow() {
        let (agenda, mut drag, section) = fixture();
        let build = || {
            agenda.layout(|_| (true, 1.), |item| {
            let index = agenda.rows.iter().position(|row| matches!(row, UpcomingRow::Item { item: other, .. } if other.id() == item.id())).unwrap();
            px([0., 40., 64., 100., 40.][index])
        }, |_| px(38.))
        };
        let base = build();
        let hidden = drag.data.items.iter().map(AnyItem::id).collect();
        let top = base.sizes[0].height;
        let duration = RelativeDuration::minutes(30);
        for delta in [
            0., 20., 40., 64., 90., 130., 164., 190., 204., 190., 164., 90., 40., 0.,
        ] {
            let pointer = top + px(delta) + drag.grab_y;
            let expected =
                insertion_hit(&agenda, &base, &drag, section, pointer, duration).unwrap();
            drag.gap = expected.gap;
            let mut rendered = build();
            rendered.apply_drop(&agenda, &hidden, drag.slots());
            for _ in 0..4 {
                assert_eq!(
                    insertion_hit(&agenda, &rendered, &drag, section, pointer, duration),
                    Some(expected)
                );
            }
        }
        for (delta, before) in [(0., 2), (64., 3), (164., 4), (204., 5)] {
            let hit = insertion_hit(
                &agenda,
                &base,
                &drag,
                section,
                top + px(delta) + drag.grab_y,
                duration,
            )
            .unwrap();
            assert_eq!(hit.gap.unwrap().before, before);
        }
    }

    #[test]
    fn day_and_outside_targets_do_not_reflow_the_last_gap() {
        let (agenda, mut drag, section) = fixture();
        let gap = DropGap { before: 4, section };
        drag.gap = Some(gap);
        let build = |drag: &UpcomingDrag| {
            let mut layout = agenda.layout(|_| (true, 1.), |_| px(40.), |_| px(38.));
            let hidden = drag.data.items.iter().map(AnyItem::id).collect();
            layout.apply_drop(&agenda, &hidden, drag.slots());
            layout
        };
        let before = build(&drag);
        for hit in [day_hit(section), None] {
            assert_eq!(drag.retarget(hit), hit.map(|hit| hit.target));
            assert_eq!(drag.gap, Some(gap));
            let after = build(&drag);
            assert_eq!(before.rows, after.rows);
            assert_eq!(before.sizes, after.sizes);
        }
    }

    #[test]
    fn only_an_intact_source_block_is_a_no_op() {
        let (agenda, mut drag, _) = fixture();
        let layout = agenda.layout(|_| (true, 1.), |_| px(40.), |_| px(38.));
        assert_eq!(origin_gap(&agenda, &layout, &drag).unwrap().before, 2);
        let UpcomingRow::Item { item, .. } = &agenda.rows[3] else {
            unreachable!()
        };
        drag.data.items.push(item.clone());
        assert_eq!(origin_gap(&agenda, &layout, &drag), None);
        let UpcomingRow::Item { item, .. } = &agenda.rows[2] else {
            unreachable!()
        };
        drag.data.items[1] = item.clone();
        assert_eq!(origin_gap(&agenda, &layout, &drag).unwrap().before, 3);
        drag.data.items.reverse();
        assert_eq!(origin_gap(&agenda, &layout, &drag), None);
    }

    #[test]
    fn a_large_multi_item_slot_does_not_capture_pointer_targeting() {
        let (agenda, mut drag, section) = fixture();
        drag.data
            .items
            .push(AnyItem::Action(Action::new("External")));
        drag.heights.push(px(640.));
        let mut layout = agenda.layout(|_| (true, 1.), |_| px(40.), |_| px(38.));
        let top = layout.sizes[0].height;
        drag.gap = Some(DropGap { before: 2, section });
        let hidden = drag.data.items.iter().map(AnyItem::id).collect();
        layout.apply_drop(&agenda, &hidden, drag.slots());
        let hit = insertion_hit(
            &agenda,
            &layout,
            &drag,
            section,
            top + px(120.) + drag.grab_y,
            RelativeDuration::minutes(30),
        )
        .unwrap();
        assert_ne!(hit.gap, drag.gap);
        assert_eq!(hit.gap.unwrap().before, 5);
    }
}
