use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyElement, App, Axis, Element, ElementId, Entity, GlobalElementId, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, MouseUpEvent, ParentElement as _, Pixels, Point,
    Render, StatefulInteractiveElement, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use crate::components::ext::AxisExt as _;
use crate::components::transition::{self, WindowTransitionExt as _};
use gpui_kit_theme::ActiveTheme as _;

const HOVER_FADE_STRENGTH: f32 = 0.8;
pub(crate) const HANDLE_PADDING: Pixels = px(4.);
pub(crate) const HANDLE_WIDTH: Pixels = px(4.);
pub(crate) const HANDLE_LENGTH: Pixels = px(42.);

type DragHandler<E> = dyn Fn(&Point<Pixels>, &mut Window, &mut App) -> Entity<E> + 'static;

pub(crate) fn resize_handle<T: 'static, E: 'static + Render>(
    id: impl Into<ElementId>,
    axis: Axis,
) -> ResizeHandle<T, E> {
    ResizeHandle::new(id, axis)
}

pub(crate) struct ResizeHandle<T: 'static, E: 'static + Render> {
    id: ElementId,
    axis: Axis,
    offset: Option<Pixels>,
    drag_value: Option<Rc<T>>,
    on_drag: Option<Rc<DragHandler<E>>>,
}

impl<T: 'static, E: 'static + Render> ResizeHandle<T, E> {
    fn new(id: impl Into<ElementId>, axis: Axis) -> Self {
        let id = id.into();
        Self {
            id,
            axis,
            offset: None,
            on_drag: None,
            drag_value: None,
        }
    }

    pub(crate) fn left(mut self, offset: Pixels) -> Self {
        self.offset = Some(offset);
        self
    }

    pub(crate) fn on_drag(
        mut self,
        value: T,
        f: impl Fn(Rc<T>, &Point<Pixels>, &mut Window, &mut App) -> Entity<E> + 'static,
    ) -> Self {
        let value = Rc::new(value);
        self.drag_value = Some(value.clone());
        self.on_drag = Some(Rc::new(move |p, window, cx| {
            f(value.clone(), p, window, cx)
        }));
        self
    }
}

#[derive(Default, Debug, Clone)]
struct ResizeHandleState {
    active: Cell<bool>,
}

impl ResizeHandleState {
    fn set_active(&self, active: bool) {
        self.active.set(active);
    }

    fn is_active(&self) -> bool {
        self.active.get()
    }
}

impl<T: 'static, E: 'static + Render> IntoElement for ResizeHandle<T, E> {
    type Element = ResizeHandle<T, E>;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl<T: 'static, E: 'static + Render> Element for ResizeHandle<T, E> {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        let axis = self.axis;

        window.with_element_state(id.unwrap(), |state, window| {
            let state = state.unwrap_or(ResizeHandleState::default());

            let hover_transition =
                window.keyed_transition("hover", cx, transition::QUICK, || 0.0_f32);

            let hover = hover_transition.animate(window, cx);
            let bg_color = cx.theme().colors.text.alpha(hover * HOVER_FADE_STRENGTH);

            let mut el = div()
                .id(self.id.clone())
                .occlude()
                .absolute()
                .flex_shrink_0()
                .on_hover(move |is_hovered, _window, cx| {
                    hover_transition.set(*is_hovered as u8 as f32, cx);
                })
                .on_mouse_down(MouseButton::Left, |_, _window, _cx| {})
                .when_some(self.on_drag.clone(), |this, on_drag| {
                    this.on_drag(
                        self.drag_value.clone().unwrap(),
                        move |_, position, window, cx| on_drag(&position, window, cx),
                    )
                })
                .when(axis.is_horizontal(), |this| {
                    this.cursor_col_resize()
                        .top_0()
                        .h_full()
                        .map(|d| match self.offset {
                            Some(offset) => d
                                .absolute()
                                .left(offset - HANDLE_PADDING)
                                .w(HANDLE_PADDING * 2.),
                            None => d.w(HANDLE_WIDTH).px(HANDLE_PADDING).right_0(),
                        })
                })
                .when(axis.is_vertical(), |this| {
                    this.cursor_row_resize()
                        .top(-HANDLE_PADDING)
                        .left_0()
                        .w_full()
                        .h(HANDLE_WIDTH)
                        .py(HANDLE_PADDING)
                })
                .child(
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .absolute()
                                .rounded_xs()
                                .bg(bg_color)
                                .when(axis.is_horizontal(), |this| {
                                    this.h(HANDLE_LENGTH).w(HANDLE_WIDTH)
                                })
                                .when(axis.is_vertical(), |this| {
                                    this.w(HANDLE_LENGTH).h(HANDLE_WIDTH)
                                }),
                        ),
                )
                .into_any_element();

            let layout_id = el.request_layout(window, cx);

            ((layout_id, el), state)
        })
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: gpui::Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        request_layout.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        request_layout.paint(window, cx);

        window.with_element_state(id.unwrap(), |state: Option<ResizeHandleState>, window| {
            let state = state.unwrap_or_default();

            window.on_mouse_event({
                let state = state.clone();
                move |ev: &MouseDownEvent, phase, window, _| {
                    if bounds.contains(&ev.position) && phase.bubble() {
                        state.set_active(true);
                        window.refresh();
                    }
                }
            });

            window.on_mouse_event({
                let state = state.clone();
                move |_: &MouseUpEvent, _, window, _| {
                    if state.is_active() {
                        state.set_active(false);
                        window.refresh();
                    }
                }
            });

            ((), state)
        });
    }
}
