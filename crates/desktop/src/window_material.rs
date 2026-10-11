use gpui::{App, Hsla, Window, WindowBackgroundAppearance};
use gpui_kit::motion::MotionSpec;
use gpui_kit_theme::{ActiveTheme as _, Theme};

use crate::components::transition::{EASE_OUT_CUBIC, WindowTransitionExt as _};

const FOCUS_WASH: MotionSpec = MotionSpec::new(200, EASE_OUT_CUBIC);
const INACTIVE_WASH_FACTOR: f32 = 0.6;
const WASH_BOOST: f32 = 0.08;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
use self::windows as platform;
#[cfg(target_os = "macos")]
use macos as platform;

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn init(cx: &mut App) {
    platform::init(cx);
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn init(_: &mut App) {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WindowMaterial {
    #[default]
    Opaque,
    Frosted,
}

impl WindowMaterial {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(crate) fn for_window(cx: &App) -> Self {
        if crate::settings::Settings::global(cx).frosted_background
            && platform::allows_transparency(cx)
            && !cx.theme().reduce_transparency
        {
            Self::Frosted
        } else {
            Self::Opaque
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    pub(crate) fn for_window(_: &App) -> Self {
        Self::Opaque
    }

    pub(crate) fn animated_tint(self, window: &mut Window, cx: &mut App) -> Hsla {
        let target = self.tint(cx.theme(), window.is_window_active());
        let opacity = window.keyed_transition("window-material-wash", cx, FOCUS_WASH, || target.a);
        match self {
            Self::Opaque => opacity.snap(target.a, cx),
            Self::Frosted => opacity.set(target.a, cx),
        }
        target.alpha(opacity.animate(window, cx))
    }

    fn tint(self, theme: &Theme, active: bool) -> Hsla {
        let canvas = theme.colors.canvas.alpha(1.0);
        if self == Self::Opaque {
            return canvas;
        }

        // Protect muted labels at rest and primary text through hover/selection,
        // without letting stacked interaction tints make the entire window opaque.
        let opacity = (12..=20)
            .map(|step| step as f32 / 20.0)
            .find(|opacity| {
                [gpui::black(), gpui::white()].into_iter().all(|backdrop| {
                    let background = backdrop.blend(canvas.alpha(*opacity));
                    [theme.colors.text, theme.colors.text_muted]
                        .into_iter()
                        .all(|foreground| theme.contrast(foreground, background) >= 4.5)
                        && [background, background.blend(theme.colors.hover)]
                            .into_iter()
                            .flat_map(|background| {
                                [background, background.blend(theme.colors.focus.alpha(0.10))]
                            })
                            .all(|background| theme.contrast(theme.colors.text, background) >= 4.5)
                })
            })
            .unwrap_or(1.0);
        let opacity = if active {
            opacity
        } else {
            opacity * INACTIVE_WASH_FACTOR
        };
        canvas.alpha((opacity + WASH_BOOST).min(1.0))
    }

    pub(crate) fn apply(self, window: &mut Window) {
        window.set_background_appearance(match self {
            Self::Opaque => WindowBackgroundAppearance::Opaque,
            Self::Frosted => WindowBackgroundAppearance::Blurred,
        });
        window.refresh();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::tokens::TokenDocument;
    use gpui_kit_theme::Density;

    #[test]
    fn window_tint_restores_contrast_when_active_and_stays_opaque_without_frost() {
        let themes = [
            include_str!("../../../assets/themes/subroutine-dark.json"),
            include_str!("../../../assets/themes/subroutine-light.json"),
        ]
        .map(|json| {
            let tokens = TokenDocument::parse(json).unwrap();
            Theme::from_tokens(&tokens, Density::Comfortable)
        });
        for active in [true, false] {
            assert_eq!(
                WindowMaterial::Frosted.tint(&themes[0], active).a,
                WindowMaterial::Frosted.tint(&themes[1], active).a,
            );
        }
        for theme in themes {
            let tint = WindowMaterial::Frosted.tint(&theme, true);
            let inactive = WindowMaterial::Frosted.tint(&theme, false);
            assert!(0.0 < inactive.a && inactive.a < tint.a && tint.a < 1.0);
            assert_eq!(WindowMaterial::Opaque.tint(&theme, true).a, 1.0);
            assert_eq!(WindowMaterial::Opaque.tint(&theme, false).a, 1.0);

            for backdrop in [gpui::black(), gpui::white()] {
                let resting = backdrop.blend(tint);
                assert!(theme.contrast(theme.colors.text_muted, resting) >= 4.5);
                let hovered = resting.blend(theme.colors.hover);
                for background in [resting, hovered] {
                    for background in [background, background.blend(theme.colors.focus.alpha(0.10))]
                    {
                        assert!(theme.contrast(theme.colors.text, background) >= 4.5);
                    }
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ptr::NonNull;

    use block2::RcBlock;
    use gpui::{App, Global, Task};
    use objc2::{
        rc::Retained,
        runtime::{NSObjectProtocol, ProtocolObject},
    };
    use objc2_app_kit::{NSWorkspace, NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification};
    use objc2_foundation::{NSNotification, NSNotificationCenter};

    struct SystemTransparency {
        reduced: bool,
        center: Retained<NSNotificationCenter>,
        observer: Retained<ProtocolObject<dyn NSObjectProtocol>>,
        _updates: Task<()>,
    }

    impl Global for SystemTransparency {}

    impl Drop for SystemTransparency {
        fn drop(&mut self) {
            // The token belongs to this workspace notification center.
            unsafe { self.center.removeObserver((*self.observer).as_ref()) };
        }
    }

    pub(super) fn allows_transparency(cx: &App) -> bool {
        cx.try_global::<SystemTransparency>()
            .is_some_and(|state| !state.reduced)
    }

    pub(crate) fn init(cx: &mut App) {
        if cx.has_global::<SystemTransparency>() {
            return;
        }

        let workspace = NSWorkspace::sharedWorkspace();
        let center = workspace.notificationCenter();
        let (changes_tx, changes_rx) = flume::bounded(1);
        let callback = RcBlock::new(move |_: NonNull<NSNotification>| {
            // Coalesce notifications; a disconnected receiver means the app has shut down.
            let _ = changes_tx.try_send(());
        });
        // Only a thread-safe sender crosses the native callback boundary, never GPUI state.
        let observer = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
                Some(workspace.as_ref()),
                None,
                &callback,
            )
        };
        let reduced = workspace.accessibilityDisplayShouldReduceTransparency();
        let updates = cx.spawn(async move |cx| {
            while changes_rx.recv_async().await.is_ok() {
                let reduced = workspace.accessibilityDisplayShouldReduceTransparency();
                cx.update(|cx| {
                    if cx.global::<SystemTransparency>().reduced != reduced {
                        cx.global_mut::<SystemTransparency>().reduced = reduced;
                        cx.refresh_windows();
                    }
                });
            }
        });
        cx.set_global(SystemTransparency {
            reduced,
            center,
            observer,
            _updates: updates,
        });
        cx.refresh_windows();
    }
}
