use chrono::{DateTime, Duration as ChronoDuration, Local};
use gpui::{
    Context, Entity, IntoElement, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    PressureStage, Window, canvas, px,
};

use crate::{
    haptics::HapticsExt as _,
    selection::{SelectionManager, SelectionScope},
    settings::{Settings, TimelineCreationKind},
};

use super::super::{MIN_ITEM_HEIGHT, POINTER_FLOOR_BOUNDARY_FRACTION, TimelineView};
use super::{ActiveDropState, DropShape};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::views::main_view::timeline_view) struct ForceCreateState {
    anchor: DateTime<Local>,
    pointer: DateTime<Local>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ForceCreateCommit {
    Signal(DateTime<Local>),
    Span {
        start: DateTime<Local>,
        duration: ChronoDuration,
    },
}

impl ForceCreateState {
    fn new(anchor: DateTime<Local>) -> Self {
        Self {
            anchor,
            pointer: anchor,
        }
    }

    fn move_to(&mut self, pointer: DateTime<Local>) {
        self.pointer = pointer;
    }

    fn boundaries(self) -> (DateTime<Local>, DateTime<Local>) {
        (self.anchor.min(self.pointer), self.anchor.max(self.pointer))
    }

    fn commit(self) -> ForceCreateCommit {
        let (start, end) = self.boundaries();
        if start == end {
            ForceCreateCommit::Signal(start)
        } else {
            ForceCreateCommit::Span {
                start,
                duration: end - start,
            }
        }
    }

    fn preview(self) -> ActiveDropState {
        let (start, end) = self.boundaries();
        let shape = if start == end {
            DropShape::Moment {
                offset: ChronoDuration::zero(),
            }
        } else {
            DropShape::Span {
                lead: ChronoDuration::zero(),
                duration: Some(end - start),
            }
        };
        ActiveDropState {
            dragged: Vec::new(),
            drop_time: start,
            shape,
            min_height: MIN_ITEM_HEIGHT,
            slot_visual_start: start,
            slot_visual_end: end,
        }
    }
}

impl TimelineView {
    pub(in crate::views::main_view::timeline_view) fn begin_force_press(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let pressed_item = SelectionManager::global(cx)
            .read(cx)
            .card_at(SelectionScope::Timeline, position);
        self.inspect_press_item = pressed_item;
        self.force_press =
            (pressed_item.is_none() && self.active_drop.is_none() && self.active_resize.is_none())
                .then_some(position);
    }

    pub(in crate::views::main_view::timeline_view) fn pressure_changed(
        &mut self,
        stage: PressureStage,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if stage != PressureStage::Force {
            return;
        }

        if self.force_create.is_some() {
            self.update_force_create(position, cx);
            return;
        }
        let Some(press_position) = self.force_press else {
            return;
        };
        if self.active_drop.is_some() || self.active_resize.is_some() {
            return;
        }

        let Some(raw_time) = self.time_at_timeline_position(press_position) else {
            return;
        };
        let boundary = self
            .current_division_state()
            .auto_boundary_with_floor_fraction(raw_time, POINTER_FLOOR_BOUNDARY_FRACTION);
        let state = ForceCreateState::new(boundary);

        self.marquee.end();
        SelectionManager::clear_global(cx);
        self.edge_scroll_speed = None;
        self.drop_dragged = None;
        self.suppress_force_click = true;
        self.force_create = Some(state);
        self.active_drop = Some(state.preview());
        self.hovered_divider = Some(boundary);
        cx.notify();
    }

    pub(in crate::views::main_view::timeline_view) fn update_force_create(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(raw_time) = self.time_at_timeline_position(position) else {
            return;
        };
        let boundary = self
            .current_division_state()
            .auto_boundary_with_floor_fraction(raw_time, POINTER_FLOOR_BOUNDARY_FRACTION);
        let Some(mut state) = self.force_create else {
            return;
        };
        let before = state.pointer;
        state.move_to(boundary);

        if before != boundary {
            cx.play_alignment_haptic();
        }
        self.force_create = Some(state);
        self.active_drop = Some(state.preview());
        let divider_changed = self.hovered_divider != Some(boundary);
        self.hovered_divider = Some(boundary);

        let next_speed = self.bounds.and_then(|bounds| {
            let local_y = position.y - bounds.origin.y;
            super::super::timeline::compute_edge_scroll_speed(local_y, bounds.size.height)
        });
        if self.edge_scroll_speed != next_speed || before != boundary || divider_changed {
            self.edge_scroll_speed = next_speed;
            cx.notify();
        }
    }

    pub(in crate::views::main_view::timeline_view) fn finish_force_create(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.force_create.is_none() {
            self.force_press = None;
            return;
        }
        self.update_force_create(position, cx);
        let Some(state) = self.force_create.take() else {
            return;
        };
        self.active_drop = None;
        self.edge_scroll_speed = None;
        self.force_press = None;
        match state.commit() {
            ForceCreateCommit::Signal(time) => self.add_draft_signal(time, window, cx),
            ForceCreateCommit::Span { start, duration } => {
                match Settings::global(cx).timeline_creation.force_drag {
                    TimelineCreationKind::Action => {
                        self.add_draft_action_with_duration(start, Some(duration), window, cx);
                    }
                    TimelineCreationKind::Event => {
                        self.add_draft_event_with_duration(start, duration, window, cx);
                    }
                }
            }
        }

        cx.spawn(async move |view, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(1))
                .await;
            let _ = view.update(cx, |view, _| view.suppress_force_click = false);
        })
        .detach();
        cx.notify();
    }

    fn time_at_timeline_position(&self, position: Point<Pixels>) -> Option<DateTime<Local>> {
        let bounds = self.bounds?;
        let local_y = (position.y - bounds.origin.y).clamp(px(0.), bounds.size.height);
        let center_offset = local_y - self.center_relative().y;
        Some(self.position_to_time(center_offset))
    }
}

pub(in crate::views::main_view::timeline_view) fn force_create_hook(
    entity: Entity<TimelineView>,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, _cx| {
            let moved = entity.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if !phase.bubble() {
                    return;
                }
                match event.pressed_button {
                    Some(MouseButton::Left) => moved.update(cx, |view, cx| {
                        view.update_force_create(event.position, cx);
                    }),
                    None => moved.update(cx, |view, cx| {
                        view.finish_force_create(event.position, window, cx);
                    }),
                    _ => {}
                }
            });

            let released = entity.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                if phase.bubble() && event.button == MouseButton::Left {
                    released.update(cx, |view, cx| {
                        view.finish_force_create(event.position, window, cx);
                    });
                }
            });
        },
    )
}
