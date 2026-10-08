use crate::components::transition::{self, WindowTransitionExt as _};
use gpui_kit::foundation::{Sizable as _, StyledExt as _};
use gpui_kit::motion::MotionSpec;
use std::{collections::HashSet, time::Duration};

use crate::components::ext::InteractiveElementExt;
use crate::components::menu::MenuBuilder;
use crate::components::{Button, ButtonVariants, Label};
use chrono::{DateTime, Duration as ChronoDuration, Local};
use gpui::{
    App, ClickEvent, Context, Entity, Focusable, FontWeight, InteractiveElement, IntoElement,
    MouseButton, MouseMoveEvent, MousePressureEvent, MouseUpEvent, ParentElement, PinchEvent,
    Pixels, Point, ScrollDelta, ScrollWheelEvent, StatefulInteractiveElement, Styled, Window, div,
    point, prelude::FluentBuilder, px,
};
use gpui_kit_theme::ActiveTheme;

use crate::{
    components::Divider,
    icons::{AppIcon, Icon},
    item_manager::ItemManager,
    presentation::UxColor,
    selection::{SelectionManager, SelectionScope},
    views::TOP_EDGE_INSET,
};
use subroutine_core::{AnyItem, SchedulePoint};

use super::{
    BaseTimeDivision, EDGE_HORIZON, HOUR_DIVIDER_HEIGHT, MAX_PIXEL_DURATION, MIN_PIXEL_DURATION,
    PIXEL_SCALE, POINTER_FLOOR_BOUNDARY_FRACTION, SPAN_FIT_FILL, STICKY_OUTER_LABEL_GAP,
    STICKY_OUTER_LABEL_HEIGHT, STICKY_OUTER_LABEL_INSET, TimelineView,
};

const ZOOM: MotionSpec = MotionSpec::new(100, transition::EASE_OUT_CUBIC);

const WHEEL_ZOOM_SENSITIVITY: f32 = 0.004;
const MAX_WHEEL_ZOOM_EXPONENT: f32 = 1.0;

fn wheel_zoom_scale(delta: Pixels) -> f32 {
    (delta.as_f32() * WHEEL_ZOOM_SENSITIVITY)
        .clamp(-MAX_WHEEL_ZOOM_EXPONENT, MAX_WHEEL_ZOOM_EXPONENT)
        .exp()
}

fn scaled_pixel_duration(current: ChronoDuration, scale: f32) -> ChronoDuration {
    if !scale.is_finite() || scale <= 0.0 {
        return current;
    }
    let Some(current_ns) = current.num_nanoseconds() else {
        return current;
    };
    ChronoDuration::nanoseconds((current_ns as f64 / f64::from(scale)).round() as i64)
        .clamp(MIN_PIXEL_DURATION, MAX_PIXEL_DURATION)
}

fn anchored_scroll_offset(
    now: DateTime<Local>,
    anchor_time: DateTime<Local>,
    pixel_duration: ChronoDuration,
    anchor_offset: Pixels,
) -> Pixels {
    let offset_seconds = (now - anchor_time).as_seconds_f32();
    anchor_offset + px(offset_seconds / pixel_duration.as_seconds_f32())
}

fn timeline_y_rebase(
    previous_now: DateTime<Local>,
    now: DateTime<Local>,
    previous_pixel_duration: ChronoDuration,
    pixel_duration: ChronoDuration,
) -> (f32, Pixels) {
    let seconds_per_pixel = pixel_duration.as_seconds_f32();
    (
        previous_pixel_duration.as_seconds_f32() / seconds_per_pixel,
        px((previous_now - now).as_seconds_f32() / seconds_per_pixel),
    )
}

const MIN_SELECTION_FIT_SPAN: ChronoDuration = ChronoDuration::minutes(30);

fn attached_outer_label_top(boundary_center_y: Pixels) -> Pixels {
    boundary_center_y - HOUR_DIVIDER_HEIGHT / 2.
}

fn sticky_outer_label_ceiling() -> Pixels {
    TOP_EDGE_INSET
}

fn sticky_outer_label_handoff_y() -> Pixels {
    sticky_outer_label_ceiling() + HOUR_DIVIDER_HEIGHT / 2.
}

fn sticky_outer_label_top(next_boundary_center_y: Pixels) -> Pixels {
    sticky_outer_label_ceiling().min(
        attached_outer_label_top(next_boundary_center_y)
            - STICKY_OUTER_LABEL_HEIGHT
            - STICKY_OUTER_LABEL_GAP,
    )
}

