use chrono::{DateTime, Local};
use gpui::{
    App, Context, DragMoveEvent, Element, ElementId, Entity, GlobalElementId, IntoElement,
    MouseUpEvent, ParentElement, Pixels, Render, Styled, Window, div, prelude::FluentBuilder, px,
};
use gpui_kit_theme::ActiveTheme;
use subroutine_core::{Action, AnyItem, SchedulePoint};
use uuid::Uuid;

use crate::{
    haptics::HapticsExt as _,
    stores::AppDatabaseStore,
    views::{
        TOP_EDGE_INSET,
        main_view::timeline_view::{
            FALLBACK_ITEM_DURATION, measure_from, timeline::compute_edge_scroll_speed,
        },
    },
};

use super::super::{Lane, TimelineView, is_durationless_action, item_min_height};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ResizeEdge {
    Top,
    Bottom,
}

#[derive(Clone, Debug)]
pub(crate) struct ResizeDragData {
    pub item_id: Uuid,
    pub edge: ResizeEdge,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ActiveResizeState {
    pub item_id: Uuid,
    pub edge: ResizeEdge,
    pub original_time: DateTime<Local>,
    pub original_end: DateTime<Local>,
    pub new_time: DateTime<Local>,
    pub new_end: DateTime<Local>,
    min_height: Pixels,
}

impl ActiveResizeState {
    pub(super) fn min_height(&self) -> Pixels {
        self.min_height
    }
}

pub(super) struct ResizeGhost;
impl Render for ResizeGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

pub(crate) struct ResizeDragMouseUpHook(pub Entity<TimelineView>);

impl IntoElement for ResizeDragMouseUpHook {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for ResizeDragMouseUpHook {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui::LayoutId, ()) {
        (window.request_layout(gpui::Style::default(), [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: gpui::Bounds<Pixels>,
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: gpui::Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        _cx: &mut App,
    ) {
        let entity = self.0.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, phase, _window, cx| {
            if phase.bubble() {
                entity.update(cx, |view, cx| {
                    view.commit_resize_state(cx);
                });
            }
        });
    }
}

impl TimelineView {
    fn resize_state_at(
        &self,
        item_id: Uuid,
        edge: ResizeEdge,
        mouse_pos: gpui::Point<Pixels>,
    ) -> Option<ActiveResizeState> {
        if self.item_details.is_open(item_id) {
            return None;
        }
        let local_pos = self
            .bounds
            .and_then(|bounds| bounds.localize(&mouse_pos))
            .filter(|position| position.y >= TOP_EDGE_INSET)?;
        let ti = self.items.iter().find(|i| i.item.id() == item_id)?;
        let original_time = ti.item.start_datetime()?;
        let durationless_action = is_durationless_action(&ti.item);
        let original_duration = ti
            .item
            .duration()
            .map(|d| measure_from(original_time, d))
            .unwrap_or(FALLBACK_ITEM_DURATION);
        let original_end = original_time + original_duration;

        let center = self.center_relative().y;
        let offset = local_pos.y - center;
        let raw_time = self.position_to_time(offset);
        let state = self.current_division_state();
        let base = state.base_division;
        let sub = state.current_subdivision();
        let snapped = sub
            .map(|s| s.nearest_boundary(raw_time))
            .unwrap_or_else(|| base.nearest_boundary(raw_time));
        let (new_time, new_end) = if durationless_action {
            clamp_durationless_resize(edge, original_time, original_end, snapped)
        } else {
            clamp_resize_to_adjacent_boundary(edge, original_time, original_end, snapped, &state)
        };

        Some(ActiveResizeState {
            item_id,
            edge,
            original_time,
            original_end,
            new_time,
            new_end,
            min_height: item_min_height(&ti.item),
        })
    }

    fn set_resize_preview(&mut self, new_info: ActiveResizeState, cx: &mut Context<Self>) {
        if self.active_resize.as_ref() != Some(&new_info) {
            if self.active_resize.is_some() {
                cx.play_alignment_haptic();
            }
            self.active_resize = Some(new_info);
            cx.notify();
        }
    }

    pub(in crate::views::main_view::timeline_view) fn update_resize_preview_at(
        &mut self,
        mouse_pos: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(active) = self.active_resize.as_ref() else {
            return;
        };
        let item_id = active.item_id;
        let edge = active.edge;
        if let Some(new_info) = self.resize_state_at(item_id, edge, mouse_pos) {
            self.set_resize_preview(new_info, cx);
        }
    }

