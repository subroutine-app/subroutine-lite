use crate::components::Label;

use crate::components::transition::{self, KeyedTransition, WindowTransitionExt as _};
use crate::icons::Icon;
use chrono::{DateTime, Duration as ChronoDuration, Local};
use gpui::prelude::FluentBuilder;
use gpui::{
    AnimationExt, AnyElement, App, AppContext, Bounds, Context, ElementId, FocusHandle,
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MousePressureEvent,
    ParentElement, Pixels, PressureStage, SharedString, SpringAnimation, SpringConfig,
    SpringPlayback, StatefulInteractiveElement, Styled, Window, div, point, px, size,
};
use gpui_kit::display::loading::Skeleton;
use gpui_kit::foundation::StyledExt as _;
use gpui_kit::motion::{Interpolate, MotionSpec};
use gpui_kit_theme::ActiveTheme;
use subroutine_core::{AnyItem, StartPrecision};

use crate::AppIcon;
use crate::components::ItemCard;
use crate::item_manager::ItemManager;
use crate::keys::force_click_modifier;
use crate::selection::{SelectionManager, SelectionOrder, SelectionScope};
use crate::views::{InspectItem, TOP_EDGE_INSET};
use uuid::Uuid;

use super::super::{MIN_ITEM_HEIGHT, MIN_ITEM_WIDTH, ResizeDragData, ResizeEdge, TimelineView};
use super::{
    ATTACHED_ITEM_LEFT, FALLBACK_ITEM_DURATION, ITEM_KEY_CONTEXT, Lane, NavigationHandoffs,
    RESIZE_HANDLE_HEIGHT, ResizeGhost, SLOT_GAP, TimelineSlot, item_inspection_matches_press,
    item_layout_duration, item_min_height, item_timeline_span, item_title_only, measure_from,
    timeline_selection_ids,
};

const SKELETON_EVENT_DURATION: ChronoDuration = ChronoDuration::hours(1);
const SKELETON_ACTION_DURATION: ChronoDuration = ChronoDuration::minutes(5);
const RESCHEDULE: MotionSpec = MotionSpec::new(100, transition::EASE_OUT_CUBIC);
const STICKY_CONTENT_SPRING: SpringConfig = SpringConfig::new(1024.0, 64.0, 0.125);
const MIN_COLUMN_WIDTH: Pixels = px(48.);

const PROJECTED_WORK_FILL_ALPHA: f32 = 0.055;
const PROJECTED_WORK_EDGE_ALPHA: f32 = 0.28;
const PROJECTED_WORK_LABEL_MIN_HEIGHT: Pixels = px(20.);

const LARGE_ITEM_TITLE_MIN_HEIGHT: Pixels = px(112.);

fn uses_large_item_title(card_height: Pixels, card_width: Pixels) -> bool {
    card_height >= LARGE_ITEM_TITLE_MIN_HEIGHT && card_width >= MIN_ITEM_WIDTH
}

struct AttachedItemRender {
    item: AnyItem,
    focus_handle: FocusHandle,
    meta_text: Option<SharedString>,
    bounds: Option<Bounds<Pixels>>,
    order: SelectionOrder,
    next_focus: Option<(Uuid, FocusHandle)>,
}

fn lane_horizontal_geometry(total_width: Pixels, lane: Lane) -> (Pixels, Pixels) {
    let count = lane.count.max(1) as f32;
    let width =
        ((total_width - SLOT_GAP * (count - 1.)) / count).max(MIN_COLUMN_WIDTH.min(total_width));
    ((width + SLOT_GAP) * lane.index as f32, width)
}

fn item_details_height() -> Pixels {
    MIN_ITEM_HEIGHT - SLOT_GAP
}

fn sticky_contained_top(
    top: Pixels,
    bottom: Pixels,
    height: Pixels,
    visible_top: Pixels,
) -> Pixels {
    let floor = (bottom - height).max(top);
    top.max(visible_top).min(floor)
}