fn timeline_context_menu(time: DateTime<Local>, view: Entity<TimelineView>) -> MenuBuilder {
    let label = time.format("%a %b %-d, %-I:%M %p").to_string();
    let zoom = view.clone();
    let action = view.clone();
    let event = view.clone();
    let marker = view.clone();
    let signal = view;

    MenuBuilder::new()
        .label(label)
        .item("New action", move |window, cx| {
            action.update(cx, |this, cx| this.add_draft_action(time, window, cx));
        })
        .item("New event", move |window, cx| {
            event.update(cx, |this, cx| this.add_draft_event(time, window, cx));
        })
        .separator()
        .item("New marker", move |window, cx| {
            marker.update(cx, |this, cx| this.add_draft_marker(time, window, cx));
        })
        .item("New signal", move |window, cx| {
            signal.update(cx, |this, cx| this.add_draft_signal(time, window, cx));
        })
        .separator()
        .item("Zoom to period", move |_window, cx| {
            zoom.update(cx, |this, cx| {
                let end = this.current_division_state().auto_next_boundary(time);
                this.zoom_to_span(time, end, cx);
            });
        })
}

pub(super) fn compute_edge_scroll_speed(local_y: Pixels, height: Pixels) -> Option<Pixels> {
    const ZONE: Pixels = px(96.0);
    const MAX_SPEED: f32 = 22.0;
    let y = local_y;
    let h = height;
    if y < TOP_EDGE_INSET {
        None
    } else if y < ZONE + TOP_EDGE_INSET {
        let t = 1.0 - ((y - TOP_EDGE_INSET) / ZONE).clamp(0.0, 1.0);
        Some(px(t * t * MAX_SPEED))
    } else if y > h - ZONE {
        let t = 1.0 - ((h - y) / ZONE).clamp(0.0, 1.0);
        Some(px(-(t * t * MAX_SPEED)))
    } else {
        None
    }
}

fn item_fit_span(item: &AnyItem) -> Option<(DateTime<Local>, DateTime<Local>)> {
    if let AnyItem::Signal(signal) = item {
        let instant = signal.datetime.with_timezone(&Local);
        return Some((instant, instant));
    }
    let (start, duration) = super::item_timeline_span(item)?;
    Some((start, start + duration))
}

fn fit_span_for_items<'a>(
    items: impl IntoIterator<Item = &'a AnyItem>,
) -> Option<(DateTime<Local>, DateTime<Local>)> {
    let mut spans = items.into_iter().filter_map(item_fit_span);
    let (mut start, mut end) = spans.next()?;
    for (item_start, item_end) in spans {
        start = start.min(item_start);
        end = end.max(item_end);
    }

    let span = end - start;
    if span < MIN_SELECTION_FIT_SPAN {
        let center = start + span / 2;
        start = center - MIN_SELECTION_FIT_SPAN / 2;
        end = center + MIN_SELECTION_FIT_SPAN / 2;
    }
    Some((start, end))
}

fn span_fit_pixel_duration(span: ChronoDuration, viewport_height: Pixels) -> ChronoDuration {
    let usable = (viewport_height * SPAN_FIT_FILL).max(px(120.));
    let span = span.max(ChronoDuration::minutes(1));
    let seconds_per_pixel = span.as_seconds_f32() / usable.as_f32();
    ChronoDuration::nanoseconds((seconds_per_pixel * 1_000_000_000.0) as i64)
        .clamp(MIN_PIXEL_DURATION, MAX_PIXEL_DURATION)
}

fn calendar_date_range_span(
    range: crate::dates::InclusiveDateRange,
) -> (DateTime<Local>, DateTime<Local>) {
    let start: DateTime<Local> = SchedulePoint::Date(range.start()).into();
    let end = range
        .end()
        .succ_opt()
        .map(|date| SchedulePoint::Date(date).into())
        .unwrap_or(start + ChronoDuration::days(i64::from(range.day_count())));
    (start, end)
}

impl TimelineView {
    pub fn time_to_offset(&self, time: DateTime<Local>) -> Pixels {
        let offset_secs = (time - self.now).as_seconds_f32();
        let conversion = self.pixel_duration.as_seconds_f32();
        px(offset_secs / conversion)
    }

    pub fn duration_to_height(&self, duration: ChronoDuration) -> Pixels {
        px(duration.as_seconds_f32() / self.pixel_duration.as_seconds_f32())
    }

    pub(super) fn position_to_time(&self, center_offset: Pixels) -> DateTime<Local> {
        let pos_offset = center_offset - self.scroll_offset;
        let time_offset_ns =
            self.pixel_duration.as_seconds_f32() * pos_offset.as_f32() * 1_000_000_000.0;
        self.now + ChronoDuration::nanoseconds(time_offset_ns.round() as i64)
    }

    fn pointed_subdivision_boundary_at(&self, position: Point<Pixels>) -> Option<DateTime<Local>> {
        let bounds = self.bounds?;
        let local_y = bounds.localize(&position)?.y;
        let center_offset = local_y - self.center_relative().y;
        let time = self.position_to_time(center_offset);
        Some(
            self.current_division_state()
                .auto_boundary_with_floor_fraction(time, POINTER_FLOOR_BOUNDARY_FRACTION),
        )
    }

    pub(super) fn should_render_items(&self) -> bool {
        true
    }

    pub(super) fn center_relative(&self) -> Point<Pixels> {
        self.bounds.map(|b| b.size.center()).unwrap_or_default()
    }

