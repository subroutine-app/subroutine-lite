#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
pub(crate) use windows::{WindowMaterial, init};

#[cfg(not(target_os = "windows"))]
pub(crate) fn init(_: &mut gpui::App) {}

#[cfg(not(target_os = "windows"))]
#[derive(Default)]
pub(crate) struct WindowMaterial;

#[cfg(not(target_os = "windows"))]
impl WindowMaterial {
    pub(crate) fn background(&mut self, _: &mut gpui::Window, cx: &gpui::App) -> gpui::Hsla {
        use gpui_kit_theme::ActiveTheme as _;

        // Keep the content layer opaque; AppKit owns the unified titlebar and toolbar.
        cx.theme().colors.canvas.alpha(1.0)
    }
}
