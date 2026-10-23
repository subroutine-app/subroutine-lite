use super::{RootView, TOP_EDGE_INSET, panels::WORKSPACE_PANEL_GAP};
use gpui::{
    App, Bounds, Context, IntoElement, Pixels, Point, Size, Window, canvas, point, px, size,
};
use std::time::Duration;

const REVEAL_EDGE_WIDTH: Pixels = px(12.);
const HOVER_EXIT_GUTTER: Pixels = px(24.);
const HOVER_REVEAL_DELAY: Duration = Duration::from_millis(300);
const HOVER_CLOSE_DELAY: Duration = Duration::from_millis(180);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum NavigationDrawer {
    Closed,
    Compact,
    Hover,
}

fn hover_bounds(
    viewport: Size<Pixels>,
    sidebar_width: Pixels,
    library_width: Option<Pixels>,
) -> Bounds<Pixels> {
    let right = sidebar_width
        + library_width.map_or(px(0.), |width| WORKSPACE_PANEL_GAP + width)
        + HOVER_EXIT_GUTTER;
    Bounds::new(
        Point::default(),
        size(right.min(viewport.width), viewport.height),
    )
}

impl RootView {
    pub(super) fn navigation_drawer_width(&self, window: &Window, cx: &App) -> Pixels {
        let layout = self.layout_state.read(cx);
        let width = layout.left_open_width();
        if layout.is_compact() {
            width.min((window.viewport_size().width - px(48.)).max(px(0.)))
        } else {
            width
        }
    }

    fn navigation_hover_contains_pointer(&self, window: &Window, cx: &App) -> bool {
        let viewport = window.viewport_size();
        let width = self.navigation_drawer_width(window, cx);
        let library_width = self.library_drawer_open.then(|| {
            self.library_drawer_width
                .min((viewport.width - width - WORKSPACE_PANEL_GAP).max(px(0.)))
        });
        self.navigation_pointer
            .is_some_and(|pointer| hover_bounds(viewport, width, library_width).contains(&pointer))
    }

    fn navigation_reveal_edge_contains_pointer(&self, window: &Window, cx: &App) -> bool {
        let viewport = window.viewport_size();
        let edge = Bounds::new(
            point(px(0.), TOP_EDGE_INSET),
            size(
                REVEAL_EDGE_WIDTH,
                (viewport.height - TOP_EDGE_INSET).max(px(0.)),
            ),
        );
        self.layout_state.read(cx).animated_left_px <= px(0.)
            && self
                .navigation_pointer
                .is_some_and(|pointer| edge.contains(&pointer))
    }

    fn navigation_hover_suspended(&self, window: &Window, cx: &App) -> bool {
        self.layout_state.read(cx).is_resizing()
            || self.navigation_panel_open(cx)
            || self.current_overlay.is_some()
            || self.context_menu.entity().read(cx).is_open()
            || !window.is_window_active()
    }

    pub(super) fn observe_navigation_pointer(
        &mut self,
        pointer: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigation_pointer = pointer;
        if self.navigation_hover_suspended(window, cx) {
            self.navigation_hover_task = None;
            return;
        }

        match self.navigation_drawer {
            NavigationDrawer::Closed => {
                if !self.navigation_reveal_edge_contains_pointer(window, cx) {
                    self.navigation_hover_task = None;
                } else if self.navigation_hover_task.is_none() {
                    self.navigation_hover_task =
                        Some(cx.spawn_in(window, async move |view, cx| {
                            cx.background_executor().timer(HOVER_REVEAL_DELAY).await;
                            let _ = view.update_in(cx, |view, window, cx| {
                                view.navigation_hover_task = None;
                                if view.navigation_drawer == NavigationDrawer::Closed
                                    && !view.navigation_hover_suspended(window, cx)
                                    && view.navigation_reveal_edge_contains_pointer(window, cx)
                                {
                                    view.navigation_drawer = NavigationDrawer::Hover;
                                    cx.notify();
                                }
                            });
                        }));
                }
            }
            NavigationDrawer::Hover => {
                if self.navigation_hover_contains_pointer(window, cx) {
                    self.navigation_hover_task = None;
                } else if self.navigation_hover_task.is_none() {
                    self.navigation_hover_task =
                        Some(cx.spawn_in(window, async move |view, cx| {
                            cx.background_executor().timer(HOVER_CLOSE_DELAY).await;
                            let _ = view.update_in(cx, |view, window, cx| {
                                view.navigation_hover_task = None;
                                if view.navigation_drawer == NavigationDrawer::Hover
                                    && !view.layout_state.read(cx).is_resizing()
                                    && view.current_overlay.is_none()
                                    && !view.context_menu.entity().read(cx).is_open()
                                    && !view.navigation_hover_contains_pointer(window, cx)
                                {
                                    view.close_navigation_drawer(window, cx);
                                }
                            });
                        }));
                }
            }
            NavigationDrawer::Compact => {}
        }
    }

    pub(super) fn navigation_hover_observer(&self, cx: &Context<Self>) -> impl IntoElement {
        let root = cx.entity().downgrade();
        canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                // Observe before occluding descendants, without adding a hitbox over the workspace.
                let moved = root.clone();
                window.on_mouse_event(move |event: &gpui::MouseMoveEvent, phase, window, cx| {
                    if phase.capture() {
                        let _ = moved.update(cx, |view, cx| {
                            view.observe_navigation_pointer(Some(event.position), window, cx);
                        });
                    }
                });
                window.on_mouse_event(move |_: &gpui::MouseExitEvent, phase, window, cx| {
                    if phase.capture() {
                        let _ = root.update(cx, |view, cx| {
                            view.observe_navigation_pointer(None, window, cx);
                        });
                    }
                });
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_region_bridges_both_drawers_and_keeps_an_exit_gutter() {
        let viewport = size(px(1200.), px(800.));
        let sidebar = hover_bounds(viewport, px(280.), None);
        let both = hover_bounds(viewport, px(280.), Some(px(420.)));

        assert!(sidebar.contains(&point(px(300.), px(400.))));
        assert!(!sidebar.contains(&point(px(305.), px(400.))));
        assert!(both.contains(&point(px(284.), px(400.))));
        assert!(both.contains(&point(px(600.), px(400.))));
        assert!(both.contains(&point(px(730.), px(400.))));
        assert!(!both.contains(&point(px(733.), px(400.))));
    }

    #[test]
    fn hover_region_never_extends_outside_the_window() {
        let bounds = hover_bounds(size(px(600.), px(800.)), px(280.), Some(px(312.)));
        assert!(bounds.contains(&point(px(599.), px(400.))));
        assert!(!bounds.contains(&point(px(601.), px(400.))));
        assert!(!bounds.contains(&point(px(-1.), px(400.))));
        assert!(!bounds.contains(&point(px(10.), px(-1.))));
        assert!(!bounds.contains(&point(px(10.), px(801.))));
    }
}