    pub(super) fn scroll_position(&self) -> DateTime<Local> {
        let offset_secs = self.scroll_offset.as_f32() * self.pixel_duration.as_seconds_f32();
        self.now - ChronoDuration::seconds(offset_secs.round() as i64)
    }

    pub fn scroll_to(&mut self, datetime: DateTime<Local>, cx: &mut Context<Self>) {
        self.scroll_update = Some(datetime);
        self.scroll_target = Some(datetime);
        cx.notify();
    }

    pub(super) fn scroll_by(&mut self, duration: ChronoDuration, cx: &mut Context<Self>) {
        let current = self.scroll_target.unwrap_or_else(|| self.scroll_position());
        self.scroll_to(current + duration, cx);
    }

    pub(super) fn scroll_by_px(&mut self, px: Pixels, cx: &mut Context<Self>) {
        self.scroll_update = None;
        self.scroll_target = None;
        self.scroll_cancelled = true;
        self.scroll_offset += px;
        cx.notify();
    }

    pub(super) fn scroll_reset(&mut self, cx: &mut Context<Self>) {
        self.scroll_to(self.now, cx);
    }

    pub(super) fn scroll_next_subdivision(&mut self, cx: &mut Context<Self>) {
        let delta = self.current_division_state().division_duration();
        self.scroll_by(delta, cx);
    }

    pub(super) fn scroll_previous_subdivision(&mut self, cx: &mut Context<Self>) {
        let delta = self.current_division_state().division_duration() * -1;
        self.scroll_by(delta, cx);
    }
    pub(super) fn scroll_next_division(&mut self, cx: &mut Context<Self>) {
        let delta = self
            .current_division_state()
            .base_division
            .approximate_duration();
        self.scroll_by(delta, cx);
    }

    pub(super) fn scroll_previous_division(&mut self, cx: &mut Context<Self>) {
        let delta = self
            .current_division_state()
            .base_division
            .approximate_duration()
            * -1;
        self.scroll_by(delta, cx);
    }

    pub(super) fn scroll_next_outer_division(&mut self, cx: &mut Context<Self>) {
        let delta = self
            .current_division_state()
            .base_division
            .outer_division()
            .unwrap_or(self.current_division_state().base_division)
            .approximate_duration();
        self.scroll_by(delta, cx);
    }

    pub(super) fn scroll_previous_outer_division(&mut self, cx: &mut Context<Self>) {
        let delta = self
            .current_division_state()
            .base_division
            .outer_division()
            .unwrap_or(self.current_division_state().base_division)
            .approximate_duration()
            * -1;
        self.scroll_by(delta, cx);
    }

    pub(super) fn take_zoom_target(&mut self) -> ChronoDuration {
        self.zoom_target.take().unwrap_or(self.pixel_duration)
    }

    fn pending_zoom(&self) -> ChronoDuration {
        self.zoom_target.unwrap_or(self.pixel_duration)
    }

    pub(super) fn can_zoom_in(&self) -> bool {
        self.pending_zoom() > MIN_PIXEL_DURATION
    }

    pub(super) fn can_zoom_out(&self) -> bool {
        self.pending_zoom() < MAX_PIXEL_DURATION
    }

    pub(super) fn is_zoomed(&self) -> bool {
        self.pixel_duration != PIXEL_SCALE
    }

    pub(super) fn adjust_scroll_for_zoom(
        &mut self,
        new_pixel_duration: ChronoDuration,
        offset: Pixels,
        instant: bool,
    ) {
        if self.center_relative().y == px(0.) {
            return;
        }
        let anchor_time = self.position_to_time(offset);
        self.zoom_anchor = Some(anchor_time);
        self.zoom_anchor_offset = offset;
        self.zoom_anchor_since = Some(std::time::Instant::now());
        if instant {
            self.item_y_scale =
                self.pixel_duration.as_seconds_f32() / new_pixel_duration.as_seconds_f32();
            self.scroll_offset = anchored_scroll_offset(
                self.now,
                anchor_time,
                new_pixel_duration,
                self.zoom_anchor_offset,
            );
        }
    }

    fn zoom_by_scale(&mut self, scale: f32, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(bounds) = self.bounds else {
            return;
        };
        let Some(local_position) = bounds.localize(&position) else {
            return;
        };
        let current_target = self.pending_zoom();
        let new_target = scaled_pixel_duration(current_target, scale);
        if new_target == current_target {
            return;
        }

        let anchor_offset = local_position.y - bounds.size.center().y;
        self.adjust_scroll_for_zoom(new_target, anchor_offset, false);
        self.zoom_target = Some(new_target);
        self.zoom_update = Some(new_target);
        cx.notify();
    }

    pub(super) fn handle_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delta = event.delta.pixel_delta(window.line_height());
        if event.modifiers.alt {
            if delta.y != px(0.) {
                self.zoom_by_scale(wheel_zoom_scale(delta.y), event.position, cx);
            }
            cx.stop_propagation();
            return;
        }

        if delta.y == px(0.) {
            return;
        }
        self.pending_item_focus = None;

