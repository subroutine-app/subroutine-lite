use chrono::{DateTime, Duration as ChronoDuration, Local, NaiveDate};
use gpui::{
    App, Context, DragMoveEvent, IntoElement, ParentElement, Pixels, Point, Styled, Window, div, px,
};
use gpui_kit_theme::ActiveTheme;
use subroutine_core::{Action, AnyItem, SchedulePoint};
use uuid::Uuid;

use crate::{
    components::Divider,
    components::{DragData, DraggedItems},
    haptics::HapticsExt as _,
    settings::Settings,
    stores::AppDatabaseStore,
    views::{
        TOP_EDGE_INSET,
        drop_confirmation::{confirm_drop, resolve_dragged, scheduled_item_count},
    },
};

use super::super::{
    ANNOTATION_CHIP_HEIGHT, ATTACHED_ITEM_LEFT, FALLBACK_ITEM_DURATION, Lane, MIN_ITEM_HEIGHT,
    TimelineView, is_durationless_action, measure_from, timeline::compute_edge_scroll_speed,
};
use super::annotations::{SIGNAL_CARD_HEIGHT, day_start, sticky_chip_top};
use super::item_min_height;

fn carried_duration(item: &AnyItem, at: DateTime<Local>, settings: &Settings) -> ChronoDuration {
    item.duration()
        .map(|duration| measure_from(at, duration))
        .filter(|duration| *duration > ChronoDuration::zero())
        .unwrap_or_else(|| measure_from(at, settings.schedule.default_action_duration))
}

fn plan_placements(
    dragged: &DraggedItems,
    drop_time: DateTime<Local>,
    settings: Settings,
) -> impl Iterator<Item = (&AnyItem, DateTime<Local>)> {
    let anchor = dragged.source_anchor().map(DateTime::<Local>::from);
    let mut cursor = drop_time;
    dragged.items.iter().map(move |item| {
        let target = match (anchor, item.start_datetime()) {
            (Some(anchor), Some(start)) => drop_time + (start - anchor),
            _ => {
                let at = cursor;
                cursor = at + carried_duration(item, at, &settings);
                at
            }
        };
        (item, target)
    })
}

fn group_span(
    dragged: &DraggedItems,
    drop_time: DateTime<Local>,
    settings: Settings,
) -> (ChronoDuration, Option<ChronoDuration>) {
    let placed: Vec<_> = plan_placements(dragged, drop_time, settings)
        .filter(|(item, _)| {
            matches!(
                item,
                AnyItem::Action(_) | AnyItem::Event(_) | AnyItem::Routine(_)
            )
        })
        .collect();
    let multiple = placed.len() > 1;

    let mut bounds: Option<(DateTime<Local>, DateTime<Local>)> = None;
    for (item, target) in placed {
        let extent = item
            .duration()
            .map(|duration| measure_from(target, duration))
            .filter(|duration| *duration > ChronoDuration::zero())
            .or_else(|| is_durationless_action(item).then_some(FALLBACK_ITEM_DURATION))
            .unwrap_or_else(|| {
                if multiple {
                    FALLBACK_ITEM_DURATION
                } else {
                    ChronoDuration::zero()
                }
            });
        bounds = Some(match bounds {
            Some((earliest, latest)) => (earliest.min(target), latest.max(target + extent)),
            None => (target, target + extent),
        });
    }

    let Some((earliest, latest)) = bounds else {
        return (ChronoDuration::zero(), None);
    };
    let duration = latest - earliest;
    (
        (drop_time - earliest).max(ChronoDuration::zero()),
        (duration > ChronoDuration::zero()).then_some(duration),
    )
}

fn place_dropped_action(action: Action, target: DateTime<chrono::Utc>) -> Action {
    action
        .with_queued(true)
        .with_start(Some(SchedulePoint::DateTime(target)))
}

