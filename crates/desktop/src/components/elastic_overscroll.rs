use std::time::{Duration, Instant};

use gpui::{
    App, Pixels, ScrollWheelEvent, SpringConfig, SpringState, TouchPhase, Window, point, px,
};
use gpui_kit::motion::overscroll;
use gpui_kit_theme::{ActiveTheme, Theme};

const BURST_GAP: Duration = Duration::from_millis(120);
const IMPACT_WINDOW: Duration = Duration::from_millis(48);
const IMPULSE_SCALE: f32 = 70.0;
const MAX_VELOCITY: f32 = 1_200.0;
const EPSILON: f32 = 0.1;

fn spring(theme: &Theme) -> SpringConfig {
    let tokens = theme.motion.snappy;
    SpringConfig::new(tokens.stiffness, tokens.damping, tokens.mass)
}

#[derive(Clone, Copy, Debug)]
struct Burst {
    started: Instant,
    last_input: Instant,
    direction: f32,
}

fn accepts_impulse(
    burst: &mut Option<Burst>,
    now: Instant,
    direction: f32,
    force_new: bool,
) -> bool {
    let starts_new = force_new
        || burst.is_none_or(|burst| {
            direction != burst.direction
                || now.saturating_duration_since(burst.last_input) >= BURST_GAP
        });
    if starts_new {
        *burst = Some(Burst {
            started: now,
            last_input: now,
            direction,
        });
        return true;
    }

    let burst = burst.as_mut().expect("burst was checked above");
    burst.last_input = now;
    now.saturating_duration_since(burst.started) <= IMPACT_WINDOW
}

#[derive(Debug)]
pub(crate) struct ElasticOverscroll {
    spring: SpringState,
    updated_at: Instant,
    burst: Option<Burst>,
    accepting_input: bool,
    input_gate_updated_at: Instant,
    first_paint: bool,
}

impl Default for ElasticOverscroll {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            spring: SpringState::default(),
            updated_at: now,
            burst: None,
            accepting_input: false,
            input_gate_updated_at: now,
            first_paint: true,
        }
    }
}

impl ElasticOverscroll {
    pub(crate) fn handle_delta(
        &mut self,
        delta: Pixels,
        touch_phase: TouchPhase,
        cx: &App,
    ) -> bool {
        if cx.reduce_motion() {
            return false;
        }

        let delta = f32::from(delta);
        if delta == 0.0 {
            if touch_phase == TouchPhase::Cancelled {
                self.burst = None;
                self.accepting_input = false;
                self.input_gate_updated_at = Instant::now();
            }
            return false;
        }

        let now = Instant::now();
        if !self.accepting_input {
            let starts_gesture = touch_phase == TouchPhase::Started;
            let follows_quiet =
                now.saturating_duration_since(self.input_gate_updated_at) >= BURST_GAP;
            if !starts_gesture && !follows_quiet {
                self.input_gate_updated_at = now;
                return false;
            }
            self.accepting_input = true;
        }

        let config = spring(cx.theme());
        let elapsed = now.saturating_duration_since(self.updated_at).as_secs_f32();
        self.spring = config.step(self.spring, 0.0, elapsed);
        self.updated_at = now;

        let accepts_impulse = accepts_impulse(
            &mut self.burst,
            now,
            delta.signum(),
            touch_phase == TouchPhase::Started,
        );
        if accepts_impulse {
            let resisted = f32::from(overscroll(px(delta), cx.theme()));
            self.spring.velocity = (self.spring.velocity + resisted * IMPULSE_SCALE)
                .clamp(-MAX_VELOCITY, MAX_VELOCITY);
        }
        if touch_phase == TouchPhase::Cancelled {
            self.burst = None;
            self.accepting_input = false;
            self.input_gate_updated_at = now;
        }
        true
    }

    pub(crate) fn handle_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let line_height = window.line_height();
        let delta = event.delta.pixel_delta(line_height).y;
        if !self.handle_delta(delta, event.touch_phase, cx) {
            return false;
        }

        window.consume_scroll_delta(point(px(0.0), delta), line_height, cx);
        true
    }

    pub(crate) fn advance(&mut self, cx: &App) -> Pixels {
        if cx.reduce_motion() {
            self.reset();
            return px(0.0);
        }

        let now = Instant::now();
        if self.first_paint {
            self.spring = SpringState::default();
            self.updated_at = now;
            self.first_paint = false;
            return px(0.0);
        }

        let config = spring(cx.theme());
        let elapsed = now.saturating_duration_since(self.updated_at).as_secs_f32();
        self.spring = config.step(self.spring, 0.0, elapsed);
        if config.is_settled(self.spring, 0.0, EPSILON) {
            self.spring = SpringState::default();
        }
        self.updated_at = now;
        px(self.spring.position)
    }

    pub(crate) fn needs_frame(&self, cx: &App) -> bool {
        !spring(cx.theme()).is_settled(self.spring, 0.0, EPSILON)
    }

    pub(crate) fn offset(&self) -> Pixels {
        px(self.spring.position)
    }

    pub(crate) fn reset(&mut self) {
        let now = Instant::now();
        self.spring = SpringState::default();
        self.updated_at = now;
        self.burst = None;
        self.accepting_input = false;
        self.input_gate_updated_at = now;
        self.first_paint = true;
    }
}
