use std::time::Duration;

use gpui::{AsyncApp, Context, ScrollWheelEvent, TouchPhase, Window};

use crate::item_manager::ItemManager;

use super::FocusView;

const PIXEL_GESTURE_THRESHOLD_PX: f32 = 18.0;
const PIXEL_GESTURE_END_DELAY: Duration = Duration::from_millis(140);
const PIXEL_GESTURE_IDLE_DELAY: Duration = Duration::from_millis(350);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CarouselScrollAxis {
    Horizontal,
    Vertical,
}

fn pixel_gesture_step(accumulated: f32, delta: f32) -> (f32, isize) {
    let accumulated = accumulated + delta;
    let step = if accumulated.abs() >= PIXEL_GESTURE_THRESHOLD_PX {
        coarse_scroll_step(accumulated)
    } else {
        0
    };
    (accumulated, step)
}

fn scroll_axis(horizontal: f32, vertical: f32, shift: bool) -> Option<CarouselScrollAxis> {
    if shift && vertical != 0.0 {
        Some(CarouselScrollAxis::Vertical)
    } else if horizontal == 0.0 && vertical == 0.0 {
        None
    } else if horizontal.abs() >= vertical.abs() {
        Some(CarouselScrollAxis::Horizontal)
    } else {
        Some(CarouselScrollAxis::Vertical)
    }
}

fn coarse_scroll_step(delta: f32) -> isize {
    if delta < 0.0 {
        1
    } else if delta > 0.0 {
        -1
    } else {
        0
    }
}

impl FocusView {
    pub(super) fn reset_wheel(&mut self) {
        self.wheel_active = false;
        self.wheel_axis = None;
        self.pixel_gesture_travel = 0.0;
        self.pixel_gesture_navigated = false;
        self.wheel_phase_ended = false;
        self.wheel_generation = self.wheel_generation.wrapping_add(1);
    }

    pub(super) fn handle_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ItemManager::global(cx).read(cx).is_editing() {
            return;
        }
        if event.touch_phase == TouchPhase::Started {
            self.reset_wheel();
            self.wheel_active = true;
        }
        if event.touch_phase == TouchPhase::Cancelled {
            self.carousel_overscroll.reset();
            if self.wheel_axis.is_some() || self.wheel_active {
                self.reset_wheel();
                cx.stop_propagation();
            }
            return;
        }

        let delta = event.delta.pixel_delta(window.line_height());
        if self.wheel_axis.is_none() {
            self.wheel_axis = scroll_axis(
                f32::from(delta.x),
                f32::from(delta.y),
                event.modifiers.shift,
            );
        }
        let Some(axis) = self.wheel_axis else {
            return;
        };
        let travel = match axis {
            CarouselScrollAxis::Horizontal => delta.x,
            CarouselScrollAxis::Vertical => delta.y,
        };

        let ids = self.navigable_ids(cx);
        let current = self
            .carousel_active_id()
            .and_then(|active| ids.iter().position(|id| *id == active))
            .unwrap_or_default();
        if ids.is_empty() {
            return;
        }

        if !event.delta.precise() {
            let step = coarse_scroll_step(f32::from(travel));
            cx.stop_propagation();
            if step != 0 {
                let target = (current as isize + step).clamp(0, ids.len() as isize - 1) as usize;
                if target != current {
                    self.activate(ids[target], window, cx);
                } else if self
                    .carousel_overscroll
                    .handle_delta(travel, event.touch_phase, cx)
                {
                    cx.notify();
                }
            }
            return;
        }

        if !self.wheel_active {
            self.wheel_active = true;
            self.pixel_gesture_travel = 0.0;
            self.pixel_gesture_navigated = false;
        }

        if !self.pixel_gesture_navigated {
            let (accumulated, step) =
                pixel_gesture_step(self.pixel_gesture_travel, f32::from(travel));
            self.pixel_gesture_travel = accumulated;
            if step != 0 {
                self.pixel_gesture_navigated = true;
                let target = (current as isize + step).clamp(0, ids.len() as isize - 1) as usize;
                if target != current {
                    self.activate(ids[target], window, cx);
                    self.wheel_active = true;
                    self.wheel_axis = Some(axis);
                    self.pixel_gesture_navigated = true;
                    self.pixel_gesture_travel = accumulated;
                } else {
                    self.carousel_overscroll
                        .handle_delta(travel, event.touch_phase, cx);
                }
            }
        }

        if event.touch_phase == TouchPhase::Ended {
            self.wheel_phase_ended = true;
        }
        self.wheel_generation = self.wheel_generation.wrapping_add(1);
        let generation = self.wheel_generation;
        let delay = if self.wheel_phase_ended {
            PIXEL_GESTURE_END_DELAY
        } else {
            PIXEL_GESTURE_IDLE_DELAY
        };
        cx.spawn(async move |view, cx: &mut AsyncApp| {
            cx.background_executor().timer(delay).await;
            let _ = view.update(cx, |view, cx| {
                if view.wheel_generation == generation && view.wheel_active {
                    view.reset_wheel();
                    cx.notify();
                }
            });
        })
        .detach();

        cx.stop_propagation();
        cx.notify();
    }
}
