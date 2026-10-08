use crate::components::transition::{self, WindowTransitionExt as _};
use gpui::{
    App, ElementId, InteractiveElement, IntoElement, KeyBinding, Length, MouseButton,
    ParentElement, Styled, Window, actions, deferred, div, ease_in_out, prelude::FluentBuilder,
};
use gpui_kit::foundation::StyledExt as _;

actions!(overlay, [CloseOverlay]);

pub fn init(cx: &mut App) {
    let context: Option<&str> = Some("Overlay");
    cx.bind_keys([KeyBinding::new("escape", CloseOverlay, context)]);
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OverlayPosition {
    Top(Length),
    Center,
}

pub fn overlay<T: IntoElement + Styled>(
    id: impl Into<ElementId>,
    inner: T,
    position: OverlayPosition,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    overlay_with_scrim(id, inner, position, 0.42, window, cx)
}

pub fn overlay_with_scrim<T: IntoElement + Styled>(
    id: impl Into<ElementId>,
    inner: T,
    position: OverlayPosition,
    scrim_opacity: f32,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let id = id.into();
    let transition =
        window.keyed_transition((id.clone(), "transition"), cx, transition::QUICK, || 0.0);
    transition.set(1.0, cx);

    let t = transition.animate(window, cx);

    let bg_color = gpui::black().opacity(scrim_opacity.clamp(0.0, 1.0) * t);

    let spacer_height = match position {
        OverlayPosition::Top(top) => Some(top),
        OverlayPosition::Center => None,
    };

    deferred(
        div()
            .column()
            .bg(bg_color)
            .absolute()
            .inset_0()
            .size_full()
            .occlude()
            .key_context("Overlay")
            .on_mouse_down(MouseButton::Left, |_event, window, cx| {
                window.dispatch_action(Box::new(CloseOverlay), cx);
            })
            .items_center()
            .when(position == OverlayPosition::Center, |this| {
                this.justify_center()
            })
            .when_some(spacer_height, |this, top| this.child(div().w_full().h(top)))
            .child(
                div()
                    .id("overlay-surface")
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(inner.opacity(ease_in_out(t))),
            ),
    )
}