        match event.delta {
            ScrollDelta::Pixels(_) => {
                self.scroll_by_px(delta.y, cx);
            }
            ScrollDelta::Lines(_) => {
                let duration_ns =
                    -delta.y.as_f32() * self.pixel_duration.as_seconds_f32() * 1_000_000_000.;
                self.scroll_by(ChronoDuration::nanoseconds(duration_ns.round() as i64), cx);
            }
        }
    }

    pub(super) fn handle_pinch(&mut self, event: &PinchEvent, cx: &mut Context<Self>) {
        if event.delta != 0.0 {
            self.zoom_by_scale(1.0 + event.delta, event.position, cx);
        }
        cx.stop_propagation();
    }

    pub(super) fn zoom_in(&mut self, cx: &mut Context<Self>) {
        let zoom_target = self.take_zoom_target();
        if zoom_target > MIN_PIXEL_DURATION {
            let new_target = zoom_target.as_seconds_f32() / 2.;
            let new_target_ns = new_target * 1_000_000_000.0;
            let new_duration = ChronoDuration::nanoseconds(new_target_ns as i64);
            self.adjust_scroll_for_zoom(new_duration, px(0.), false);
            self.zoom_target = Some(new_duration);
            self.zoom_update = Some(new_duration);
            cx.notify();
        }
    }

    pub(super) fn zoom_out(&mut self, cx: &mut Context<Self>) {
        let zoom_target = self.take_zoom_target();
        if zoom_target < MAX_PIXEL_DURATION {
            let new_target = zoom_target.as_seconds_f32() * 2.;
            let new_target_ns = new_target * 1_000_000_000.0;
            let new_duration = ChronoDuration::nanoseconds(new_target_ns as i64);
            self.adjust_scroll_for_zoom(new_duration, px(0.), false);
            self.zoom_target = Some(new_duration);
            self.zoom_update = Some(new_duration);
            cx.notify();
        }
    }

    pub(super) fn zoom_reset(&mut self, cx: &mut Context<Self>) {
        if self.is_zoomed() {
            self.adjust_scroll_for_zoom(PIXEL_SCALE, px(0.), false);
            self.zoom_target = Some(PIXEL_SCALE);
            self.zoom_update = self.zoom_target;
            cx.notify();
        }
    }

    fn selected_timeline_ids(&self, cx: &App) -> HashSet<uuid::Uuid> {
        let selection = SelectionManager::global(cx);
        let selection = selection.read(cx);
        if !selection.has_selection_in(SelectionScope::Timeline) {
            return HashSet::new();
        }
        selection.ids().iter().copied().collect()
    }

    pub(super) fn selection_fit_count(&self, cx: &App) -> usize {
        let ids = self.selected_timeline_ids(cx);
        self.items
            .iter()
            .filter(|entry| ids.contains(&entry.item.id()))
            .count()
            + self
                .known_signals()
                .filter(|signal| ids.contains(&signal.id))
                .count()
    }

    pub(super) fn zoom_to_selection(&mut self, cx: &mut Context<Self>) {
        let ids = self.selected_timeline_ids(cx);
        let signals: Vec<_> = self
            .known_signals()
            .filter(|signal| ids.contains(&signal.id))
            .cloned()
            .map(AnyItem::Signal)
            .collect();
        let span = fit_span_for_items(
            self.items
                .iter()
                .filter(|entry| ids.contains(&entry.item.id()))
                .map(|entry| &entry.item)
                .chain(signals.iter()),
        );
        if let Some((start, end)) = span {
            self.zoom_to_span(start, end, cx);
        }
    }

    pub(crate) fn zoom_to_item(&mut self, item: &AnyItem, cx: &mut Context<Self>) {
        if let Some((start, end)) = fit_span_for_items(std::iter::once(item)) {
            self.zoom_to_span(start, end, cx);
        }
    }

    fn set_hovered_divider(&mut self, divider: Option<DateTime<Local>>, cx: &mut Context<Self>) {
        if self.hovered_divider != divider {
            self.hovered_divider = divider;
            cx.notify();
        }
    }

    fn update_hovered_divider(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.set_hovered_divider(self.pointed_subdivision_boundary_at(position), cx);
    }

    pub(crate) fn zoom_to_date_range(
        &mut self,
        range: crate::dates::InclusiveDateRange,
        cx: &mut Context<Self>,
    ) {
        self.pending_fit = Some(calendar_date_range_span(range));
        self.zoom_anchor = None;
        self.zoom_anchor_offset = px(0.);
        self.zoom_anchor_since = None;
        cx.notify();
    }

    fn zoom_to_target(
        &mut self,
        center: DateTime<Local>,
        target: ChronoDuration,
        cx: &mut Context<Self>,
    ) {
        self.hovered_divider = None;
        self.zoom_anchor = None;
        self.zoom_anchor_offset = px(0.);
        self.zoom_anchor_since = None;
        self.zoom_target = Some(target);
        self.zoom_update = Some(target);
        self.scroll_to(center, cx);
    }

    pub(super) fn zoom_to_span(
        &mut self,
        start: DateTime<Local>,
        end: DateTime<Local>,
        cx: &mut Context<Self>,
    ) {
        let height = self.bounds.map(|b| b.size.height).unwrap_or(px(600.));
        let target = span_fit_pixel_duration(end - start, height);

        self.zoom_to_target(start + (end - start) / 2, target, cx);
    }

    fn draft_title_focused(&self, window: &Window, cx: &App) -> bool {
        ItemManager::global(cx)
            .read(cx)
            .editing_item
            .as_ref()
            .is_some_and(|editing| {
                editing.is_draft()
                    && self.items.iter().any(|item| item.item.id() == editing.id())
                    && editing.input.read(cx).focus_handle(cx).is_focused(window)
            })
    }

    pub(super) fn render_hover_layer(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let hovered_divider = self
            .hovered_divider
            .filter(|_| !self.draft_title_focused(window, cx));

        div()
            .absolute()
            .inset_0()
            .id("timeline-hover")
            .on_hover(cx.listener(|view, is_hovered, window, cx| {
                if *is_hovered {
                    view.update_hovered_divider(window.mouse_position(), cx);
                } else {
                    view.set_hovered_divider(None, cx);
                }
            }))
            .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _, cx| {
                view.update_hovered_divider(event.position, cx);
            }))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .h_full()
                    .left(self.item_area_left())
                    .w(self.item_area_width())
                    .id("timeline-item-lane")
                    .on_hover(cx.listener(|_, _, _, cx| cx.notify())),
            )
            .children(
                hovered_divider.and_then(|time| self.render_hover_create_button(time, window, cx)),
            )
    }

    fn render_hover_create_button(
        &self,
        time: DateTime<Local>,
        window: &Window,
        cx: &Context<Self>,
    ) -> Option<Button> {
        let pointer = self.bounds?.localize(&window.mouse_position())?;
        if !self.loaded
            || pointer.x < self.item_area_left()
            || pointer.x >= self.item_area_left() + self.item_area_width()
            || self.active_drop.is_some()
            || self.active_resize.is_some()
            || self.marquee.is_active()
        {
            return None;
        }

        let diameter = px(24.);
        let y = self.time_to_offset(time) + self.scroll_offset + self.center_relative().y;

        let colors = &cx.theme().colors;
        let hover_background = colors.control_hover.alpha(0.8);
        let hover_foreground = colors.text;

        Some(
            Button::new(("timeline-create-action", time.timestamp() as u64))
                .ghost()
                .compact()
                .absolute()
                .top(y - diameter / 2.)
                .left(self.item_area_left() + (self.item_area_width() - diameter) / 2.)
                .size(diameter)
                .rounded_full()
                .bg(colors.control.alpha(0.55))
                .text_color(colors.text_muted.alpha(0.7))
                .hover(move |style| style.bg(hover_background).text_color(hover_foreground))
                .icon(Icon::new(AppIcon::Plus).size_4())
                .tooltip("New action")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|view, _, _, cx| {
                        view.force_press = None;
                        cx.stop_propagation();
                    }),
                )
                .on_mouse_pressure(|_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                    cx.stop_propagation();
                    if !view.suppress_force_click && event.click_count() == 1 {
                        view.add_draft_action(time, window, cx);
                    }
                })),
        )
    }

    pub(super) fn render_backdrop(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let previous_pixel_duration = self.pixel_duration;
        let previous_now = self.item_geometry_now;
        let zoom_transition = window.keyed_transition("timeline-zoom", cx, ZOOM, || {
            self.pixel_duration.as_seconds_f32()
        });
        if let Some(update) = self.zoom_update.take() {
            zoom_transition.set(update.as_seconds_f32(), cx);
        }
        let ns = zoom_transition.animate(window, cx) * 1_000_000_000.;
        self.pixel_duration = ChronoDuration::nanoseconds(ns as i64);
        (self.item_y_scale, self.item_y_offset) = timeline_y_rebase(
            previous_now,
            self.now,
            previous_pixel_duration,
            self.pixel_duration,
        );
        self.item_geometry_now = self.now;

        let scroll_offset_secs = self.scroll_offset.as_f32() * self.pixel_duration.as_seconds_f32();
        let scroll_transition =
            window.keyed_transition("timeline-scroll", cx, transition::SCROLL, || {
                scroll_offset_secs
            });
        let requested = self.scroll_update.take();
        if let Some(update) = requested {
            let update_offset_duration = self.now - update;
            self.scroll_cancelled = false;
            scroll_transition.snap(scroll_offset_secs, cx);
            scroll_transition.set(update_offset_duration.as_seconds_f32(), cx);
        } else if self.scroll_cancelled {
            scroll_transition.snap(scroll_offset_secs, cx);
            self.scroll_cancelled = false;
        }
        let transition_owns_scroll = requested.is_some() || scroll_transition.is_animating(cx);
        let transitioned_offset_secs = scroll_transition.animate(window, cx);
        if transition_owns_scroll {
            self.scroll_offset =
                px(transitioned_offset_secs / self.pixel_duration.as_seconds_f32());
        }
        if !scroll_transition.is_animating(cx) {
            self.scroll_target = None;
        }

        if let Some(speed) = self.edge_scroll_speed {
            self.scroll_by_px(speed, cx);
            let pointer = window.mouse_position();
            if self.marquee.is_active() {
                self.marquee.scrolled_by(point(px(0.), speed));
                self.marquee.drag_to(pointer, cx);
            }
            if self.force_create.is_some() {
                self.update_force_create(pointer, cx);
            } else if self.active_resize.is_some() {
                self.update_resize_preview_at(pointer, cx);
            } else if self.active_drop.is_some()
                && !self.marquee.is_active()
                && self.drop_dragged.is_some()
            {
                self.retarget_drop_preview_at(pointer, cx);
            }
        }

        if let Some(anchor_time) = self.zoom_anchor
            && self.center_relative().y != px(0.)
        {
            self.scroll_offset = anchored_scroll_offset(
                self.now,
                anchor_time,
                self.pixel_duration,
                self.zoom_anchor_offset,
            );
        }
        if self
            .zoom_anchor_since
            .is_some_and(|s| s.elapsed() > ZOOM.total() + Duration::from_millis(50))
        {
            self.zoom_anchor = None;
            self.zoom_anchor_offset = px(0.);
            self.zoom_anchor_since = None;
        }

        let horizon_secs =
            (self.bounds.unwrap_or_default().size.height + EDGE_HORIZON * 2.).as_f32() / 2.
                * self.pixel_duration.as_seconds_f32();
        let horizon = ChronoDuration::seconds(horizon_secs.round() as i64);
        let scroll_position = self.scroll_position();
        let start = scroll_position - horizon;
        let end = scroll_position + horizon;
        let base_division = self.current_division_state().base_division;
        let floor = base_division.floor_boundary(start);

        let num_divisions =
            (end - start).as_seconds_f32() / base_division.approximate_duration().as_seconds_f32();
        let num_divisions = (num_divisions as usize).min(1000);

        let mut divisions = vec![floor];
        let mut current = floor;
        for _ in 0..=num_divisions {
            current = base_division.next_boundary(current);
            divisions.push(current);
        }

        let division_state = self.current_division_state();
        let mut slot_start = division_state.auto_floor_boundary(start);
        let mut slots = Vec::new();
        const MAX_VISIBLE_SLOTS: usize = 1000;
        while slot_start <= end && slots.len() < MAX_VISIBLE_SLOTS {
            let slot_end = division_state.auto_next_boundary(slot_start);
            if slot_end <= slot_start {
                break;
            }
            slots.push((slot_start, slot_end));
            slot_start = slot_end;
        }

        let hovered_divider = self
            .hovered_divider
            .filter(|_| !self.draft_title_focused(window, cx));
        let entity = cx.entity();
        let backdrop_entity = entity.clone();
        div()
            .absolute()
            .inset_0()
            .id("timeline-backdrop")
            .on_double_click(move |event: &ClickEvent, window, cx| {
                backdrop_entity.update(cx, |view, cx| {
                    if view.suppress_force_click {
                        return Some(());
                    }
                    let start = view.pointed_subdivision_boundary_at(event.position())?;
                    view.add_draft_from_double_click(start, window, cx);
                    Some(())
                });
            })
            .on_mouse_pressure(
                cx.listener(|view, event: &MousePressureEvent, _window, cx| {
                    view.pressure_changed(event.stage, event.position, cx);
                }),
            )
            .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _window, cx| {
                if view.force_create.is_some() && event.pressed_button == Some(MouseButton::Left) {
                    view.update_force_create(event.position, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, event: &MouseUpEvent, window, cx| {
                    view.finish_force_create(event.position, window, cx);
                }),
            )
            .on_aux_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                if event.is_right_click() {
                    let Some(slot_time) = view.pointed_subdivision_boundary_at(event.position())
                    else {
                        return;
                    };

                    crate::components::menu::open_context_menu(
                        timeline_context_menu(slot_time, cx.entity().clone()),
                        event.position(),
                        window,
                        cx,
                    );
                    cx.notify();
                }
            }))
            .children(slots.into_iter().map(|(slot_start, slot_end)| {
                let y =
                    self.time_to_offset(slot_start) + self.scroll_offset + self.center_relative().y;
                let height = self.duration_to_height(slot_end - slot_start);
                let key = slot_start.timestamp() as u64;
                let create = entity.clone();
                let menu = entity.clone();

                div()
                    .absolute()
                    .top(y)
                    .left_0()
                    .right_0()
                    .h(height)
                    .id(("timeline-slot", key))
                    .on_double_click(move |event: &ClickEvent, window, cx| {
                        cx.stop_propagation();
                        create.update(cx, |view, cx| {
                            if !view.suppress_force_click {
                                let start = view
                                    .pointed_subdivision_boundary_at(event.position())
                                    .unwrap_or(slot_start);
                                view.add_draft_from_double_click(start, window, cx);
                            }
                        });
                    })
                    .on_aux_click(move |event: &ClickEvent, window, cx| {
                        if event.is_right_click() {
                            cx.stop_propagation();
                            let menu_time = menu
                                .read(cx)
                                .pointed_subdivision_boundary_at(event.position())
                                .unwrap_or(slot_start);
                            crate::components::menu::open_context_menu(
                                timeline_context_menu(menu_time, menu.clone()),
                                event.position(),
                                window,
                                cx,
                            );
                        }
                    })
            }))
            .children(divisions.iter().copied().map(|time| {
                let outer_period = base_division
                    .outer_period_label_at(time)
                    .filter(|period| period.start == time);
                let is_outer = outer_period.is_some();
                let is_hovered_divider = hovered_divider == Some(time);
                let hover_divider = cx.theme().colors.hairline_strong;
                let base_end = base_division.next_boundary(time);
                let fit_base = entity.clone();
                let fit_outer = entity.clone();
                let period_hover = cx.theme().colors.hover.alpha(0.3);
                let text_muted = cx.theme().colors.text_muted;

                let sub_divisions = self.sub_divisions(time);
                let y = self.time_to_offset(time) + self.scroll_offset + self.center_relative().y
                    - HOUR_DIVIDER_HEIGHT / 2.;
                div()
                    .row()
                    .absolute()
                    .top(y)
                    .w_full()
                    .h(HOUR_DIVIDER_HEIGHT)
                    .gap_2()
                    .child(
                        Divider::horizontal()
                            .w_2()
                            .stroke(px(2.))
                            .when(is_hovered_divider, |divider| divider.color(hover_divider)),
                    )
                    .child(
                        div()
                            .id(("timeline-fit-period", time.timestamp() as u64))
                            .px_1()
                            .rounded_md()
                            .cursor_pointer()
                            .block_mouse_except_scroll()
                            .hover(move |style| style.bg(period_hover))
                            .on_click(move |_: &ClickEvent, _window, cx| {
                                cx.stop_propagation();
                                fit_base
                                    .update(cx, |view, cx| view.zoom_to_span(time, base_end, cx));
                            })
                            .child(base_division.base_label_style().label(time, false, cx)),
                    )
                    .when(is_outer, |this| {
                        this.child(
                            Divider::horizontal()
                                .flex_1()
                                .stroke(px(2.))
                                .when(is_hovered_divider, |divider| divider.color(hover_divider)),
                        )
                        .when_some(outer_period, move |this, period| {
                            let start = period.start;
                            let end = period.end;
                            this.child(
                                div()
                                    .id((
                                        "timeline-fit-outer-period",
                                        period.start.timestamp() as u64,
                                    ))
                                    .px_1()
                                    .mr_2()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .block_mouse_except_scroll()
                                    .hover(move |style| style.bg(period_hover))
                                    .on_click(move |_: &ClickEvent, _window, cx| {
                                        cx.stop_propagation();
                                        fit_outer.update(cx, |view, cx| {
                                            view.zoom_to_span(start, end, cx)
                                        });
                                    })
                                    .child(
                                        Label::new(period.text).text_color(text_muted).text_xl(),
                                    ),
                            )
                        })
                    })
                    .when(!is_outer, |this| {
                        this.child(
                            Divider::horizontal()
                                .flex_1()
                                .dashed()
                                .when(is_hovered_divider, |divider| divider.color(hover_divider)),
                        )
                    })
                    .when_some(sub_divisions, |this, sub_divisions| {
                        let subdivision =
                            self.current_division_state().current_subdivision().unwrap();

                        this.children(sub_divisions.iter().copied().map(|sub_time| {
                            let offset_duration = sub_time - time;
                            let base_offset = px(offset_duration.as_seconds_f32()
                                / self.pixel_duration.as_seconds_f32());
                            let label = subdivision
                                .style()
                                .label(sub_time, true, cx)
                                .text_xs()
                                .text_color(cx.theme().colors.text_muted);
                            let sub_end = subdivision.next_boundary(sub_time);
                            let is_hovered_divider = hovered_divider == Some(sub_time);
                            let fit_subdivision = entity.clone();
                            div()
                                .row()
                                .absolute()
                                .top(base_offset)
                                .w_full()
                                .h(HOUR_DIVIDER_HEIGHT)
                                .gap_2()
                                .child(
                                    Divider::horizontal()
                                        .w_2()
                                        .stroke(px(1.))
                                        .when(is_hovered_divider, |divider| {
                                            divider.color(hover_divider)
                                        }),
                                )
                                .child(
                                    div()
                                        .id((
                                            "timeline-fit-subdivision",
                                            sub_time.timestamp() as u64,
                                        ))
                                        .px_1()
                                        .rounded_md()
                                        .cursor_pointer()
                                        .block_mouse_except_scroll()
                                        .hover(move |style| style.bg(period_hover))
                                        .on_click(move |_: &ClickEvent, _window, cx| {
                                            cx.stop_propagation();
                                            fit_subdivision.update(cx, |view, cx| {
                                                view.zoom_to_span(sub_time, sub_end, cx)
                                            });
                                        })
                                        .child(label),
                                )
                        }))
                    })
            }))
    }

    pub(super) fn render_sticky_outer_label(&self, cx: &Context<Self>) -> Option<impl IntoElement> {
        let division = self.current_division_state().base_division;
        let bounds = self.bounds?;
        let center_y = bounds.size.center().y;
        let compact = bounds.size.width < px(560.);

        let sample_y = sticky_outer_label_handoff_y();
        let sample_time = self.position_to_time(sample_y - center_y);
        let period = division.outer_period_label_at(sample_time)?;
        let outer_start = period.start;
        let outer_end = period.end;
        let (primary_label, secondary_label) = match period.text.split_once(' ') {
            Some((primary, secondary)) => (primary.to_string(), Some(secondary.to_string())),
            None => (period.text, None),
        };

        let next_boundary_center_y = self.time_to_offset(outer_end) + self.scroll_offset + center_y;

        let y = sticky_outer_label_top(next_boundary_center_y);

        Some(
            div()
                .row()
                .absolute()
                .top(y)
                .right(STICKY_OUTER_LABEL_INSET)
                .h(STICKY_OUTER_LABEL_HEIGHT)
                .items_center()
                .child(
                    Button::new(("timeline-fit-sticky-period", outer_start.timestamp() as u64))
                        .glass_pill()
                        .h_10()
                        .px_4()
                        .when(compact, |this| this.h_8().px_2())
                        .block_mouse_except_scroll()
                        .gap_1()
                        .child(
                            Label::new(primary_label)
                                .text_2xl()
                                .when(compact, |this| this.text_base()),
                        )
                        .when_some(secondary_label, |this, secondary| {
                            this.child(
                                Label::new(secondary)
                                    .text_2xl()
                                    .when(compact, |this| this.text_base())
                                    .font_weight(FontWeight::EXTRA_LIGHT),
                            )
                        })
                        .on_click(cx.listener(move |view, _: &ClickEvent, _window, cx| {
                            cx.stop_propagation();
                            view.zoom_to_span(outer_start, outer_end, cx);
                        })),
                ),
        )
    }

    pub(super) fn render_now_shortcut(
        &self,
        top: Pixels,
        cx: &Context<Self>,
    ) -> Option<impl IntoElement> {
        let height = self.bounds?.size.height;
        let cursor_y = self.center_relative().y + self.scroll_offset;
        if height <= TOP_EDGE_INSET || (TOP_EDGE_INSET..=height).contains(&cursor_y) {
            return None;
        }

        let format = match self.current_division_state().base_division {
            BaseTimeDivision::Minute => "%-I:%M:%S %p",
            BaseTimeDivision::FiveMinutes | BaseTimeDivision::Hour => "%-I:%M %p",
            BaseTimeDivision::Day => "%b %-d",
            BaseTimeDivision::Month => "%B %Y",
            BaseTimeDivision::Year => "%Y",
        };

        Some(
            div()
                .absolute()
                .top(top)
                .left(STICKY_OUTER_LABEL_INSET)
                .child(
                    Button::new("timeline-now-shortcut")
                        .glass_pill()
                        .small()
                        .icon(AppIcon::Clock)
                        .label(format!("Now · {}", self.now.format(format)))
                        .tooltip(format!(
                            "Back to now · {}",
                            self.now.format("%a %b %-d, %Y · %-I:%M:%S %p")
                        ))
                        .text_color(cx.theme().colors.text)
                        .block_mouse_except_scroll()
                        .on_click(cx.listener(|view, _, _, cx| {
                            cx.stop_propagation();
                            view.scroll_reset(cx);
                        })),
                ),
        )
    }

    pub(super) fn render_now_cursor(&self, cx: &Context<Self>) -> impl IntoElement {
        let color = UxColor::CurrentTime.color(cx.theme());
        let on_fill = UxColor::CurrentTime.on_fill(cx.theme());

        let time_label = match self.current_division_state().base_division {
            BaseTimeDivision::Minute => self.now.format("%-I:%M:%S").to_string(),
            BaseTimeDivision::FiveMinutes | BaseTimeDivision::Hour | BaseTimeDivision::Day => {
                self.now.format("%-I:%M").to_string()
            }
            BaseTimeDivision::Month => self.now.format("%b %-d").to_string(),
            BaseTimeDivision::Year => self.now.format("%B").to_string(),
        };

        div()
            .row()
            .absolute()
            .top(self.scroll_offset - HOUR_DIVIDER_HEIGHT / 2. + self.center_relative().y)
            .w_full()
            .h(HOUR_DIVIDER_HEIGHT)
            .child(Divider::horizontal().color(color).w_2())
            .child(
                div()
                    .px(px(7.))
                    .bg(color)
                    .rounded_xl()
                    .child(Label::new(time_label).text_sm().text_color(on_fill)),
            )
            .child(
                div()
                    .row()
                    .w_full()
                    .child(Divider::horizontal().color(color).flex_1())
                    .when(false, |this| this),
            )
    }
}
