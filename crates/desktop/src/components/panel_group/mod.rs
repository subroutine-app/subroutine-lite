use crate::components::ext::ElementExt;
use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, Element, ElementId, Empty, Entity,
    EventEmitter, IntoElement, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Render,
    RenderOnce, Style, StyleRefinement, Styled, Window, div, prelude::FluentBuilder, px,
};
use gpui_kit::foundation::StyledExt;

mod center_panel;

mod resize_handle;
mod side_panel;
use crate::components::ext::StyledRefineExt as _;
use crate::components::transition::{self, WindowTransitionExt as _};
pub use center_panel::*;

use resize_handle::*;
pub use side_panel::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelGroupResized {
    pub left_proportion: Option<f32>,
}

#[derive(Clone, Default)]
pub struct PanelGroupState {
    pub center_panel: CenterPanelState,
    pub left_panel: Option<SidePanelState>,
    resizing: bool,
    bounds: Bounds<Pixels>,
    prev_container_width: Pixels,
    pub animated_left_px: Pixels,
    left_content_px: Pixels,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PanelWidths {
    left_content: Pixels,
    left_target: Pixels,
}

fn layout_needs_snap(previous_width: Pixels, width: Pixels) -> bool {
    previous_width <= px(0.) || width != previous_width
}

fn preferred_width(panel: &SidePanelState, container_width: Pixels, fallback: f32) -> Pixels {
    let proportion = if panel.opened_proportion.is_finite() {
        panel.opened_proportion
    } else {
        fallback
    };
    let width = if container_width > px(0.) {
        container_width * proportion
    } else {
        px(proportion * 200.)
    };
    width
        .max(panel.width_range.start)
        .min(panel.width_range.end)
}

impl EventEmitter<PanelGroupResized> for PanelGroupState {}

impl PanelGroupState {
    pub fn prepare_layout(&mut self, bounds: Bounds<Pixels>) {
        if self.bounds.size.width != bounds.size.width {
            self.resizing = false;
        }
        self.bounds = bounds;
        if bounds.size.width > px(0.)
            && layout_needs_snap(self.prev_container_width, bounds.size.width)
        {
            let widths = self.widths(0.25);
            self.animated_left_px = widths.left_target;
            self.left_content_px = widths.left_content;
        }
    }

    pub fn invalidate_layout(&mut self) {
        self.prev_container_width = px(0.);
        self.resizing = false;
    }

    pub fn is_compact(&self) -> bool {
        let sidebar_minimum = self
            .left_panel
            .as_ref()
            .map(|panel| panel.width_range.start)
            .unwrap_or(px(0.));
        self.bounds.size.width < self.center_panel.min_width + sidebar_minimum
    }

    fn widths(&self, left_fallback: f32) -> PanelWidths {
        let container_width = self.bounds.size.width;
        let preferred_left = self
            .left_panel
            .as_ref()
            .map(|panel| preferred_width(panel, container_width, left_fallback))
            .unwrap_or(px(0.));
        let docked = !self.is_compact();
        let left_open = docked && self.left_panel.as_ref().is_some_and(|panel| panel.open);
        let mut left_target = if left_open { preferred_left } else { px(0.) };
        let available = (container_width - self.center_panel.min_width).max(px(0.));
        let overflow = (left_target - available).max(px(0.));
        if overflow > px(0.) {
            let left_min = self
                .left_panel
                .as_ref()
                .filter(|panel| panel.open)
                .map(|panel| panel.width_range.start)
                .unwrap_or(px(0.));
            left_target -= overflow.min((left_target - left_min).max(px(0.)));
        }
        PanelWidths {
            left_target,
            left_content: if left_open {
                left_target
            } else if self.left_content_px > px(0.) {
                self.left_content_px
            } else {
                preferred_left
            },
        }
    }
    pub fn toggle_left(&mut self) {
        if let Some(panel) = self.left_panel.as_mut() {
            panel.open = !panel.open;
        }
    }

    fn resize_left(&mut self, mouse_x: Pixels) {
        let container_width = self.bounds.size.width;
        if container_width <= px(0.) || self.is_compact() {
            return;
        }
        let center_min_width = self.center_panel.min_width;
        let new_width = mouse_x - self.bounds.left();
        let panel = match self.left_panel.as_mut() {
            Some(panel) => panel,
            None => return,
        };
        let max_width = (container_width - center_min_width)
            .max(panel.width_range.start)
            .min(panel.width_range.end);
        let clamped = new_width.max(panel.width_range.start).min(max_width);
        panel.opened_proportion = clamped / container_width;
    }
}

type ContentOverlay = Box<dyn FnOnce(Pixels, Pixels, &mut Window, &mut App) -> AnyElement>;

#[derive(IntoElement)]
pub struct PanelGroup {
    state: Entity<PanelGroupState>,
    style: StyleRefinement,
    center: CenterPanel,
    left: Option<SidePanel>,
    content_wrapper: Option<Box<dyn FnOnce(AnyElement) -> AnyElement>>,
    content_overlay: Option<ContentOverlay>,
}

impl PanelGroup {
    pub fn new(state: Entity<PanelGroupState>) -> Self {
        Self {
            state,
            style: StyleRefinement::default(),
            center: CenterPanel::new(),
            left: None,
            content_wrapper: None,
            content_overlay: None,
        }
    }