fn place_dropped_item(
    item: AnyItem,
    target: DateTime<Local>,
    store: &mut AppDatabaseStore,
    cx: &mut Context<AppDatabaseStore>,
) {
    let target_utc = target.with_timezone(&chrono::Utc);
    match item {
        AnyItem::Action(action) => {
            store.upsert_action(place_dropped_action(action, target_utc), cx);
        }
        AnyItem::Event(mut event) => {
            event.start = target_utc;
            store.upsert_event(event, cx);
        }
        AnyItem::Routine(routine) => {
            store.instantiate_routine(routine.id, Some(target_utc), cx);
        }
        AnyItem::Marker(mut marker) => {
            let date = target.date_naive();
            let end_date = marker.end_date.map(|end| date + (end - marker.date));
            marker.set_date(date);
            marker.set_end_date(end_date);
            store.upsert_marker(marker, cx);
        }
        AnyItem::Signal(mut signal) => {
            signal.datetime = target_utc;
            store.upsert_signal(signal, cx);
        }
        AnyItem::ActionTemplate(_) | AnyItem::EventTemplate(_) => {}
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum DropShape {
    Span {
        lead: ChronoDuration,
        duration: Option<ChronoDuration>,
    },
    Moment {
        offset: ChronoDuration,
    },
    Days {
        start: NaiveDate,
        end: NaiveDate,
    },
}

#[derive(Clone, PartialEq)]
pub(crate) struct ActiveDropState {
    pub dragged: Vec<Uuid>,
    pub drop_time: DateTime<Local>,
    pub shape: DropShape,
    pub min_height: Pixels,
    pub slot_visual_start: DateTime<Local>,
    pub slot_visual_end: DateTime<Local>,
}

impl ActiveDropState {
    pub(crate) fn span(&self) -> Option<(DateTime<Local>, ChronoDuration)> {
        match self.shape {
            DropShape::Span { lead, duration } => {
                Some((self.drop_time - lead, self.span_duration(duration)))
            }
            DropShape::Moment { .. } | DropShape::Days { .. } => None,
        }
    }

    fn span_duration(&self, duration: Option<ChronoDuration>) -> ChronoDuration {
        duration
            .filter(|duration| *duration > ChronoDuration::zero())
            .unwrap_or(self.slot_visual_end - self.slot_visual_start)
    }

    fn preview_geometry_changed(&self, other: &Self) -> bool {
        match (self.shape, other.shape) {
            (DropShape::Span { .. }, DropShape::Span { .. }) => {
                self.span() != other.span() || self.min_height != other.min_height
            }
            (
                DropShape::Moment {
                    offset: self_offset,
                },
                DropShape::Moment {
                    offset: other_offset,
                },
            ) => self.drop_time + self_offset != other.drop_time + other_offset,
            (
                DropShape::Days {
                    start: self_start,
                    end: self_end,
                },
                DropShape::Days {
                    start: other_start,
                    end: other_end,
                },
            ) => self_start != other_start || self_end != other_end,
            _ => true,
        }
    }
}

impl TimelineView {
    pub(super) fn drop_min_height(&self, drop: &ActiveDropState) -> Pixels {
        let [id] = drop.dragged.as_slice() else {
            return drop.min_height;
        };
        let Some(entry) = self.items.iter().find(|entry| entry.item.id() == *id) else {
            return drop.min_height;
        };
        item_min_height(&entry.item)
    }

    fn drop_state_at(
        &self,
        dragged: &DraggedItems,
        mouse_pos: Point<Pixels>,
        cx: &App,
    ) -> Option<ActiveDropState> {
        let pos = self
            .bounds
            .and_then(|bounds| bounds.localize(&mouse_pos))
            .filter(|position| position.y >= TOP_EDGE_INSET)?;
        let center = self.center_relative().y;
        let offset = pos.y - center;
        let raw_time = self.position_to_time(offset);
        let state = self.current_division_state();
        let base = state.base_division;
        let sub = state.current_subdivision();
        let slot_start = sub
            .map(|s| s.nearest_boundary(raw_time))
            .unwrap_or_else(|| base.nearest_boundary(raw_time));
        let slot_dur = sub
            .map(|s| s.exact_duration(slot_start))
            .unwrap_or_else(|| base.exact_duration(slot_start));
        let slot_end = slot_start + slot_dur;

        let settings = Settings::global(cx);
        let shape = if dragged
            .items
            .iter()
            .any(|item| item.item_type().occupies_time())
        {
            let (lead, duration) = group_span(dragged, slot_start, settings);
            DropShape::Span { lead, duration }
        } else if let Some(signal) = dragged.items.iter().find_map(|item| match item {
            AnyItem::Signal(signal) => Some(signal),
            _ => None,
        }) {
            let anchor = dragged
                .source_anchor()
                .map(DateTime::<Local>::from)
                .unwrap_or_else(|| signal.datetime.with_timezone(&Local));
            DropShape::Moment {
                offset: signal.datetime.with_timezone(&Local) - anchor,
            }
        } else {
            match &dragged.primary {
                AnyItem::Marker(marker) => {
                    let date = raw_time.date_naive();
                    let span = marker
                        .end_date
                        .map(|end| end - marker.date)
                        .unwrap_or_else(ChronoDuration::zero);
                    DropShape::Days {
                        start: date,
                        end: date + span,
                    }
                }
                AnyItem::Signal(_) => DropShape::Moment {
                    offset: ChronoDuration::zero(),
                },
                _ => {
                    let (lead, duration) = group_span(dragged, slot_start, settings);
                    DropShape::Span { lead, duration }
                }
            }
        };
        let drop_time = match shape {
            DropShape::Days { start, .. } => day_start(start),
            _ => slot_start,
        };

        let min_height = if dragged.items.len() == 1 {
            item_min_height(&dragged.primary)
        } else {
            MIN_ITEM_HEIGHT
        };

        Some(ActiveDropState {
            dragged: dragged.items.iter().map(|item| item.id()).collect(),
            drop_time,
            shape,
            min_height,
            slot_visual_start: slot_start,
            slot_visual_end: slot_end,
        })
    }

    fn set_drop_preview(&mut self, new_drop: Option<ActiveDropState>, cx: &mut Context<Self>) {
        if new_drop != self.active_drop {
            if self
                .active_drop
                .as_ref()
                .zip(new_drop.as_ref())
                .is_some_and(|(before, after)| before.preview_geometry_changed(after))
            {
                cx.play_alignment_haptic();
            }
            self.active_drop = new_drop;
            cx.notify();
        }
    }

    pub(in crate::views::main_view::timeline_view) fn retarget_drop_preview_at(
        &mut self,
        mouse_pos: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let new_drop = self
            .drop_dragged
            .as_ref()
            .and_then(|dragged| self.drop_state_at(dragged, mouse_pos, cx));
        self.set_drop_preview(new_drop, cx);
    }

    pub(crate) fn handle_drag_move(
        &mut self,
        event: &DragMoveEvent<DragData<DraggedItems>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mouse_pos = window.mouse_position();
        let local_pos = self
            .bounds
            .and_then(|bounds| bounds.localize(&mouse_pos))
            .filter(|position| position.y >= TOP_EDGE_INSET);
        self.drop_dragged = Some(event.drag(cx).data.clone());
        self.retarget_drop_preview_at(mouse_pos, cx);

        let new_speed = self.bounds.and_then(|b| {
            let local_y = local_pos.map(|p| p.y)?;
            compute_edge_scroll_speed(local_y, b.size.height)
        });
        if new_speed != self.edge_scroll_speed {
            self.edge_scroll_speed = new_speed;
            cx.notify();
        }
    }

    pub(crate) fn handle_drop(
        &mut self,
        data: &DragData<DraggedItems>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut dragged = resolve_dragged(&data.data, cx);
        dragged
            .items
            .retain(|item| !matches!(item, AnyItem::ActionTemplate(_) | AnyItem::EventTemplate(_)));
        let count = scheduled_item_count(&dragged.items);
        let drop_info = self.active_drop.take();
        self.finish_item_drag(cx);
        let Some(drop_info) = drop_info.filter(|_| count > 0) else {
            return;
        };

        let drop_time = drop_info.drop_time;
        let settings = Settings::global(cx);
        let placements: Vec<_> = plan_placements(&dragged, drop_time, settings)
            .map(|(item, target)| (item.clone(), target))
            .collect();
        let mut detail = format!(
            "Schedule on the timeline at {}.",
            drop_time.format("%A, %B %-d, %Y at %H:%M")
        );
        if dragged.items.len() > 1 {
            if dragged.source_anchor().is_some() {
                detail.push_str(" Scheduled items keep their spacing; items without a time are placed consecutively.");
            } else {
                detail.push_str(" Items are placed consecutively from this time.");
            }
        }

        confirm_drop(count, "Schedule", detail, window, cx, move |_, cx| {
            AppDatabaseStore::global(cx).update(cx, |store, cx| {
                for (item, target) in placements {
                    place_dropped_item(item, target, store, cx);
                }
            });
        });
    }

    pub(crate) fn render_active_drop(
        &self,
        drop_info: &ActiveDropState,
        lane: Option<Lane>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let color = cx.theme().colors.focus;
        let scroll_y = self.scroll_offset + self.center_relative().y;
        let container = div().absolute().inset_0();

        match drop_info.shape {
            DropShape::Span { lead, duration } => {
                let start = drop_info.drop_time - lead;
                let duration = drop_info.span_duration(duration);
                let bounds = self.lane_bounds_with_min_height(
                    start,
                    duration,
                    lane.unwrap_or(Lane::FULL),
                    self.drop_min_height(drop_info),
                );
                container.child(
                    div()
                        .absolute()
                        .top(bounds.top() + scroll_y)
                        .left(bounds.left())
                        .w(bounds.size.width)
                        .h(bounds.size.height)
                        .rounded_xl()
                        .border(px(2.))
                        .border_dashed()
                        .border_color(color)
                        .flex()
                        .items_start()
                        .justify_start()
                        .p_1()
                        .child(self.drop_label(start.format("%-I:%M").to_string(), cx)),
                )
            }
            DropShape::Moment { offset } => {
                let y = self.time_to_offset(drop_info.drop_time + offset) + scroll_y;
                container
                    .child(
                        div()
                            .absolute()
                            .top(y)
                            .left(ATTACHED_ITEM_LEFT)
                            .w((self.annotation_left() - ATTACHED_ITEM_LEFT).max(px(0.)))
                            .h(px(1.))
                            .child(Divider::horizontal().dashed().color(color)),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(y - SIGNAL_CARD_HEIGHT / 2.)
                            .left(self.chip_left())
                            .w(self.chip_width())
                            .h(SIGNAL_CARD_HEIGHT)
                            .flex()
                            .items_center()
                            .child(self.drop_label(
                                drop_info.drop_time.format("%-I:%M %p").to_string(),
                                cx,
                            )),
                    )
            }
            DropShape::Days { start, end } => {
                let top = self.time_to_offset(day_start(start)) + scroll_y;
                let bottom =
                    self.time_to_offset(day_start(end + ChronoDuration::days(1))) + scroll_y;
                let label = if start == end {
                    start.format("%a %b %-d").to_string()
                } else {
                    format!("{} – {}", start.format("%b %-d"), end.format("%b %-d"))
                };
                let label_top = sticky_chip_top(top, bottom);
                container.child(
                    div()
                        .absolute()
                        .top(label_top)
                        .left(self.chip_left())
                        .w(self.chip_width())
                        .h(ANNOTATION_CHIP_HEIGHT)
                        .flex()
                        .items_center()
                        .child(self.drop_label(label, cx)),
                )
            }
        }
    }

    fn drop_label(&self, text: String, cx: &Context<Self>) -> impl IntoElement {
        div()
            .px(px(7.))
            .bg(cx.theme().colors.canvas.alpha(0.8))
            .rounded_lg()
            .text_sm()
            .text_color(cx.theme().colors.focus)
            .child(text)
    }
}
