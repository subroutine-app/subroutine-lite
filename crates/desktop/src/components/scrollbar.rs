use gpui::{
    Along, App, Axis, InteractiveElement, IntoElement, MouseDownEvent, MouseMoveEvent,
    ParentElement, Pixels, Point, RenderOnce, ScrollHandle, Styled, Window, div,
    prelude::FluentBuilder, px,
};
use gpui_kit_theme::ActiveTheme;

const TRACK: Pixels = px(10.);
const THUMB: Pixels = px(6.);
const MIN_THUMB: Pixels = px(24.);

#[derive(IntoElement)]
pub struct Scrollbar {
    id: gpui::ElementId,
    handle: ScrollHandle,
    axis: Axis,
}

impl Scrollbar {
    pub fn new(id: impl Into<gpui::ElementId>, handle: &ScrollHandle, axis: Axis) -> Self {
        Self {
            id: id.into(),
            handle: handle.clone(),
            axis,
        }
    }
}

fn thumb_placement(viewport: Pixels, content: Pixels, scrolled: Pixels) -> Option<(f32, f32)> {
    if content <= viewport || viewport <= px(0.) {
        return None;
    }
    let visible = f32::from(viewport) / f32::from(content);
    let travel = (f32::from(content) - f32::from(viewport)).max(1.0);
    let progress = (f32::from(scrolled) / travel).clamp(0.0, 1.0);
    Some((visible.clamp(0.0, 1.0), progress))
}

impl RenderOnce for Scrollbar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let axis = self.axis;
        let viewport = self.handle.bounds().size.along(axis);
        let content = viewport + self.handle.max_offset().along(axis);
        let scrolled = -self.handle.offset().along(axis);

        let Some((visible, progress)) = thumb_placement(viewport, content, scrolled) else {
            return div().id(self.id).absolute().size_0();
        };

        let thumb_length = (viewport * visible).max(MIN_THUMB).min(viewport);
        let start = (viewport - thumb_length) * progress;

        let track = theme.colors.hairline;
        let thumb = theme.colors.hairline_strong;

        let drag_handle = self.handle.clone();
        let wheel_handle = self.handle.clone();

        div()
            .id(self.id)
            .absolute()
            .right_0()
            .bottom_0()
            .when(axis == Axis::Vertical, |this| this.top_0().w(TRACK))
            .when(axis == Axis::Horizontal, |this| this.left_0().h(TRACK))
            .bg(track.opacity(0.0))
            .child(
                div()
                    .absolute()
                    .when(axis == Axis::Vertical, |this| {
                        this.top(start)
                            .left((TRACK - THUMB) / 2.)
                            .w(THUMB)
                            .h(thumb_length)
                    })
                    .when(axis == Axis::Horizontal, |this| {
                        this.left(start)
                            .top((TRACK - THUMB) / 2.)
                            .h(THUMB)
                            .w(thumb_length)
                    })
                    .rounded(THUMB / 2.)
                    .bg(thumb.opacity(0.5))
                    .hover(|this| this.bg(thumb)),
            )
            .on_mouse_down(
                gpui::MouseButton::Left,
                move |event: &MouseDownEvent, window, cx| {
                    scroll_to_pointer(&drag_handle, axis, event.position, viewport, content);
                    window.refresh();
                    cx.stop_propagation();
                },
            )
            .on_mouse_move(move |event: &MouseMoveEvent, window, cx| {
                if event.pressed_button == Some(gpui::MouseButton::Left) {
                    scroll_to_pointer(&wheel_handle, axis, event.position, viewport, content);
                    window.refresh();
                    cx.stop_propagation();
                }
            })
    }
}

fn scroll_to_pointer(
    handle: &ScrollHandle,
    axis: Axis,
    position: Point<Pixels>,
    viewport: Pixels,
    content: Pixels,
) {
    let local = (position - handle.bounds().origin).along(axis);
    let progress = (f32::from(local) / f32::from(viewport).max(1.0)).clamp(0.0, 1.0);
    let travel = content - viewport;
    handle.set_offset(handle.offset().apply_along(axis, |_| -(travel * progress)));
}
