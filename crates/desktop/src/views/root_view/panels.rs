use super::{RootView, TOP_EDGE_INSET, navigation::WorkspaceRoute};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use crate::views::{LIBRARY_HEADER_HEIGHT, SEARCH_HEADER_HEIGHT};
use crate::{
    AppIcon,
    color::ColorExt,
    components::{
        Button, ButtonVariants as _, DragData, DraggedItems, Label,
        panel_group::{CenterPanel, PanelGroup, SidePanel},
        transition::{self, WindowTransitionExt as _},
    },
    views::SelectedMainView,
};
use gpui::{
    AnyElement, Context, DragMoveEvent, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Pixels, Styled, Window, WindowControlArea, div, prelude::FluentBuilder, px,
};
use gpui_kit::{
    foundation::{StyledExt as _, ThemeOverlay},
    layout::{DesktopTitlebar, ScrollEdgeEffect, ScrollFade},
    overlay::{GlassExt as _, GlassPreset},
};
use gpui_kit_theme::{ActiveTheme, Appearance, Elevation, Radius, Surface};

pub(super) const NAVIGATION_SIDEBAR_MIN_WIDTH: gpui::Pixels = px(180.);
pub(super) const NAVIGATION_SIDEBAR_MAX_WIDTH: gpui::Pixels = px(320.);
pub(super) const WORKSPACE_PANEL_GAP: gpui::Pixels = px(8.);
pub(super) const WORKSPACE_PANEL_BORDER_WIDTH: gpui::Pixels = px(1.);
const LIBRARY_DRAWER_WIDTH: gpui::Pixels = px(420.);

impl RootView {
    pub(super) fn render_navigation_drawer(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let compact = self.layout_state.read(cx).is_compact();
        let slide =
            window.keyed_transition("navigation-drawer-slide", cx, transition::QUICK, || 0.0);
        if !compact {
            slide.snap(0.0, cx);
            return None;
        }
        slide.set(
            if self.navigation_drawer_open {
                1.0
            } else {
                0.0
            },
            cx,
        );
        let progress = slide.animate(window, cx);
        if !self.navigation_drawer_open && progress <= f32::EPSILON {
            return None;
        }
        let width = px(280.).min((window.viewport_size().width - px(48.)).max(px(0.)));
        Some(
            div()
                .absolute()
                .inset_0()
                .overflow_hidden()
                .child(
                    div()
                        .id("navigation.backdrop")
                        .absolute()
                        .inset_0()
                        .occlude()
                        .bg(gpui::black().alpha(0.24 * progress))
                        .on_any_mouse_down(cx.listener(|view, _, window, cx| {
                            crate::selection::SelectionManager::claim_press(cx);
                            view.close_navigation_drawer(window, cx);
                            cx.stop_propagation();
                        })),
                )
                .child(
                    div()
                        .id("navigation.drawer")
                        .absolute()
                        .top_0()
                        .left(width * (progress - 1.0))
                        .w(width)
                        .h_full()
                        .occlude()
                        .overflow_hidden()
                        .child(self.render_navigation_panel(cx)),
                )
                .into_any_element(),
        )
    }