fn sticky_span_card_top(top: Pixels, bottom: Pixels, height: Pixels) -> Pixels {
    sticky_contained_top(top + SLOT_GAP, bottom - SLOT_GAP, height, TOP_EDGE_INSET)
}

fn sticky_item_details_top(top: Pixels, bottom: Pixels, details_height: Pixels) -> Pixels {
    sticky_contained_top(top, bottom, details_height, TOP_EDGE_INSET)
}

fn clamp_item_details_offset(
    offset: Pixels,
    card_height: Pixels,
    details_height: Pixels,
) -> Pixels {
    let inset = px(6.);
    let max_offset = (card_height - details_height).max(px(0.)) + inset;
    offset.max(inset).min(max_offset)
}

fn clamp_span_card_offset(offset: Pixels, track_height: Pixels, card_height: Pixels) -> Pixels {
    let max_offset = (track_height - SLOT_GAP - card_height).max(SLOT_GAP);
    offset.max(SLOT_GAP).min(max_offset)
}

fn sticky_content_playback(zoom_scale: f32, bounds_changed: bool) -> SpringPlayback {
    if zoom_scale != 1.0 || bounds_changed {
        SpringPlayback::Completed
    } else {
        SpringPlayback::Running
    }
}

fn sticky_content_spring(target: Pixels, playback: SpringPlayback) -> SpringAnimation<Pixels> {
    SpringAnimation::new(STICKY_CONTENT_SPRING)
        .to(target)
        .with_epsilon(0.1)
        .playback(playback)
}

#[derive(Clone, Copy)]
pub(super) enum SlotKind {
    Item,
    Bin,
}

impl SlotKind {
    fn transition_keys(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            SlotKind::Item => ("item-width", "item-height", "item-x", "item-y"),
            SlotKind::Bin => ("bin-width", "bin-height", "bin-x", "bin-y"),
        }
    }
}

pub(super) fn attach_transition<T: Interpolate + PartialEq + 'static>(
    id: impl Into<ElementId>,
    init: T,
    spec: MotionSpec,
    window: &mut Window,
    cx: &mut App,
) -> KeyedTransition<T> {
    window.keyed_transition(id, cx, spec, || init)
}

impl TimelineView {
    pub(in crate::views::main_view::timeline_view) fn item_area_left(&self) -> Pixels {
        ATTACHED_ITEM_LEFT
    }

    pub(in crate::views::main_view::timeline_view) fn item_area_width(&self) -> Pixels {
        if let Some(bounds) = self.bounds {
            let available =
                bounds.size.width - self.item_area_left() - self.annotation_gutter_width();
            if available > px(0.) {
                return available.max(px(0.));
            }
        }
        px(400.)
    }

    pub(super) fn create_bounds(
        &self,
        time: DateTime<Local>,
        duration: ChronoDuration,
    ) -> Bounds<Pixels> {
        self.lane_bounds(time, duration, Lane::FULL)
    }

    pub(super) fn lane_bounds(
        &self,
        time: DateTime<Local>,
        duration: ChronoDuration,
        lane: Lane,
    ) -> Bounds<Pixels> {
        self.lane_bounds_with_min_height(time, duration, lane, MIN_ITEM_HEIGHT)
    }

    pub(super) fn item_bounds(
        &self,
        item: &AnyItem,
        time: DateTime<Local>,
        duration: ChronoDuration,
        lane: Lane,
    ) -> Bounds<Pixels> {
        let layout_duration = item_layout_duration(item, duration, self.pixel_duration);
        self.lane_bounds_with_min_height(time, layout_duration, lane, item_min_height(item))
    }

    pub(super) fn lane_bounds_with_min_height(
        &self,
        time: DateTime<Local>,
        duration: ChronoDuration,
        lane: Lane,
        min_height: Pixels,
    ) -> Bounds<Pixels> {
        let total_width = self.item_area_width();
        let (offset, lane_width) = lane_horizontal_geometry(total_width, lane);
        let left = self.item_area_left() + offset;
        let top = self.time_to_offset(time);
        let height = self.duration_to_height(duration).max(min_height) - SLOT_GAP;
        Bounds::from_corners(point(left, top), point(left + lane_width, top + height))
    }