    pub fn center(mut self, panel: CenterPanel) -> Self {
        self.center = panel;
        self
    }

    pub fn left(mut self, panel: SidePanel) -> Self {
        self.left = Some(panel);
        self
    }

    pub fn content_wrapper(
        mut self,
        wrapper: impl FnOnce(AnyElement) -> AnyElement + 'static,
    ) -> Self {
        self.content_wrapper = Some(Box::new(wrapper));
        self
    }

    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    pub fn content_overlay(
        mut self,
        overlay: impl FnOnce(Pixels, Pixels, &mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        self.content_overlay = Some(Box::new(overlay));
        self
    }
}

impl Styled for PanelGroup {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for PanelGroup {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = self.state.clone();
        let weak_state = state.downgrade();
        let left_initial_proportion = self.left.as_ref().map(|panel| panel.initial_proportion);

        let group_state = state.read(cx).clone();
        let container_width = group_state.bounds.size.width;

        let widths = group_state.widths(left_initial_proportion.unwrap_or(0.25));

        let left_open_px = widths.left_content;
        let left_target_px = widths.left_target;

        let snap_layout = layout_needs_snap(group_state.prev_container_width, container_width);

        let left_px = if self.left.is_some() {
            let left_target = left_target_px.as_f32();
            let transition =
                window.keyed_transition("left-panel-slide", cx, transition::QUICK, move || {
                    left_target
                });
            if group_state.resizing || snap_layout {
                transition.snap(left_target, cx);
            }
            transition.set(left_target, cx);
            px(transition.animate(window, cx))
        } else {
            px(0.)
        };

        let changed = state.update(cx, |s, _| {
            let changed = s.animated_left_px != left_px;
            s.animated_left_px = left_px;
            s.left_content_px = left_open_px;
            s.prev_container_width = container_width;
            changed
        });

        if changed {
            window.request_animation_frame();
        }
        let left_visible = left_px > px(0.);

        let left_handle = if self.left.is_some() && left_visible {
            let weak = weak_state.clone();
            Some(
                resize_handle("left-panel-handle", gpui::Axis::Horizontal)
                    .left(left_px)
                    .on_drag((), move |_, _, _, cx| {
                        cx.stop_propagation();
                        weak.update(cx, |s, _| {
                            s.resizing = true;
                        })
                        .ok();
                        cx.new(|_| DragHandle)
                    }),
            )
        } else {
            None
        };

        let PanelGroup {
            style,
            center,
            left,
            content_wrapper,
            content_overlay,
            ..
        } = self;
        let has_left = left.is_some();

        let content = div()
            .row()
            .size_full()
            .when(has_left, |this| {
                this.child(div().h_full().w(left_px).flex_shrink_0())
            })
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .min_w_0()
                    .overflow_hidden()
                    .child(center),
            )
            .into_any_element();
        let content = match content_wrapper {
            Some(wrapper) => wrapper(content),
            None => content,
        };
        let content_overlay = content_overlay.map(|overlay| overlay(left_px, px(0.), window, cx));

        div()
            .relative()
            .refine_style(&style)
            .on_prepaint({
                let state = state.clone();
                move |bounds, window, cx| {
                    if state.read(cx).bounds != bounds {
                        state.update(cx, |s, _| s.prepare_layout(bounds));
                        window.request_animation_frame();
                    }
                }
            })
            .child(content)
            .children(content_overlay)
            .when_some(left.filter(|_| left_visible), |this, left| {
                let inner = left
                    .base
                    .absolute()
                    .right_0()
                    .top_0()
                    .h_full()
                    .w(left_open_px)
                    .into_any_element();

                this.child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .h_full()
                        .overflow_hidden()
                        .w(left_px)
                        .child(inner),
                )
            })
            .when_some(left_handle, |this, handle| this.child(handle))
            .child(PanelGroupDragElement {
                state: state.clone(),
            })
    }
}

struct DragHandle;
impl Render for DragHandle {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

struct PanelGroupDragElement {
    state: Entity<PanelGroupState>,
}

impl IntoElement for PanelGroupDragElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for PanelGroupDragElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui::LayoutId, ()) {
        (window.request_layout(Style::default(), None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        window.on_mouse_event({
            let state = self.state.clone();
            let resizing = state.read(cx).resizing;
            move |e: &MouseMoveEvent, phase, _window, cx| {
                if !phase.bubble() || !resizing {
                    return;
                }
                state.update(cx, |s, cx| {
                    s.resize_left(e.position.x);
                    cx.notify();
                });
            }
        });

        window.on_mouse_event({
            let state = self.state.clone();
            let resizing = state.read(cx).resizing;
            move |_: &MouseUpEvent, phase, _window, cx| {
                if !resizing {
                    return;
                }
                if phase.bubble() {
                    state.update(cx, |s, cx| {
                        s.resizing = false;
                        cx.emit(PanelGroupResized {
                            left_proportion: s
                                .left_panel
                                .as_ref()
                                .map(|panel| panel.opened_proportion),
                        });
                        cx.notify();
                    });
                }
            }
        });
    }
}