    pub(super) fn render_library_drawer(
        &mut self,
        left_panel_width: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let viewport = window.viewport_size();
        let available = (viewport.width - left_panel_width - WORKSPACE_PANEL_GAP).max(px(0.));
        let width = LIBRARY_DRAWER_WIDTH.min(available);
        let top_inset = if cfg!(target_os = "windows") {
            TOP_EDGE_INSET
        } else {
            px(0.)
        };
        let height = (viewport.height - top_inset - WORKSPACE_PANEL_GAP * 2.0).max(px(0.));
        let target = if self.library_drawer_open { 1.0 } else { 0.0 };
        let slide =
            window.keyed_transition("library-drawer-slide", cx, transition::QUICK, || target);
        if self.layout_state.read(cx).is_compact() {
            slide.snap(0.0, cx);
            return None;
        }
        slide.set(target, cx);
        let rendered = self.library_drawer_open || slide.is_animating(cx) || slide.value(cx) > 0.0;
        let progress = slide.animate(window, cx);
        if !self.library_drawer_open && progress <= f32::EPSILON {
            if let Some(route) = self.pending_full_source_route.take() {
                self.route = route;
                self.prepare_source_route(route, cx);
                self.pending_navigation_focus = true;
                cx.notify();
            }
            return None;
        }
        if !rendered || width <= px(0.) {
            return None;
        }

        let offset = -width + (width + WORKSPACE_PANEL_GAP) * progress;
        let route = self.library_drawer_route;
        let frame = div()
            .id("library-drawer")
            .w(width)
            .h(height)
            .occlude()
            .p(WORKSPACE_PANEL_BORDER_WIDTH)
            .overflow_hidden()
            .child(self.render_source_route(route, true, cx));
        let panel = div()
            .absolute()
            .top(WORKSPACE_PANEL_GAP)
            .left(offset)
            .w(width)
            .h(height)
            .child(
                frame
                    .bg_glass()
                    .glass_surface(Surface::Overlay)
                    .glass_radius(Radius::Dialog)
                    .glass(|glass| glass.protect_text_contrast(false))
                    .when(cfg!(not(target_os = "macos")), |frame| {
                        frame.glass_preset(GlassPreset::Frosted)
                    }),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(cx.theme().radius(Radius::Dialog)))
                    .border(WORKSPACE_PANEL_BORDER_WIDTH)
                    .border_color(cx.theme().colors.hairline),
            );