    pub(super) fn transition_bounds(
        &self,
        kind: SlotKind,
        key: u64,
        target: Bounds<Pixels>,
        jump: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Bounds<Pixels> {
        let (width_key, height_key, x_key, y_key) = kind.transition_keys();

        let width_transition =
            attach_transition((width_key, key), target.size.width, RESCHEDULE, window, cx);
        let height_transition = attach_transition(
            (height_key, key),
            target.size.height,
            RESCHEDULE,
            window,
            cx,
        );
        let x_transition = attach_transition((x_key, key), target.origin.x, RESCHEDULE, window, cx);
        let y_transition = attach_transition((y_key, key), target.origin.y, RESCHEDULE, window, cx);

        if jump || self.bounds_changed {
            width_transition.snap(target.size.width, cx);
            height_transition.snap(target.size.height, cx);
            x_transition.snap(target.origin.x, cx);
            y_transition.snap(target.origin.y, cx);
            return target;
        }

        let vertical_space_changed = self.item_y_scale != 1.0 || self.item_y_offset != px(0.);
        let zoomed_this_frame = self.item_y_scale != 1.0;

        if vertical_space_changed {
            y_transition.scale_by(self.item_y_scale, cx);
            y_transition.offset_by(self.item_y_offset, cx);
        }

        if zoomed_this_frame {
            let current_width = width_transition.value(cx).as_f32();
            if current_width != 0.0 {
                width_transition.scale_by(target.size.width.as_f32() / current_width, cx);
            }
            let current_height = height_transition.value(cx).as_f32();
            if current_height != 0.0 {
                height_transition.scale_by(target.size.height.as_f32() / current_height, cx);
            }
        }

        width_transition.set(target.size.width, cx);
        height_transition.set(target.size.height, cx);
        x_transition.set(target.origin.x, cx);
        y_transition.set(target.origin.y, cx);

        Bounds::new(
            point(
                x_transition.animate(window, cx),
                y_transition.animate(window, cx),
            ),
            size(
                width_transition.animate(window, cx),
                height_transition.animate(window, cx),
            ),
        )
    }

    pub(crate) fn render_skeleton_items(&self) -> Vec<impl IntoElement> {
        const N_ACTIONS: i32 = 3;
        const N_EVENTS: i32 = 2;

        let min_span = self.min_visual_span();
        let action_step = SKELETON_ACTION_DURATION.max(min_span);
        let event_step = SKELETON_EVENT_DURATION.max(min_span);

        let actions_start = self.current_division_state().auto_floor_boundary(self.now);
        let scroll_y = self.scroll_offset + self.center_relative().y;
        let action_skeletons = (0..N_ACTIONS)
            .map(|i| {
                let time = actions_start + action_step * i;
                let bounds = self.create_bounds(time, SKELETON_ACTION_DURATION);
                div()
                    .absolute()
                    .top(bounds.top() + scroll_y)
                    .left(bounds.left())
                    .h(bounds.size.height)
                    .w(bounds.size.width)
                    .child(
                        div()
                            .w_64()
                            .h_full()
                            .child(Skeleton::new("attached.placeholder")),
                    )
            })
            .collect::<Vec<_>>();
        let events_start = actions_start + action_step * N_ACTIONS;
        let event_skeletons = (0..N_EVENTS)
            .map(|i| {
                let time = events_start + event_step * i;
                let bounds = self.create_bounds(time, SKELETON_EVENT_DURATION);
                div()
                    .absolute()
                    .top(bounds.top() + scroll_y)
                    .left(bounds.left())
                    .h(bounds.size.height)
                    .w(bounds.size.width)
                    .child(
                        div()
                            .w_64()
                            .h_full()
                            .child(Skeleton::new("attached.placeholder")),
                    )
            })
            .collect::<Vec<_>>();

        action_skeletons
            .into_iter()
            .chain(event_skeletons)
            .collect::<Vec<_>>()
    }

    pub(super) fn wire_timeline_item_card(
        &self,
        card: ItemCard,
        item: &AnyItem,
        cx: &mut Context<Self>,
    ) -> ItemCard {
        use gpui::KeyDownEvent;

        let item_id = item.id();
        let is_editing = self.is_being_edited(item_id, cx);
        let is_action = matches!(item, AnyItem::Action(_));
        let cloned = item.clone();
        let pressure_item = item.clone();
        let control_item = item.clone();
        let focus = gpui::Focusable::focus_handle(&card, cx);

        card.navigation_context(ITEM_KEY_CONTEXT)
            .on_mouse_pressure(
                cx.listener(move |view, event: &MousePressureEvent, _window, cx| {
                    if event.stage == PressureStage::Force
                        && item_inspection_matches_press(
                            view.inspect_press_item,
                            view.inspect_force_item,
                            item_id,
                        )
                    {
                        view.inspect_force_item = Some(item_id);
                        cx.stop_propagation();
                        cx.emit(InspectItem(pressure_item.clone()));
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, event: &MouseDownEvent, _window, cx| {
                    if force_click_modifier(&event.modifiers)
                        && item_inspection_matches_press(
                            view.inspect_press_item,
                            view.inspect_force_item,
                            item_id,
                        )
                    {
                        view.inspect_force_item = Some(item_id);
                        cx.stop_propagation();
                        cx.emit(InspectItem(control_item.clone()));
                    }
                }),
            )
            .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                if event.is_held || !focus.is_focused(window) {
                    return;
                }
                let item_manager = ItemManager::global(cx);
                match event.keystroke.key.as_str() {
                    "enter" if !is_editing => {
                        cx.stop_propagation();
                        item_manager.update(cx, |handler, cx| {
                            handler.begin_edit(&cloned, false, window, cx);
                        });
                        cx.notify();
                    }
                    "space" if !is_editing && is_action => {
                        cx.stop_propagation();
                        view.begin_complete_item(item_id, window, cx);
                        cx.notify();
                    }
                    _ => {}
                }
            }))
            .on_aux_click(|_, _, cx| cx.stop_propagation())
    }

