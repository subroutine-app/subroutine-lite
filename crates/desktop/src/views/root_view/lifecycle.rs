use super::{
    RootView,
    navigation::WorkspaceRoute,
    panels::{NAVIGATION_SIDEBAR_MAX_WIDTH, NAVIGATION_SIDEBAR_MIN_WIDTH},
    sources::{SourceFilterPicker, SourceSortPicker},
};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use crate::views::SelectedMainView;
use crate::{
    auth::AuthSession,
    components::{
        menu::ContextMenuHost,
        panel_group::{PanelGroupResized, PanelGroupState, SidePanelState},
    },
    selection::SelectionManager,
    settings::Settings,
    stores::AppDatabaseStore,
    views::{
        HomeView, HomeViewEvent, InspectItem, ItemInspector, MainView, RoutinesView,
        SavedItemsView, SearchView, SelectedMainViewChanged, UnqueuedView,
        drag_navigation::DragNavigation,
    },
};
use gpui::{AppContext, Context, Entity, FocusHandle, Focusable, Window};
use gpui_kit::{
    controls::search::SearchInput,
    foundation::Sizable as _,
    overlay::{ToastCorner, ToastLayer},
};
use gpui_kit_theme::ControlSize;

impl RootView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let toasts = cx.new(|cx| ToastLayer::new(window, cx).corner(ToastCorner::BottomRight));
        let context_menu = ContextMenuHost::new("root-view.context-menu", window, cx);

        let layout_state = Self::create_layout_state(cx);
        let store = AppDatabaseStore::global(cx);
        Self::observe_connection(&store, window, cx);
        let home_view = cx.new(|cx| HomeView::new(window, cx));
        cx.on_focus_in(
            &home_view.read(cx).focus_handle(cx),
            window,
            |view, _, cx| view.dismiss_library_drawer(cx),
        )
        .detach();
        let inspector = cx.new(|cx| ItemInspector::new(window, cx));
        let main_view = cx.new(|cx| MainView::new(window, cx));
        Self::observe_main_focus(&main_view, window, cx);
        let source_search = cx.new(|cx| {
            SearchInput::new("workspace.search", window, cx)
                .name("Search all items")
                .placeholder("Search titles, notes, and routine steps")
                .control_size(ControlSize::Lg)
        });
        let source_filter_picker = cx.new(|cx| SourceFilterPicker::new(window, cx));
        let source_sort_picker = cx.new(|cx| SourceSortPicker::new(window, cx));
        let search_view = cx.new(|cx| SearchView::new(window, cx));
        let unqueued_view = cx.new(|cx| UnqueuedView::new(window, cx));
        let routines_view = cx.new(|cx| RoutinesView::new(window, cx));
        let saved_items_view = cx.new(|cx| SavedItemsView::new(window, cx));

        Self::subscribe_source_controls(
            &source_search,
            &source_filter_picker,
            &source_sort_picker,
            window,
            cx,
        );

        Self::subscribe_home_navigation(&home_view, window, cx);

        let initial_fh = home_view.read(cx).focus_handle(cx);
        window.focus(&initial_fh, cx);

        Self::restore_root_focus(&focus_handle, window, cx);

        Self::subscribe_main_navigation(&main_view, window, cx);

        Self::observe_inspected_item(&inspector, &store, cx);

        Self {
            focus_handle,
            pending_main_command: None,
            manual_sync: None,
            pending_navigation_focus: false,
            toasts,
            context_menu,
            home_view,
            main_view,
            inspector,
            source_search,
            source_filter_picker,
            source_sort_picker,
            search_view,
            unqueued_view,
            routines_view,
            saved_items_view,
            source_kinds: Vec::new(),
            route: WorkspaceRoute::Home,

            library_drawer_route: WorkspaceRoute::Unqueued,
            library_drawer_open: false,
            library_background_route: WorkspaceRoute::Home,
            pending_full_source_route: None,
            item_drag_active: false,
            drag_navigation: DragNavigation::default(),
            drag_navigation_task: None,
            layout_state,
            navigation_drawer_open: false,
            navigation_focus: cx.focus_handle(),
            sidebar_scroll: gpui::ScrollHandle::new(),
            current_overlay: None,
        }
    }

    fn create_layout_state(cx: &mut Context<Self>) -> Entity<PanelGroupState> {
        let desktop_layout = Settings::global(cx).desktop_layout;
        let layout_state = cx.new(|_| {
            let mut state = PanelGroupState::default();
            state.left_panel = Some(SidePanelState {
                width_range: NAVIGATION_SIDEBAR_MIN_WIDTH..NAVIGATION_SIDEBAR_MAX_WIDTH,
                opened_proportion: desktop_layout.left_panel_proportion(),
                open: desktop_layout.left_panel_open,
            });
            state
        });
        cx.subscribe(
            &layout_state,
            |_view, _, resized: &PanelGroupResized, cx| {
                if let Some(proportion) = resized.left_proportion {
                    Settings::update(cx, |settings| {
                        settings
                            .desktop_layout
                            .set_left_panel_proportion(proportion)
                    });
                }
            },
        )
        .detach();

        layout_state
    }

    fn observe_connection(
        store: &Entity<AppDatabaseStore>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe_in(
            store,
            window,
            |_, _, error: &crate::stores::SaveFailed, window, cx| {
                gpui_kit::overlay::toast::push(
                    window,
                    cx,
                    crate::components::timed_toast("item.save-failed", error.message.clone())
                        .tone(gpui_kit::display::badge::Tone::Warning),
                );
            },
        )
        .detach();
        cx.observe_in(store, window, |view, _, window, cx| {
            crate::app::update_app_menu(cx);
            view.update_sync_feedback(window, cx);
            cx.notify();
        })
        .detach();
        cx.observe_window_activation(window, |_, window, cx| {
            if window.is_window_active() {
                AppDatabaseStore::global(cx).update(cx, |store, cx| store.recover_connection(cx));
            }
        })
        .detach();
        let auth_changes = AuthSession::global(cx).subscribe();
        cx.spawn_in(window, async move |view, cx| {
            let mut previous_error = None;
            while auth_changes.recv_async().await.is_ok() {
                if view
                    .update_in(cx, |_, window, cx| {
                        let error = match AuthSession::global(cx).state() {
                            crate::auth::AuthenticationState::Error(error)
                            | crate::auth::AuthenticationState::Offline(error) => Some(error),
                            _ => None,
                        };
                        if error != previous_error
                            && let Some(error) = &error
                        {
                            gpui_kit::overlay::toast::push(
                                window,
                                cx,
                                crate::components::timed_toast(
                                    "account.authentication-error",
                                    error.clone(),
                                )
                                .tone(gpui_kit::display::badge::Tone::Warning),
                            );
                        }
                        previous_error = error;
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    fn observe_main_focus(
        main_view: &Entity<MainView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        cx.observe(main_view, |view, main, cx| {
            if view.route == WorkspaceRoute::Main
                && main.read(cx).selected_view() == SelectedMainView::Calendar
            {
                cx.notify();
            }
        })
        .detach();
        cx.on_focus_in(
            &main_view.read(cx).focus_handle(cx),
            window,
            |view, _, cx| view.dismiss_library_drawer(cx),
        )
        .detach();
    }

    fn subscribe_home_navigation(
        home_view: &Entity<HomeView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe_in(
            home_view,
            window,
            |view, _, event: &HomeViewEvent, window, cx| {
                SelectionManager::clear_global(cx);
                view.route = WorkspaceRoute::Main;
                view.library_drawer_open = false;
                view.pending_full_source_route = None;
                match event {
                    HomeViewEvent::OpenFocusEvents => view
                        .main_view
                        .update(cx, |main, cx| main.select_focus_events(window, cx)),
                }
                cx.notify();
            },
        )
        .detach();
    }

    fn restore_root_focus(focus_handle: &FocusHandle, window: &mut Window, cx: &mut Context<Self>) {
        let focus = focus_handle.clone();
        cx.on_focus_lost(window, move |_view, window, cx| {
            focus.focus(window, cx);
        })
        .detach();
        let focus = focus_handle.clone();
        cx.on_focus_out(focus_handle, window, move |_, _event, window, cx| {
            focus.focus(window, cx);
        })
        .detach();
    }

    fn subscribe_main_navigation(
        main_view: &Entity<MainView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe(
            main_view,
            move |view, _, change: &SelectedMainViewChanged, cx| {
                view.dismiss_library_drawer(cx);
                view.pending_full_source_route = None;
                view.route = WorkspaceRoute::Main;
                if Settings::global(cx).desktop_layout.selected_main_view != change.0 {
                    Settings::update(cx, |settings| {
                        settings.desktop_layout.selected_main_view = change.0
                    });
                }
                cx.notify();
            },
        )
        .detach();

        cx.subscribe_in(
            main_view,
            window,
            move |view, _, request: &InspectItem, window, cx| {
                view.open_inspected_item(request.0.clone(), None, window, cx);
            },
        )
        .detach();
    }
}
