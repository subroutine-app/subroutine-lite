use crate::components::ext::StyledRefineExt as _;
use gpui::Edges;
use gpui::{App, Corners, ParentElement, Pixels, StyleRefinement, Styled, Window, div, px};
use gpui_kit_theme::ActiveTheme;

mod button;
mod checkbox;
mod divider;
mod drag_drop;
pub mod dynamic_list;
pub(crate) mod elastic_overscroll;
mod empty_state;
pub mod ext;
mod item_card;
mod label;
mod marquee;
pub mod menu;
mod month_selector;
mod overlay;
pub mod scrollbar;
mod sidebar_panel;
pub mod text_input;
mod toast;
pub mod transition;
pub mod virtual_list;

pub mod panel_group;
pub use button::*;
pub use checkbox::*;
pub use divider::*;
pub use drag_drop::*;
pub use dynamic_list::*;
pub use empty_state::*;
pub use item_card::*;
pub use label::*;
pub use marquee::*;
pub use month_selector::*;
pub use overlay::*;
pub use sidebar_panel::*;
pub(crate) use toast::timed_toast;

pub fn init(cx: &mut App) {
    item_card::init(cx);
    overlay::init(cx);
}

trait FocusableExt<T: ParentElement + Styled + Sized> {
    fn focus_ring(self, is_focused: bool, margins: Pixels, window: &Window, cx: &App) -> Self;
}

impl<T: ParentElement + Styled + Sized> FocusableExt<T> for T {
    fn focus_ring(mut self, is_focused: bool, margins: Pixels, window: &Window, cx: &App) -> Self {
        if !is_focused {
            return self;
        }

        const RING_BORDER_WIDTH: Pixels = px(1.5);
        let rem_size = window.rem_size();
        let style = self.style();

        let border_widths = Edges::<Pixels> {
            top: style
                .border_widths
                .top
                .map(|v| v.to_pixels(rem_size))
                .unwrap_or_default(),
            bottom: style
                .border_widths
                .bottom
                .map(|v| v.to_pixels(rem_size))
                .unwrap_or_default(),
            left: style
                .border_widths
                .left
                .map(|v| v.to_pixels(rem_size))
                .unwrap_or_default(),
            right: style
                .border_widths
                .right
                .map(|v| v.to_pixels(rem_size))
                .unwrap_or_default(),
        };

        let radius = Corners::<Pixels> {
            top_left: style
                .corner_radii
                .top_left
                .map(|v| v.to_pixels(rem_size))
                .unwrap_or_default(),
            top_right: style
                .corner_radii
                .top_right
                .map(|v| v.to_pixels(rem_size))
                .unwrap_or_default(),
            bottom_left: style
                .corner_radii
                .bottom_left
                .map(|v| v.to_pixels(rem_size))
                .unwrap_or_default(),
            bottom_right: style
                .corner_radii
                .bottom_right
                .map(|v| v.to_pixels(rem_size))
                .unwrap_or_default(),
        }
        .map(|v| *v + RING_BORDER_WIDTH);

        let mut inner_style = StyleRefinement::default();
        inner_style.corner_radii.top_left = Some(radius.top_left.into());
        inner_style.corner_radii.top_right = Some(radius.top_right.into());
        inner_style.corner_radii.bottom_left = Some(radius.bottom_left.into());
        inner_style.corner_radii.bottom_right = Some(radius.bottom_right.into());

        let inset = RING_BORDER_WIDTH + margins;

        self.child(
            div()
                .flex_none()
                .absolute()
                .top(-(inset + border_widths.top))
                .left(-(inset + border_widths.left))
                .right(-(inset + border_widths.right))
                .bottom(-(inset + border_widths.bottom))
                .border(RING_BORDER_WIDTH)
                .border_color(cx.theme().colors.focus.alpha(0.2))
                .refine_style(&inner_style),
        )
    }
}
