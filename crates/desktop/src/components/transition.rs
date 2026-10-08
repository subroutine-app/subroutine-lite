use std::ops::{Add, Mul};

use gpui::{App, ElementId, Entity, Window};
use gpui_kit::motion::{CubicBezier, Interpolate, MotionSpec, Transition};

pub const EASE_OUT_CUBIC: CubicBezier = CubicBezier::new(0.215, 0.61, 0.355, 1.0);

pub const QUICK: MotionSpec = MotionSpec::new(150, EASE_OUT_CUBIC);

pub const SCROLL: MotionSpec = MotionSpec::new(200, EASE_OUT_CUBIC);

#[derive(Clone)]
pub struct KeyedTransition<T: Interpolate + 'static> {
    state: Entity<Transition<T>>,
}

impl<T: Interpolate + PartialEq + 'static> KeyedTransition<T> {
    pub fn set(&self, target: T, cx: &mut App) {
        self.state
            .update(cx, |transition, _| transition.set(target));
    }

    pub fn snap(&self, target: T, cx: &mut App) {
        self.state
            .update(cx, |transition, _| transition.snap(target));
    }

    pub fn value(&self, cx: &App) -> T {
        self.state.read(cx).value()
    }

    pub fn is_animating(&self, cx: &App) -> bool {
        self.state.read(cx).is_animating()
    }

    pub fn animate(&self, window: &mut Window, cx: &mut App) -> T {
        self.state
            .update(cx, |transition, cx| transition.animate(window, cx))
    }
}

impl<T> KeyedTransition<T>
where
    T: Interpolate + PartialEq + Mul<f32, Output = T> + 'static,
{
    pub fn scale_by(&self, ratio: f32, cx: &mut App) {
        self.state
            .update(cx, |transition, _| transition.scale_by(ratio));
    }
}

impl<T> KeyedTransition<T>
where
    T: Interpolate + PartialEq + Add<T, Output = T> + 'static,
{
    pub fn offset_by(&self, delta: T, cx: &mut App) {
        self.state
            .update(cx, |transition, _| transition.offset_by(delta));
    }
}

pub trait WindowTransitionExt {
    fn keyed_transition<T: Interpolate + PartialEq + 'static>(
        &mut self,
        key: impl Into<ElementId>,
        cx: &mut App,
        spec: MotionSpec,
        init: impl FnOnce() -> T,
    ) -> KeyedTransition<T>;
}

impl WindowTransitionExt for Window {
    fn keyed_transition<T: Interpolate + PartialEq + 'static>(
        &mut self,
        key: impl Into<ElementId>,
        cx: &mut App,
        spec: MotionSpec,
        init: impl FnOnce() -> T,
    ) -> KeyedTransition<T> {
        let state = self.use_keyed_state(key, cx, |_, _| Transition::new(init(), spec));
        KeyedTransition { state }
    }
}