        Some(
            div()
                .absolute()
                .top(top_inset)
                .bottom_0()
                .left(left_panel_width)
                .right_0()
                .overflow_hidden()
                .child(panel)
                .into_any_element(),
        )
    }

    pub(super) fn render_navigation_toggle(&self, open: bool, cx: &mut Context<Self>) -> Button {
        Button::new("navigation.toggle")
            .ghost()
            .compact()
            .size_7()
            .icon(if open {
                AppIcon::PanelLeftClose
            } else {
                AppIcon::PanelLeftOpen
            })
            .tooltip(if open {
                "Hide navigation"
            } else {
                "Show navigation"
            })
            .on_drag_move::<DragData<DraggedItems>>(cx.listener(
                |view, event: &DragMoveEvent<DragData<DraggedItems>>, window, cx| {
                    if event.bounds.contains(&event.event.position)
                        && !event.drag(cx).data.items.is_empty()
                    {
                        view.open_navigation_drawer(window, cx);
                    }
                },
            ))
            .on_click(cx.listener(|view, _, window, cx| view.toggle_navigation(window, cx)))
    }

    pub(super) fn navigation_toggle_left(left_panel_width: Pixels) -> Pixels {
        let closed_left = if cfg!(target_os = "macos") {
            px(96.)
        } else {
            px(8.)
        };
        (left_panel_width - px(28.) - px(8.) - WORKSPACE_PANEL_BORDER_WIDTH).max(closed_left)
    }

    pub(super) fn windows_title_left(left_panel_width: Pixels) -> Pixels {
        let reveal = (left_panel_width / NAVIGATION_SIDEBAR_MIN_WIDTH).clamp(0., 1.);
        px(50.) - px(40.) * reveal
    }

    pub(super) fn render_windows_title(left: Pixels) -> gpui::Div {
        div()
            .absolute()
            .top_0()
            .left(left)
            .h(TOP_EDGE_INSET)
            .flex()
            .items_center()
            .window_control_area(WindowControlArea::Drag)
            .on_mouse_down(MouseButton::Left, |event, window, cx| {
                if event.click_count >= 2 && window.is_resizable() {
                    window.zoom_window();
                } else {
                    window.start_window_move();
                }
                cx.stop_propagation();
            })
            .on_mouse_down(MouseButton::Right, |event, window, cx| {
                window.show_window_menu(event.position);
                cx.stop_propagation();
            })
            .child(
                Label::new("Subroutine Lite")
                    .debug_selector(|| "window.titlebar-title".into())
                    .text_sm(),
            )
    }

    pub(super) fn render_windows_navigation_toggle(
        navigation: impl IntoElement,
        left: Pixels,
    ) -> gpui::Div {
        div()
            .debug_selector(|| "window.navigation-toggle".into())
            .absolute()
            .top((TOP_EDGE_INSET - px(28.)) / 2.)
            .left(left)
            .window_control_area(WindowControlArea::Client)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .child(navigation)
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn workspace_header_viewport(height: Pixels, left: Pixels, right: Pixels) -> gpui::Div {
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .when(cfg!(target_os = "windows"), |header| {
                header.left(left).right(right)
            })
            .h(height)
            .debug_selector(|| "workspace.header".into())
    }

    pub(super) fn render_windows_titlebar(left: Pixels, with_material: bool) -> impl IntoElement {
        let frame = div()
            .id("window.titlebar-material")
            .debug_selector(|| "window.titlebar".into())
            .w_full()
            .h(TOP_EDGE_INSET)
            .child(ThemeOverlay::new(
                |theme| {
                    theme.clone().modify(|theme| {
                        theme.colors.panel = theme.colors.panel.alpha(0.);
                    })
                },
                DesktopTitlebar::new("window.titlebar", "")
                    .on_event(|event, window, _cx| event.apply(window)),
            ));
        let frame = if with_material {
            frame
                .bg_glass()
                .glass_surface(Surface::Canvas)
                .glass_radius_px(0.0)
                .glass(|glass| {
                    glass
                        .protect_text_contrast(false)
                        .elevation(Elevation::Flat)
                })
                .glass_preset(GlassPreset::Frosted)
                .into_any_element()
        } else {
            frame.into_any_element()
        };
        div()
            .absolute()
            .top_0()
            .left(left)
            .right_0()
            .block_mouse_except_scroll()
            .child(frame)
    }

    fn render_top_edge(content: impl IntoElement) -> impl IntoElement {
        let content = ScrollFade::new("window.top-edge.fade")
            .top(true)
            .band(f32::from(TOP_EDGE_INSET))
            .text_only()
            .child(content);
        if cfg!(target_os = "windows") {
            content.into_any_element()
        } else {
            ScrollEdgeEffect::new("window.top-edge")
                .top(true)
                .soft()
                .band(40.)
                .blur(12.)
                .child(content)
                .into_any_element()
        }
    }

    pub(super) fn content_owns_top_material(&self, cx: &gpui::App) -> bool {
        let main_owns_top_material = self.route == WorkspaceRoute::Main
            && matches!(
                self.main_view.read(cx).selected_view(),
                SelectedMainView::Calendar | SelectedMainView::Focus
            );
        matches!(
            self.route,
            WorkspaceRoute::Search
                | WorkspaceRoute::Unqueued
                | WorkspaceRoute::Routines
                | WorkspaceRoute::SavedItems
        ) || main_owns_top_material
    }

    pub(super) fn render_workspace(
        &mut self,
        content_owns_top_material: bool,
        cx: &mut Context<Self>,
    ) -> PanelGroup {
        let center = div()
            .relative()
            .size_full()
            .overflow_hidden()
            .capture_any_mouse_down(cx.listener(|view, _, window, cx| {
                view.close_library_drawer(window, cx);
            }))
            .map(|content| match self.route {
                WorkspaceRoute::Home => content.child(self.home_view.clone()),
                WorkspaceRoute::Main => content.child(self.main_view.clone()),
                route => content.child(self.render_source_route(route, false, cx)),
            });
        let workspace = PanelGroup::new(self.layout_state.clone())
            .absolute()
            .inset_0()
            .when(!self.layout_state.read(cx).is_compact(), |workspace| {
                workspace.left(self.render_navigation_panel(cx))
            })
            .center(CenterPanel::new().child(center));
        if content_owns_top_material {
            workspace
        } else {
            workspace.content_wrapper(|content| Self::render_top_edge(content).into_any_element())
        }
    }

    fn render_navigation_panel(&self, cx: &mut Context<Self>) -> SidePanel {
        SidePanel::left()
            .width_range_open(NAVIGATION_SIDEBAR_MIN_WIDTH..NAVIGATION_SIDEBAR_MAX_WIDTH)
            .when(cfg!(target_os = "macos"), |panel| {
                panel.p(WORKSPACE_PANEL_GAP)
            })
            .pr_0()
            .child(
                div()
                    .size_full()
                    .overflow_hidden()
                    .bg(cx.theme().colors.canvas)
                    .border_color(cx.theme().colors.hairline)
                    .when_else(
                        cfg!(target_os = "macos"),
                        |panel| {
                            panel
                                .rounded(px(cx.theme().radius(Radius::Dialog)))
                                .border(WORKSPACE_PANEL_BORDER_WIDTH)
                                .shadow_sm()
                                .bg(match cx.theme().appearance {
                                    Appearance::Light => cx.theme().colors.canvas,
                                    Appearance::Dark => cx
                                        .theme()
                                        .colors
                                        .canvas
                                        .mix(gpui::black(), 0.95)
                                        .alpha(0.96),
                                })
                        },
                        |panel| panel.border_r_1(),
                    )
                    .child(self.render_sidebar(cx)),
            )
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(super) fn render_workspace_header(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<(Pixels, AnyElement, bool)> {
        let window_header = match self.route {
            WorkspaceRoute::Main => self
                .main_view
                .update(cx, |main, cx| main.render_calendar_header(cx))
                .map(|(height, foreground)| (height, foreground, true)),
            WorkspaceRoute::Search
            | WorkspaceRoute::Unqueued
            | WorkspaceRoute::Routines
            | WorkspaceRoute::SavedItems => {
                let height = if self.route == WorkspaceRoute::Search {
                    SEARCH_HEADER_HEIGHT
                } else {
                    LIBRARY_HEADER_HEIGHT
                };
                Some((
                    height,
                    self.render_source_header(self.route, false, cx),
                    false,
                ))
            }
            WorkspaceRoute::Home => None,
        };
        #[cfg(target_os = "windows")]
        let window_header = window_header.or_else(|| {
            (self.route != WorkspaceRoute::Main
                || self.main_view.read(cx).selected_view() != SelectedMainView::Focus)
                .then(|| (TOP_EDGE_INSET, div().into_any_element(), false))
        });
        window_header
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(super) fn with_workspace_header(
        &self,
        workspace: PanelGroup,
        window_header: Option<(Pixels, AnyElement, bool)>,

        cx: &mut Context<Self>,
    ) -> PanelGroup {
        if let Some((height, foreground, calendar)) = window_header {
            let titlebar_height = if calendar { TOP_EDGE_INSET } else { px(0.) };
            let height = height + titlebar_height;
            let foreground = div()
                .size_full()
                .pt(titlebar_height)
                .overflow_hidden()
                .capture_any_mouse_down(cx.listener(|view, _, window, cx| {
                    view.close_library_drawer(window, cx);
                }))
                .block_mouse_except_scroll()
                .child(foreground);
            workspace.content_overlay(move |left, right, _, cx| {
                Self::workspace_header_viewport(height, left, right)
                    .child(
                        div()
                            .id("workspace.header-material")
                            .bg_glass()
                            .glass_surface(Surface::Canvas)
                            .glass_radius_px(0.0)
                            .glass(|glass| glass.protect_text_contrast(false))
                            .when(cfg!(target_os = "windows"), |header| {
                                header
                                    .glass_preset(GlassPreset::Frosted)
                                    .glass(|glass| glass.elevation(Elevation::Flat))
                            })
                            .size_full()
                            .when(calendar, |header| {
                                header.border_b_1().border_color(cx.theme().colors.hairline)
                            })
                            .child(
                                div()
                                    .row()
                                    .size_full()
                                    .when(cfg!(not(target_os = "windows")), |row| {
                                        row.child(div().w(left).h_full().flex_shrink_0())
                                    })
                                    .child(div().flex_1().min_w_0().h_full().child(foreground))
                                    .when(cfg!(not(target_os = "windows")), |row| {
                                        row.child(div().w(right).h_full().flex_shrink_0())
                                    }),
                            ),
                    )
                    .into_any_element()
            })
        } else {
            workspace
        }
    }
}
