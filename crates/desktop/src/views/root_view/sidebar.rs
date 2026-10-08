use std::{cell::RefCell, rc::Rc};

use super::{
    FocusNext, FocusPrevious, RootView, StartItemCreator, TOP_EDGE_INSET,
    navigation::WorkspaceRoute,
    panels::{WORKSPACE_PANEL_BORDER_WIDTH, WORKSPACE_PANEL_GAP},
};
use crate::{
    AppIcon,
    app::{ShowAccountSettings, ShowSettings},
    auth::{AuthSession, AuthenticationState},
    components::{Button, ButtonVariants as _, DragData, DraggedItems, Label},
    icons::Icon,
    stores::AppDatabaseStore,
    views::{SelectedMainView, drag_navigation::is_scheduler, library_drop::LibraryDropTarget},
};
use gpui::{
    App, Context, Div, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement as _, Styled, Window, actions, canvas, div, prelude::FluentBuilder,
    px,
};
use gpui_kit::{
    display::avatar::Avatar,
    foundation::{Selectable as _, StyledExt as _},
};
use gpui_kit_semantics::{NodeSpec, Role, Semantic as _};
use gpui_kit_theme::ActiveTheme;

actions!(sidebar, [FocusPreviousButton, FocusNextButton]);

pub(super) fn init(cx: &mut App) {
    cx.bind_keys([
        crate::keys::key("up", FocusPreviousButton, Some("NavigationSidebar")),
        crate::keys::key("down", FocusNextButton, Some("NavigationSidebar")),
    ]);
}

#[derive(Clone)]
struct SidebarNavigation {
    handles: Rc<RefCell<Vec<FocusHandle>>>,
    tab_stop: bool,
}

impl SidebarNavigation {
    fn button(&self, button: Button) -> Button {
        let handles = self.handles.clone();
        button
            .tab_stop(self.tab_stop)
            .on_focus_resolved(move |_, handle, _, _| {
                if let Some(handle) = handle.filter(|handle| handle.tab_stop) {
                    handles.borrow_mut().push(handle.clone());
                }
            })
    }

    fn bind(&self, sidebar: Div, trap_focus: bool) -> Div {
        let previous = self.clone();
        let next = self.clone();
        sidebar
            .key_context("NavigationSidebar")
            .on_action(move |_: &FocusPreviousButton, window, cx| {
                previous.step(-1, false, window, cx);
            })
            .on_action(move |_: &FocusNextButton, window, cx| {
                next.step(1, false, window, cx);
            })
            .when(trap_focus, |sidebar| {
                let previous = self.clone();
                let next = self.clone();
                sidebar
                    .on_action(move |_: &FocusPrevious, window, cx| {
                        previous.step(-1, true, window, cx);
                    })
                    .on_action(move |_: &FocusNext, window, cx| {
                        next.step(1, true, window, cx);
                    })
            })
    }

    fn step(&self, offset: isize, wrap: bool, window: &mut Window, cx: &mut App) {
        let handles = self.handles.borrow();
        if handles.is_empty() {
            return;
        }
        let index = handles
            .iter()
            .position(|handle| handle.contains_focused(window, cx));
        let target = if wrap {
            let index = index.map_or(if offset < 0 { 0 } else { -1 }, |index| index as isize);
            handles.get((index + offset).rem_euclid(handles.len() as isize) as usize)
        } else {
            index
                .and_then(|index| index.checked_add_signed(offset))
                .and_then(|index| handles.get(index))
        };
        if let Some(target) = target {
            target.focus(window, cx);
        }
    }
}

fn main_navigation_icon(view: SelectedMainView) -> AppIcon {
    match view {
        SelectedMainView::Timeline => AppIcon::Timeline,
        SelectedMainView::Calendar => AppIcon::Calendar,
        SelectedMainView::Queue => AppIcon::ListChecks,
        SelectedMainView::Focus => AppIcon::ScanEye,
    }
}

impl RootView {
    pub(super) fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.active_navigation_id(cx);
        let navigation = SidebarNavigation {
            handles: Rc::default(),
            tab_stop: self.navigation_open(cx),
        };
        let account_button = navigation.button(self.render_account_button(cx));
        let top_inset = if cfg!(target_os = "macos") {
            TOP_EDGE_INSET - WORKSPACE_PANEL_GAP - WORKSPACE_PANEL_BORDER_WIDTH
        } else {
            TOP_EDGE_INSET
        };