    fn render_attached_item(
        &mut self,
        args: AttachedItemRender,
        resize_handles: &mut Vec<AnyElement>,
        details_overlays: &mut Vec<AnyElement>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let AttachedItemRender {
            item,
            focus_handle: item_focus_handle,
            meta_text,
            bounds,
            order,
            next_focus,
        } = args;
        let item_id = item.id();

        let (start_time, timeline_duration) = item_timeline_span(&item)?;
        let target = bounds.unwrap_or_else(|| {
            let bounds = self.item_bounds(&item, start_time, timeline_duration, Lane::FULL);
            Bounds::new(bounds.origin, size(MIN_ITEM_WIDTH, bounds.size.height))
        });

        let resize_target = self
            .active_resize
            .as_ref()
            .filter(|resize| resize.item_id == item_id)
            .map(|resize| {
                let top = self.time_to_offset(resize.new_time);
                let height = self
                    .duration_to_height(resize.new_end - resize.new_time)
                    .max(resize.min_height())
                    - SLOT_GAP;
                Bounds::from_corners(
                    point(target.left(), top),
                    point(target.right(), top + height),
                )
            });
        let is_being_resized = resize_target.is_some();
        let current_bounds = self.transition_bounds(
            SlotKind::Item,
            item.id_u64(),
            resize_target.unwrap_or(target),
            is_being_resized,
            window,
            cx,
        );
        if is_being_resized {
            return None;
        }

        let anim = current_bounds + point(px(0.), self.scroll_offset + self.center_relative().y);

        let y = anim.top();
        let h = anim.size.height;
        let item_left = anim.left();
        let item_w = anim.size.width;
        let details_open = self.item_details.is_open(item_id);
        let details_height = item_details_height().min(h);
        let details_top = sticky_item_details_top(y, y + h, details_height);
        let details_offset = details_top - y + px(6.);

        let is_timed = item.start_precision() == StartPrecision::DateTime;
        let is_resizable =
            is_timed && !details_open && !ItemManager::global(cx).read(cx).is_draft(item_id);

        let top_handle = div()
            .flex()
            .id(("resize-top", item_id.as_u64_pair().1))
            .block_mouse_except_scroll()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h(RESIZE_HANDLE_HEIGHT)
            .cursor_row_resize()
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                SelectionManager::claim_press(cx)
            })
            .on_drag(
                ResizeDragData {
                    item_id,
                    edge: ResizeEdge::Top,
                },
                |_, _, _, cx| cx.new(|_| ResizeGhost),
            )
            .opacity(0.)
            .hover(|s| s.opacity(0.5))
            .justify_center()
            .child(
                div()
                    .h_0p5()
                    .w_5()
                    .top(px(-0.5))
                    .rounded_full()
                    .bg(cx.theme().colors.text),
            );

        let bottom_handle = div()
            .flex()
            .id(("resize-bottom", item_id.as_u64_pair().1))
            .block_mouse_except_scroll()
            .absolute()
            .bottom_0()
            .left_0()
            .w_full()
            .h(RESIZE_HANDLE_HEIGHT)
            .cursor_row_resize()
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                SelectionManager::claim_press(cx)
            })
            .on_drag(
                ResizeDragData {
                    item_id,
                    edge: ResizeEdge::Bottom,
                },
                |_, _, _, cx| cx.new(|_| ResizeGhost),
            )
            .opacity(0.)
            .hover(|s| s.opacity(0.5))
            .items_end()
            .justify_center()
            .child(
                div()
                    .h_0p5()
                    .w_5()
                    .top(px(0.5))
                    .rounded_full()
                    .bg(cx.theme().colors.text),
            );

        if is_resizable {
            resize_handles.push(
                div()
                    .absolute()
                    .top(y)
                    .left(item_left)
                    .w(item_w)
                    .h(h)
                    .child(top_handle)
                    .child(bottom_handle)
                    .into_any_element(),
            );
        }

        let card_height = if is_timed {
            h
        } else {
            item_min_height(&item) - SLOT_GAP
        };
        let card_top = if is_timed {
            y
        } else {
            sticky_span_card_top(y, y + h, card_height)
        };
        let visible = self.bounds.is_none_or(|bounds| {
            card_top + card_height > TOP_EDGE_INSET && card_top < bounds.size.height
        });
        let card = ItemCard::new(&item, meta_text, window, cx)
            .details(self.item_details.get(item_id))
            .compact(item_w < MIN_ITEM_WIDTH)
            .title_only(item_title_only(&item, card_height))
            .large_title(!details_open && uses_large_item_title(card_height, item_w))
            .with_focus_handle(item_focus_handle)
            .next_focus(next_focus)
            .selectable(order)
            .size_full()
            .bg(cx.theme().colors.panel)
            .draggable(true, None);
        let card = self.wire_timeline_item_card(card, &item, cx);
        let sticky_playback =
            sticky_content_playback(self.item_y_scale, self.bounds_changed || details_open);
        let card = if is_timed {
            card.with_spring(
                (ElementId::Uuid(item_id), "sticky-content"),
                sticky_content_spring(details_offset, sticky_playback),
                move |card, top| {
                    card.content_top(if details_open {
                        px(6.)
                    } else {
                        clamp_item_details_offset(top, card_height, details_height)
                    })
                },
            )
            .into_any_element()
        } else {
            card.into_any_element()
        };
        let card = div()
            .absolute()
            .left_0()
            .w_full()
            .when_else(
                details_open,
                |card| card.h_full(),
                |card| card.h(card_height),
            )
            .child(card);
        let card = if is_timed {
            card.top(if details_open { px(0.) } else { card_top - y })
                .into_any_element()
        } else {
            card.with_spring(
                (ElementId::Uuid(item_id), "sticky-card"),
                sticky_content_spring(card_top - y, sticky_playback),
                move |card, top| {
                    card.top(if details_open {
                        px(0.)
                    } else {
                        clamp_span_card_offset(top, h, card_height)
                    })
                },
            )
            .into_any_element()
        };

        if details_open {
            let anchor = Bounds::new(
                point(item_left, if is_timed { details_top } else { card_top }),
                size(item_w, card_height),
            );
            details_overlays.push(self.render_item_details(item_id, card, anchor, window, cx));
            return Some(
                div()
                    .absolute()
                    .top(y)
                    .left(item_left)
                    .w(item_w)
                    .h(h)
                    .rounded_xl()
                    .border_1()
                    .border_color(cx.theme().colors.hairline)
                    .bg(cx.theme().colors.raised.alpha(0.35))
                    .into_any_element(),
            );
        }
        Some(
            div()
                .id(("timeline-item-frame", item_id.as_u64_pair().1))
                .absolute()
                .top(y)
                .h(h)
                .left(item_left)
                .w(item_w)
                .when(!is_timed, |this| this.overflow_hidden().rounded_xl())
                .when(!visible && !self.is_being_edited(item_id, cx), |this| {
                    this.invisible()
                })
                .child(card)
                .into_any_element(),
        )
    }

    pub(super) fn selection_order(&self) -> SelectionOrder {
        SelectionOrder::new(
            SelectionScope::Timeline,
            timeline_selection_ids(
                self.items.iter().map(|entry| &entry.item),
                self.known_signals(),
            ),
        )
    }

    pub(crate) fn render_timeline_cards(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut cards = self.render_projected_work(cx);

        let handoffs = self.navigation_handoffs(cx);
        cards.extend(self.render_attached_items(&handoffs, window, cx));
        cards
    }

    fn render_projected_work(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        self.sync_projected_work();
        let Some(bounds) = self.bounds else {
            return Vec::new();
        };
        let scroll_y = self.scroll_offset + self.center_relative().y;
        let left = self.item_area_left();
        let width = self.item_area_width();

        self.projected_work
            .iter()
            .filter_map(|item| {
                let start = item.start_datetime()?;
                let duration = item
                    .duration()
                    .map(|duration| measure_from(start, duration))
                    .filter(|duration| *duration > ChronoDuration::zero())
                    .unwrap_or(FALLBACK_ITEM_DURATION);
                let top = self.time_to_offset(start) + scroll_y;
                let height = self.duration_to_height(duration).max(px(1.));
                if top + height < px(0.) || top > bounds.size.height {
                    return None;
                }

                let neutral = cx.theme().colors.text_muted;
                let title = SharedString::from(item.title().to_string());
                Some(
                    div()
                        .id(("timeline-projected-work", item.id_u64()))
                        .absolute()
                        .top(top)
                        .left(left)
                        .w(width)
                        .h(height)
                        .overflow_hidden()
                        .border_l_1()
                        .border_color(neutral.alpha(PROJECTED_WORK_EDGE_ALPHA))
                        .bg(neutral.alpha(PROJECTED_WORK_FILL_ALPHA))
                        .when(height >= PROJECTED_WORK_LABEL_MIN_HEIGHT, |this| {
                            this.child(
                                div()
                                    .row()
                                    .h_full()
                                    .items_center()
                                    .gap_1()
                                    .px_2()
                                    .opacity(0.55)
                                    .child(Icon::new(AppIcon::Repeat).size_3())
                                    .child(Label::new(title).text_xs().truncate()),
                            )
                        })
                        .into_any_element(),
                )
            })
            .collect()
    }

    fn render_attached_items(
        &mut self,
        handoffs: &NavigationHandoffs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let slots = self.layout_slots(cx);
        self.sync_expanded_bin(&slots, window, cx);
        self.sync_item_details(&slots, window, cx);

        let viewport_height = self.bounds.map(|b| b.size.height).unwrap_or(px(800.));
        let cull_margin = px(200.);
        let scroll_y = self.scroll_offset + self.center_relative().y;
        let on_screen = |top: Pixels, height: Pixels| {
            top + height + cull_margin >= px(0.) && top - cull_margin <= viewport_height
        };

        let mut elements = Vec::new();
        let mut resize_handles = Vec::new();
        let mut details_overlays = Vec::new();
        let mut expanded_bin = None;
        let mut mounted_pending_focus = None;
        let order = self.selection_order();
        SelectionManager::report_order(&order, cx);
        for slot in slots {
            match slot {
                TimelineSlot::Item { index, lane } => {
                    let Some(entry) = self.items.get(index) else {
                        continue;
                    };
                    let Some((stored_time, stored_duration)) = item_timeline_span(&entry.item)
                    else {
                        continue;
                    };
                    let resize = self
                        .active_resize
                        .as_ref()
                        .filter(|resize| resize.item_id == entry.item.id())
                        .cloned();
                    let (time, duration) = resize
                        .as_ref()
                        .map(|resize| (resize.new_time, resize.new_end - resize.new_time))
                        .unwrap_or((stored_time, stored_duration));
                    let bounds = self.item_bounds(&entry.item, time, duration, lane);
                    if !on_screen(bounds.top() + scroll_y, bounds.size.height)
                        && !self.item_details.is_open(entry.item.id())
                        && !self.is_being_edited(entry.item.id(), cx)
                    {
                        continue;
                    }
                    let item = entry.item.clone();
                    let item_id = entry.item.id();
                    let focus_handle = entry.focus_handle.clone();
                    let pending_focus_handle = focus_handle.clone();
                    let should_focus = self.pending_item_focus == Some(item_id);
                    let meta = entry.cached_meta.clone();
                    let next_focus = handoffs.get(&item_id).cloned().flatten();
                    if let Some(element) = self.render_attached_item(
                        AttachedItemRender {
                            item,
                            focus_handle,
                            meta_text: meta,
                            bounds: Some(bounds),
                            order: order.clone(),
                            next_focus,
                        },
                        &mut resize_handles,
                        &mut details_overlays,
                        window,
                        cx,
                    ) {
                        elements.push(element.into_any_element());
                        if should_focus {
                            mounted_pending_focus = Some((item_id, pending_focus_handle));
                        }
                    } else if let Some(resize) = resize {
                        elements.push(
                            self.render_active_resize(&resize, lane, cx)
                                .into_any_element(),
                        );
                    }
                }
                TimelineSlot::Bin(bin) => {
                    let expanded = self.bin_is_expanded(&bin);
                    let bounds = self.lane_bounds(bin.start, ChronoDuration::zero(), bin.lane);
                    let screen_top = bounds.top() + scroll_y;
                    if !expanded && !on_screen(screen_top, bounds.size.height) {
                        continue;
                    }
                    let element = self.render_bin(&bin, handoffs, window, cx);
                    if expanded {
                        expanded_bin = Some(element);
                        if let Some(entry) = bin
                            .members
                            .iter()
                            .filter_map(|index| self.items.get(*index))
                            .find(|entry| self.pending_item_focus == Some(entry.item.id()))
                        {
                            mounted_pending_focus =
                                Some((entry.item.id(), entry.focus_handle.clone()));
                        }
                    } else {
                        elements.push(element);
                    }
                }
                TimelineSlot::Drop { lane } => {
                    if let Some(drop) = self.active_drop.clone() {
                        elements.push(self.render_active_drop(&drop, lane, cx).into_any_element());
                    }
                }
            }
        }
        if let Some((item_id, focus_handle)) = mounted_pending_focus {
            cx.on_next_frame(window, move |view, window, cx| {
                if view.pending_item_focus == Some(item_id) {
                    focus_handle.focus(window, cx);
                    view.pending_item_focus = None;
                    cx.notify();
                }
            });
        }
        elements.push(self.render_hover_layer(window, cx).into_any_element());
        elements.extend(resize_handles);
        elements.extend(details_overlays);
        elements.extend(expanded_bin);
        elements
    }
}
