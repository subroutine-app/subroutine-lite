use std::time::Duration;

use gpui::{
    App, AppContext, Bounds, Context, Pixels, Task, Window, WindowBounds, WindowOptions, point, px,
    size,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::settings::Settings;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum WindowMode {
    Windowed,
    Maximized,
    Fullscreen,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct WindowState {
    pub bounds: Bounds<Pixels>,
    pub display_uuid: Option<Uuid>,
    pub mode: WindowMode,
}

const SAVE_DELAY: Duration = Duration::from_millis(500);
const DEFAULT_WIDTH: f32 = 1200.0;
const DEFAULT_HEIGHT: f32 = 800.0;
const MIN_WIDTH: f32 = 400.0;
const MIN_HEIGHT: f32 = 250.0;

pub(crate) fn restore(options: &mut WindowOptions, cx: &App) -> WindowState {
    let saved = Settings::global(cx)
        .window_state
        .filter(|state| valid_bounds(state.bounds));
    let displays = cx.displays();
    let previous_display = saved
        .and_then(|state| state.display_uuid)
        .filter(|uuid| !uuid.is_nil())
        .and_then(|uuid| {
            displays
                .iter()
                .find(|display| display.uuid().ok() == Some(uuid))
                .cloned()
        });
    let display = previous_display
        .clone()
        .or_else(|| cx.primary_display())
        .or_else(|| displays.first().cloned());
    let available = display
        .as_ref()
        .map(|display| display.visible_bounds())
        .filter(|bounds| valid_bounds(*bounds))
        .unwrap_or_else(default_bounds);
    let state = restored_state(
        saved,
        available,
        previous_display.is_some(),
        display.as_ref().and_then(|display| display.uuid().ok()),
    );
    options.display_id = display.map(|display| display.id());
    prepare_to_open(options, state, available)
}

fn prepare_to_open(
    options: &mut WindowOptions,
    state: WindowState,
    available: Bounds<Pixels>,
) -> WindowState {
    #[cfg(target_os = "macos")]
    let state = if state.mode == WindowMode::Maximized {
        WindowState {
            bounds: available,
            mode: WindowMode::Windowed,
            ..state
        }
    } else {
        state
    };

    options.window_bounds = Some(state.window_bounds());
    options.focus = false;
    options.show = false;
    options.window_min_size = Some(size(
        px(MIN_WIDTH.min(available.size.width.into())),
        px(MIN_HEIGHT.min(available.size.height.into())),
    ));
    state
}

fn default_bounds() -> Bounds<Pixels> {
    Bounds::new(
        point(px(0.0), px(0.0)),
        size(px(DEFAULT_WIDTH), px(DEFAULT_HEIGHT)),
    )
}

fn valid_bounds(bounds: Bounds<Pixels>) -> bool {
    [
        bounds.origin.x,
        bounds.origin.y,
        bounds.size.width,
        bounds.size.height,
    ]
    .into_iter()
    .all(|value| f32::from(value).is_finite())
        && bounds.size.width > px(0.0)
        && bounds.size.height > px(0.0)
}

fn restored_state(
    saved: Option<WindowState>,
    available: Bounds<Pixels>,
    same_display: bool,
    display_uuid: Option<Uuid>,
) -> WindowState {
    let saved = saved.filter(|state| valid_bounds(state.bounds));
    let bounds = saved.map_or_else(default_bounds, |state| state.bounds);
    let width = f32::from(bounds.size.width)
        .max(MIN_WIDTH)
        .min(available.size.width.into());
    let height = f32::from(bounds.size.height)
        .max(MIN_HEIGHT)
        .min(available.size.height.into());
    let left = f32::from(available.origin.x);
    let top = f32::from(available.origin.y);
    let right = left + f32::from(available.size.width) - width;
    let bottom = top + f32::from(available.size.height) - height;
    let origin = if saved.is_some() && same_display {
        point(
            px(f32::from(bounds.origin.x).clamp(left, right)),
            px(f32::from(bounds.origin.y).clamp(top, bottom)),
        )
    } else {
        point(px((left + right) / 2.0), px((top + bottom) / 2.0))
    };
    WindowState {
        bounds: Bounds::new(origin, size(px(width), px(height))),
        mode: saved.map_or(WindowMode::Windowed, |state| state.mode),
        display_uuid,
    }
}

impl WindowState {
    fn window_bounds(self) -> WindowBounds {
        match self.mode {
            WindowMode::Windowed => WindowBounds::Windowed(self.bounds),
            WindowMode::Maximized => WindowBounds::Maximized(self.bounds),
            WindowMode::Fullscreen => WindowBounds::Fullscreen(self.bounds),
        }
    }

    fn record(
        &mut self,
        bounds: WindowBounds,
        mode: WindowMode,
        display_uuid: Option<Uuid>,
        fullscreen_restore_is_normal: bool,
    ) {
        if !valid_bounds(bounds.get_bounds()) {
            return;
        }
        if mode == WindowMode::Windowed {
            self.bounds = bounds.get_bounds();
        } else if cfg!(target_os = "windows")
            && mode == WindowMode::Maximized
            && let WindowBounds::Maximized(restore_bounds) = bounds
        {
            self.bounds = restore_bounds;
        } else if mode == WindowMode::Fullscreen
            && fullscreen_restore_is_normal
            && let WindowBounds::Fullscreen(restore_bounds) = bounds
        {
            self.bounds = restore_bounds;
        }
        self.mode = mode;
        self.display_uuid = display_uuid;
    }
}

struct WindowObservation {
    bounds: WindowBounds,
    mode: WindowMode,
    display_uuid: Option<Uuid>,
}

struct WindowStateTracker {
    current: WindowState,
    pending_observation: Option<WindowObservation>,
    saw_maximized: bool,
    pending_save: Option<Task<()>>,
}

fn observed_mode(bounds: WindowBounds, fullscreen: bool) -> WindowMode {
    if fullscreen {
        return WindowMode::Fullscreen;
    }
    match bounds {
        WindowBounds::Windowed(_) => WindowMode::Windowed,
        WindowBounds::Maximized(_) => WindowMode::Maximized,
        WindowBounds::Fullscreen(_) => WindowMode::Fullscreen,
    }
}

impl WindowStateTracker {
    fn new(initial: WindowState) -> Self {
        Self {
            current: initial,
            pending_observation: None,
            saw_maximized: initial.mode == WindowMode::Maximized,
            pending_save: None,
        }
    }

    fn observe(&mut self, observation: WindowObservation, active: bool) {
        if active && valid_bounds(observation.bounds.get_bounds()) {
            self.saw_maximized |= observation.mode == WindowMode::Maximized;
            self.pending_observation = Some(observation);
        }
    }

    fn settle(&mut self) {
        if let Some(observation) = self.pending_observation.take() {
            self.current.record(
                observation.bounds,
                observation.mode,
                observation.display_uuid,
                !self.saw_maximized,
            );
            if observation.mode == WindowMode::Windowed {
                self.saw_maximized = false;
            }
        }
    }

    fn capture(&mut self, window: &Window, cx: &App) {
        let Some(display) = window.display(cx) else {
            return;
        };
        let bounds = window.inner_window_bounds();
        self.observe(
            WindowObservation {
                bounds,
                mode: observed_mode(bounds, window.is_fullscreen()),
                display_uuid: display.uuid().ok(),
            },
            window.is_window_active(),
        );
    }

    fn changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.capture(window, cx);
        self.pending_save = Some(cx.spawn_in(window, async move |tracker, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            let _ = tracker.update_in(cx, |tracker, window, cx| {
                tracker.capture(window, cx);
                tracker.save(cx, false);
            });
        }));
    }

    fn save(&mut self, cx: &mut App, retry: bool) {
        self.settle();
        let settings = Settings::global(cx);
        if settings.window_state != Some(self.current) {
            Settings::update(cx, |settings| settings.window_state = Some(self.current));
        } else if retry && settings.persistence_issue().is_some() {
            Settings::retry_persistence(cx);
        }
    }

    fn flush(&mut self, window: &Window, cx: &mut App) {
        self.pending_save = None;
        self.capture(window, cx);
        self.save(cx, true);
    }
}

pub(crate) fn install(window: &mut Window, initial: WindowState, cx: &mut App) {
    let tracker = cx.new(|cx| {
        cx.observe_window_bounds(window, WindowStateTracker::changed)
            .detach();
        WindowStateTracker::new(initial)
    });
    let weak_tracker = tracker.downgrade();
    window.on_window_should_close(cx, move |window, cx| {
        tracker.update(cx, |tracker, cx| tracker.flush(window, cx));
        true
    });
    let handle = window.window_handle();
    cx.on_app_quit(move |cx| {
        let _ = handle.update(cx, |_, window, cx| {
            let _ = weak_tracker.update(cx, |tracker, cx| tracker.flush(window, cx));
        });
        async {}
    })
    .detach();
}