        let search = Button::new("navigation.search")
            .map(|button| navigation.button(button))
            .ghost()
            .h_9()
            .w_full()
            .justify_start()
            .px_3()
            .gap_3()
            .rounded_lg()
            .icon(Icon::from_path("icons/regular/magnifying-glass.svg"))
            .label("Search items")
            .text_color(cx.theme().colors.text_muted)
            .selected(active == WorkspaceRoute::Search.id())
            .when(active == WorkspaceRoute::Search.id(), |button| {
                button
                    .text_color(cx.theme().colors.text)
                    .font_weight(gpui::FontWeight::SEMIBOLD)
            })
            .on_click({
                let root = cx.entity().clone();
                move |_, window, cx| {
                    root.update(cx, |root, cx| root.show_search(window, cx));
                }
            });

        let settings = Button::new("navigation.settings")
            .map(|button| navigation.button(button))
            .ghost()
            .compact()
            .size_9()
            .flex_none()
            .rounded_lg()
            .text_color(cx.theme().colors.text_muted)
            .icon(AppIcon::Settings)
            .tooltip("Settings")
            .on_click(cx.listener(|view, _, window, cx| {
                view.close_navigation_drawer(window, cx);
                window.dispatch_action(Box::new(ShowSettings), cx);
            }));

        div()
            .track_focus(&self.navigation_focus)
            .map(|sidebar| navigation.bind(sidebar, self.navigation_drawer_open))
            .column()
            .size_full()
            .child(div().h(top_inset).w_full().flex_none())
            .child(
                div()
                    .column()
                    .flex_none()
                    .px_2()
                    .pb_4()
                    .gap_4()
                    .child(
                        div()
                            .row()
                            .w_full()
                            .gap_1()
                            .child(account_button)
                            .child(settings),
                    )
                    .child(
                        div()
                            .column()
                            .gap_1()
                            .child(
                                Button::new("navigation.new-item")
                                    .map(|button| navigation.button(button))
                                    .primary()
                                    .h_9()
                                    .w_full()
                                    .px_3()
                                    .gap_3()
                                    .justify_start()
                                    .rounded_lg()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .icon(AppIcon::Plus)
                                    .label("New item")
                                    .tooltip("Create a new item")
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.close_navigation_drawer(window, cx);
                                        window.dispatch_action(Box::new(StartItemCreator), cx);
                                    })),
                            )
                            .child(search),
                    ),
            )
            .child(
                div()
                    .id("navigation.contents")
                    .column()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.sidebar_scroll)
                    .px_2()
                    .pb_4()
                    .gap_5()
                    .child(
                        div()
                            .column()
                            .flex_none()
                            .gap_1()
                            .child(
                                div().px_2().pb_1().child(
                                    Label::new("Views")
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(cx.theme().colors.text_muted),
                                ),
                            )
                            .child(
                                self.render_route_button("home", "Home", AppIcon::Home, active, cx)
                                    .map(|button| navigation.button(button)),
                            )
                            .children(SelectedMainView::ALL.into_iter().map(|view| {
                                navigation
                                    .button(self.render_main_navigation_button(view, active, cx))
                            })),
                    )
                    .child(
                        div()
                            .column()
                            .flex_none()
                            .gap_1()
                            .child(
                                div().px_2().pb_1().child(
                                    Label::new("Library")
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(cx.theme().colors.text_muted),
                                ),
                            )
                            .child(
                                self.render_route_button(
                                    "unqueued",
                                    "Unqueued",
                                    AppIcon::Inbox,
                                    active,
                                    cx,
                                )
                                .map(|button| navigation.button(button)),
                            )
                            .child(
                                self.render_route_button(
                                    "routines",
                                    "Routines",
                                    AppIcon::Repeat,
                                    active,
                                    cx,
                                )
                                .map(|button| navigation.button(button)),
                            )
                            .child(
                                self.render_route_button(
                                    "saved-items",
                                    "Saved items",
                                    AppIcon::Archive,
                                    active,
                                    cx,
                                )
                                .map(|button| navigation.button(button)),
                            ),
                    ),
            )
    }

    fn render_main_navigation_button(
        &self,
        view: SelectedMainView,
        active: &str,
        cx: &Context<Self>,
    ) -> Button {
        let root = cx.entity().clone();
        let id = view.id();
        Button::new(format!("navigation.main.{id}"))
            .reveal_on_focus(&self.sidebar_scroll, px(4.))
            .ghost()
            .h_9()
            .w_full()
            .justify_start()
            .px_3()
            .gap_3()
            .rounded_lg()
            .icon(main_navigation_icon(view))
            .label(view.name())
            .selected(active == id)
            .when(!self.navigation_drawer_open, |button| {
                button.current(active == id)
            })
            .when(active == id, |button| {
                button.font_weight(gpui::FontWeight::SEMIBOLD)
            })
            .when(is_scheduler(view), |button| {
                let measured = root.downgrade();
                button
                    .tooltip(format!("Drag items here to switch to {}", view.name()))
                    .child(
                        canvas(
                            move |bounds, window, cx| {
                                let visible = bounds.intersect(&window.content_mask().bounds);
                                let _ = measured.update(cx, |root, _| {
                                    root.drag_navigation.record_bounds(view, visible);
                                });
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .drag_over::<DragData<DraggedItems>>(|style, data, _, cx| {
                        if data.data.items.is_empty() {
                            style
                        } else {
                            style
                                .bg(cx.theme().colors.selected)
                                .text_color(cx.theme().colors.accent)
                        }
                    })
                    .on_drag_move::<DragData<DraggedItems>>({
                        let root = root.clone();
                        move |event, window, cx| {
                            if !event.drag(cx).data.items.is_empty() {
                                root.update(cx, |root, cx| {
                                    root.hover_scheduler_view(
                                        view,
                                        event.event.position,
                                        window,
                                        cx,
                                    );
                                });
                            }
                        }
                    })
            })
            .on_click({
                let root = root.clone();
                move |_, window, cx| {
                    root.update(cx, |root, cx| root.select_navigation(id, window, cx));
                }
            })
    }

    fn render_route_button(
        &self,
        id: &'static str,
        label: &'static str,
        icon: AppIcon,
        active: &str,
        cx: &Context<Self>,
    ) -> Button {
        let root = cx.entity().clone();
        Button::new(format!("navigation.route.{id}"))
            .reveal_on_focus(&self.sidebar_scroll, px(4.))
            .ghost()
            .h_9()
            .w_full()
            .justify_start()
            .px_3()
            .gap_3()
            .rounded_lg()
            .icon(icon)
            .label(label)
            .selected(active == id)
            .when(
                id == WorkspaceRoute::Home.id() && !self.navigation_drawer_open,
                |button| button.current(active == id),
            )
            .when(active == id, |button| {
                button.font_weight(gpui::FontWeight::SEMIBOLD)
            })
            .when_some(LibraryDropTarget::from_id(id), |button, target| {
                button
                    .tooltip(target.drop_hint())
                    .can_drop(move |value, _, cx| {
                        value
                            .downcast_ref::<DragData<DraggedItems>>()
                            .is_some_and(|data| {
                                Self::library_drop_items(&data.data, cx)
                                    .iter()
                                    .any(|item| target.accepts(&item.item, item.from_saved_items))
                            })
                    })
                    .drag_over::<DragData<DraggedItems>>(|style, _, _, cx| {
                        style
                            .bg(cx.theme().colors.selected)
                            .text_color(cx.theme().colors.accent)
                    })
                    .on_drop::<DragData<DraggedItems>>({
                        let root = root.clone();
                        move |data, window, cx| {
                            cx.stop_propagation();
                            root.update(cx, |root, cx| {
                                root.commit_library_drop(target, &data.data, window, cx);
                            });
                        }
                    })
            })
            .on_click({
                let root = root.clone();
                move |_, window, cx| {
                    root.update(cx, |root, cx| root.select_navigation(id, window, cx));
                }
            })
    }

    fn render_account_button(&self, cx: &Context<Self>) -> Button {
        let auth = AuthSession::global(cx);
        let signed_in = auth.is_signed_in();
        let offline = signed_in
            && (matches!(auth.state(), AuthenticationState::Offline(_))
                || AppDatabaseStore::global(cx).read(cx).sync_status()
                    == crate::stores::SyncStatus::Offline);
        let name = if signed_in { auth.display_name() } else { None };
        let label = name.clone().unwrap_or_else(|| "Account".to_owned());
        let hint = if offline {
            "Offline — open account settings"
        } else {
            "Open account settings"
        };
        Button::new("navigation.account")
            .ghost()
            .h_9()
            .flex_1()
            .min_w_0()
            .pl_2()
            .pr_3()
            .gap_2()
            .justify_start()
            .rounded_lg()
            .child(
                Avatar::new(name.unwrap_or_default())
                    .id("navigation.account.avatar")
                    .size(24.),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(label),
                    )
                    .when(offline, |account| {
                        account.child(
                            div()
                                .id("navigation.account.offline")
                                .flex_none()
                                .text_color(cx.theme().colors.warning)
                                .child(Icon::new(AppIcon::Info).size_4())
                                .semantic_in(
                                    cx,
                                    NodeSpec::new("navigation.account.offline", Role::Status)
                                        .text("Offline"),
                                ),
                        )
                    }),
            )
            .tooltip(hint)
            .on_click(cx.listener(|view, _, window, cx| {
                view.close_navigation_drawer(window, cx);
                window.dispatch_action(Box::new(ShowAccountSettings), cx);
            }))
    }
}
