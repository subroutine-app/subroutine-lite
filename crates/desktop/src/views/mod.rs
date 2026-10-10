use crate::color::ColorExt as _;
use gpui::{AnyElement, App, Hsla, IntoElement, Pixels, px};
#[cfg(not(target_os = "macos"))]
use gpui_kit::foundation::ThemeOverlay;
use gpui_kit_theme::{Appearance, Theme};

pub(crate) const LIST_VIEW_MIN_WIDTH: Pixels = px(360.);
pub(crate) const LIST_VIEW_MAX_WIDTH: Pixels = px(760.);

pub(crate) const LIBRARY_HEADER_HEIGHT: Pixels = px(96.);
pub(crate) const SEARCH_HEADER_HEIGHT: Pixels = px(216.);

pub(crate) fn platform_material_scope(child: impl IntoElement) -> AnyElement {
    #[cfg(target_os = "macos")]
    {
        child.into_any_element()
    }

    #[cfg(not(target_os = "macos"))]
    {
        ThemeOverlay::new(|theme| theme.clone().with_reduce_transparency(true), child)
            .into_any_element()
    }
}

pub(crate) fn frosted_overlay_tint(theme: &Theme) -> Hsla {
    let tint = match theme.appearance {
        Appearance::Dark => gpui::black(),
        Appearance::Light => gpui::white(),
    };
    theme.colors.overlay.mix(tint, 0.65)
}

pub(crate) fn fixed_glass_bevel_theme(theme: &Theme, initial_short_edge: f32) -> Theme {
    let upper = theme
        .effects
        .glass_bevel_max
        .max(0.0)
        .min(initial_short_edge * 0.5);
    let lower = theme.effects.glass_bevel_min.max(0.0).min(upper);
    let bevel = (initial_short_edge * theme.effects.glass_bevel_ratio.max(0.0)).clamp(lower, upper);
    theme.clone().modify(|theme| {
        theme.effects.glass_bevel_min = bevel;
        theme.effects.glass_bevel_max = bevel;
    })
}

mod action_library_view;
mod command_palette;
mod drag_navigation;
mod drop_confirmation;
mod home_view;
mod item_creator;
mod item_inspector;
mod library_drop;
mod main_view;
mod root_view;
mod routines_view;
mod saved_items_view;
mod search_view;
mod settings_view;

pub use action_library_view::*;
pub use home_view::*;
pub use item_creator::*;
pub use item_inspector::*;
pub use main_view::*;
pub use root_view::*;
pub use routines_view::*;
pub use saved_items_view::*;
pub use search_view::*;
pub use settings_view::*;

pub fn init(cx: &mut App) {
    root_view::init(cx);
    command_palette::init(cx);
    item_creator::init(cx);
    main_view::init(cx);
    settings_view::init(cx);
}