    pub(crate) fn handle_resize_move(
        &mut self,
        event: &DragMoveEvent<ResizeDragData>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mouse_pos = window.mouse_position();
        let local_pos = self
            .bounds
            .and_then(|bounds| bounds.localize(&mouse_pos))
            .filter(|position| position.y >= TOP_EDGE_INSET);

        let Some(local_pos) = local_pos else {
            if self.active_resize.is_some() || self.edge_scroll_speed.is_some() {
                self.active_resize = None;
                self.edge_scroll_speed = None;
                cx.notify();
            }
            return;
        };

        let data = event.drag(cx);
        let item_id = data.item_id;
        let edge = data.edge;
        let Some(new_info) = self.resize_state_at(item_id, edge, mouse_pos) else {
            return;
        };
        self.set_resize_preview(new_info, cx);

        let new_speed = self
            .bounds
            .and_then(|b| compute_edge_scroll_speed(local_pos.y, b.size.height));
        if new_speed != self.edge_scroll_speed {
            self.edge_scroll_speed = new_speed;
            cx.notify();
        }
    }

    pub(crate) fn commit_resize_state(&mut self, cx: &mut Context<Self>) {
        self.edge_scroll_speed = None;
        let Some(resize) = self.active_resize.take() else {
            return;
        };
        cx.notify();

        let new_duration = resize.new_end - resize.new_time;
        let new_time_utc = resize.new_time.with_timezone(&chrono::Utc);

        let Some(ti) = self
            .items
            .iter()
            .find(|item| item.item.id() == resize.item_id)
        else {
            return;
        };
        let store = AppDatabaseStore::global(cx);
        match ti.item.clone() {
            AnyItem::Action(action) => {
                let action = resized_action(action, new_time_utc, new_duration);
                let _ = store.update(cx, |s, cx| s.upsert_action(action, cx));
            }
            AnyItem::Event(mut event) => {
                event.start = new_time_utc;
                event.duration = new_duration.into();
                let _ = store.update(cx, |s, cx| s.upsert_event(event, cx));
            }
            AnyItem::Routine(_) => (),
            AnyItem::Marker(_) => (),
            AnyItem::Signal(_) => (),
            AnyItem::ActionTemplate(_) | AnyItem::EventTemplate(_) => (),
        };
    }

    pub(crate) fn render_active_resize(
        &self,
        resize: &ActiveResizeState,
        lane: Lane,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let bounds = self.lane_bounds_with_min_height(
            resize.new_time,
            resize.new_end - resize.new_time,
            lane,
            resize.min_height(),
        );
        let y = bounds.top() + self.scroll_offset + self.center_relative().y;
        let h = bounds.size.height;
        let color = cx.theme().colors.focus;
        let label_text = match resize.edge {
            ResizeEdge::Top => resize.new_time.format("%-I:%M").to_string(),
            ResizeEdge::Bottom => resize.new_end.format("%-I:%M").to_string(),
        };
        div()
            .absolute()
            .top(y)
            .h(h)
            .left(bounds.left())
            .w(bounds.size.width)
            .border_1()
            .border_dashed()
            .border_color(color)
            .rounded_lg()
            .map(|this| match resize.edge {
                ResizeEdge::Top => this.flex().items_start().justify_start().pl_2().pt_1(),
                ResizeEdge::Bottom => this.flex().items_end().justify_start().pl_2().pb_1(),
            })
            .child(
                div()
                    .px(px(6.))
                    .bg(cx.theme().colors.canvas.alpha(0.85))
                    .rounded_md()
                    .text_sm()
                    .text_color(color)
                    .child(label_text),
            )
    }
}

fn resized_action(
    action: Action,
    start: DateTime<chrono::Utc>,
    duration: chrono::Duration,
) -> Action {
    action
        .with_queued(true)
        .with_start(Some(SchedulePoint::DateTime(start)))
        .with_duration(Some(duration.into()))
}

fn clamp_durationless_resize(
    edge: ResizeEdge,
    original_time: DateTime<Local>,
    original_end: DateTime<Local>,
    snapped: DateTime<Local>,
) -> (DateTime<Local>, DateTime<Local>) {
    match edge {
        ResizeEdge::Top => (snapped.min(original_time), original_end),
        ResizeEdge::Bottom => (original_time, snapped.max(original_end)),
    }
}

fn clamp_resize_to_adjacent_boundary(
    edge: ResizeEdge,
    original_time: DateTime<Local>,
    original_end: DateTime<Local>,
    snapped: DateTime<Local>,
    divisions: &super::super::TimeDivisionState,
) -> (DateTime<Local>, DateTime<Local>) {
    match edge {
        ResizeEdge::Top => (
            snapped.min(divisions.auto_previous_boundary(original_end)),
            original_end,
        ),
        ResizeEdge::Bottom => (
            original_time,
            snapped.max(divisions.auto_next_boundary(original_time)),
        ),
    }
}
