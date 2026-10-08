use super::{NavigationDestination, RootView};
use crate::{
    item_manager::ItemManager,
    selection::{SelectionManager, SelectionScope},
    settings::Settings,
    stores::AppDatabaseStore,
    views::{SavedItemsFilter, SelectedMainView, SourceKind},
};
use gpui::{App, Context, FocusHandle, Focusable, Window};

#[derive(Clone, Copy)]
pub(super) enum ViewDirection {
    Next,
    Previous,
}

impl NavigationDestination {
    const ALL: [Self; 9] = [
        Self::Home,
        Self::Main(SelectedMainView::Queue),
        Self::Main(SelectedMainView::Timeline),
        Self::Main(SelectedMainView::Calendar),
        Self::Main(SelectedMainView::Focus),
        Self::Unqueued,
        Self::Routines,
        Self::SavedItems,
        Self::Search,
    ];

    fn step(self, direction: ViewDirection) -> Self {
        let index = Self::ALL
            .iter()
            .position(|destination| *destination == self)
            .expect("every workspace destination belongs to the view cycle");
        let offset = match direction {
            ViewDirection::Next => 1,
            ViewDirection::Previous => -1,
        };
        Self::ALL[(index as isize + offset).rem_euclid(Self::ALL.len() as isize) as usize]
    }

    fn id(self) -> &'static str {
        match self {
            Self::Main(view) => view.id(),
            destination => destination.route().id(),
        }
    }

    fn route(self) -> WorkspaceRoute {
        match self {
            Self::Home => WorkspaceRoute::Home,
            Self::Main(_) => WorkspaceRoute::Main,
            Self::Search => WorkspaceRoute::Search,
            Self::Unqueued => WorkspaceRoute::Unqueued,
            Self::Routines => WorkspaceRoute::Routines,
            Self::SavedItems => WorkspaceRoute::SavedItems,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WorkspaceRoute {
    Home,
    Main,
    Search,
    Unqueued,
    Routines,
    SavedItems,
}

impl WorkspaceRoute {
    fn destination(self, main_view: SelectedMainView) -> NavigationDestination {
        match self {
            Self::Home => NavigationDestination::Home,
            Self::Main => NavigationDestination::Main(main_view),
            Self::Search => NavigationDestination::Search,
            Self::Unqueued => NavigationDestination::Unqueued,
            Self::Routines => NavigationDestination::Routines,
            Self::SavedItems => NavigationDestination::SavedItems,
        }
    }

    pub(super) fn drawer_background(self, previous: Self) -> Self {
        match self {
            Self::Home | Self::Main => self,
            Self::Search | Self::Unqueued | Self::Routines | Self::SavedItems => previous,
        }
    }

    pub(super) fn owns_main_commands(self, drawer_open: bool) -> bool {
        self == Self::Main && !drawer_open
    }
    pub(super) const fn id(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Main => "main",
            Self::Search => "search",
            Self::Unqueued => "unqueued",
            Self::Routines => "routines",
            Self::SavedItems => "saved-items",
        }
    }
}

impl RootView {
    fn active_navigation_destination(&self, cx: &App) -> NavigationDestination {
        self.pending_full_source_route
            .or_else(|| {
                self.library_drawer_open
                    .then_some(self.library_drawer_route)
            })
            .unwrap_or(self.route)
            .destination(self.main_view.read(cx).selected_view())
    }

    pub(super) fn active_navigation_id(&self, cx: &App) -> &'static str {
        self.active_navigation_destination(cx).id()
    }

    pub(super) fn switch_view(
        &mut self,
        direction: ViewDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let destination = self.active_navigation_destination(cx).step(direction);
        self.navigate_to_view(destination, window, cx);
    }

    pub(super) fn prepare_shell_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let changed_mode = self.layout_state.update(cx, |layout, _| {
            let was_compact = layout.is_compact();
            layout.prepare_layout(gpui::Bounds::new(
                gpui::Point::default(),
                window.viewport_size(),
            ));
            was_compact != layout.is_compact()
        });
        if changed_mode {
            self.dismiss_navigation_drawer(cx);
            self.cancel_drag_navigation();
            if self.current_overlay.is_none() && self.navigation_focus.contains_focused(window, cx)
            {
                self.pending_navigation_focus = true;
            }
        }
        if self.layout_state.read(cx).is_compact() {
            let route = self
                .pending_full_source_route
                .take()
                .inspect(|route| {
                    self.prepare_source_route(*route, cx);
                })
                .or_else(|| {
                    self.library_drawer_open
                        .then_some(self.library_drawer_route)
                });
            if let Some(route) = route {
                self.library_drawer_open = false;
                self.route = route;
                self.pending_navigation_focus = true;
            }
        }
    }

    pub(super) fn navigation_open(&self, cx: &App) -> bool {
        let layout = self.layout_state.read(cx);
        if layout.is_compact() {
            self.navigation_drawer_open
        } else {
            layout.left_panel.as_ref().is_some_and(|panel| panel.open)
        }
    }

    pub(super) fn open_navigation_drawer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.layout_state.read(cx).is_compact()
            || self.navigation_drawer_open
            || self.current_overlay.is_some()
        {
            return;
        }
        self.navigation_drawer_open = true;
        self.pending_navigation_focus = false;
        cx.on_next_frame(window, |view, window, cx| {
            if view.navigation_drawer_open
                && view.current_overlay.is_none()
                && !cx.has_active_drag()
            {
                view.navigation_focus.focus(window, cx);
                window.focus_next(cx);
            }
        });
        cx.notify();
    }

    pub(super) fn dismiss_navigation_drawer(&mut self, cx: &mut Context<Self>) {
        if self.navigation_drawer_open {
            self.navigation_drawer_open = false;
            self.cancel_drag_navigation();
            cx.notify();
        }
    }

    pub(super) fn close_navigation_drawer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.navigation_drawer_open {
            self.dismiss_navigation_drawer(cx);
            self.workspace_focus_handle(cx).focus(window, cx);
        }
    }

    pub(super) fn toggle_navigation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.layout_state.read(cx).is_compact() {
            if self.navigation_drawer_open {
                self.close_navigation_drawer(window, cx);
            } else {
                self.open_navigation_drawer(window, cx);
            }
            return;
        }
        let open = self.layout_state.update(cx, |state, cx| {
            state.toggle_left();
            cx.notify();
            state.left_panel.as_ref().is_some_and(|panel| panel.open)
        });
        if !open {
            self.cancel_drag_navigation();
            self.library_drawer_open = false;
            self.pending_full_source_route = None;
        }
        Settings::update(cx, |settings| {
            settings.desktop_layout.left_panel_open = open;
        });
        cx.notify();
    }

    fn source_route(id: &str) -> Option<WorkspaceRoute> {
        match id {
            "search" => Some(WorkspaceRoute::Search),
            "unqueued" => Some(WorkspaceRoute::Unqueued),
            "routines" => Some(WorkspaceRoute::Routines),
            "saved-items" => Some(WorkspaceRoute::SavedItems),
            _ => None,
        }
    }

    pub(super) fn prepare_source_route(
        &mut self,
        route: WorkspaceRoute,
        cx: &mut Context<Self>,
    ) -> FocusHandle {
        match route {
            WorkspaceRoute::Search => {
                self.set_source_kinds(Vec::new(), cx);
                self.source_search.read(cx).focus_handle(cx)
            }
            WorkspaceRoute::Unqueued => {
                self.set_source_kinds(vec![SourceKind::Unqueued], cx);
                self.clear_source_query(cx);
                self.unqueued_view.read(cx).focus_handle()
            }
            WorkspaceRoute::Routines => {
                self.set_source_kinds(vec![SourceKind::Routine], cx);
                self.clear_source_query(cx);
                self.routines_view.read(cx).focus_handle()
            }
            WorkspaceRoute::SavedItems => {
                self.saved_items_view
                    .update(cx, |saved, cx| saved.set_filter(SavedItemsFilter::All, cx));
                self.clear_source_query(cx);
                self.saved_items_view.read(cx).focus_handle()
            }
            WorkspaceRoute::Home => self.home_view.read(cx).focus_handle(cx),
            WorkspaceRoute::Main => self.main_view.read(cx).focus_handle.clone(),
        }
    }

    pub(super) fn dismiss_library_drawer(&mut self, cx: &mut Context<Self>) {
        if !self.library_drawer_open {
            return;
        }
        self.library_drawer_open = false;
        self.pending_full_source_route = None;
        let scope = match self.library_drawer_route {
            WorkspaceRoute::Search => Some(SelectionScope::Search),
            WorkspaceRoute::Unqueued => Some(SelectionScope::Unqueued),
            WorkspaceRoute::Routines => Some(SelectionScope::Routines),
            WorkspaceRoute::SavedItems => Some(SelectionScope::SavedItems),
            WorkspaceRoute::Home | WorkspaceRoute::Main => None,
        };
        if scope.is_some_and(|scope| {
            SelectionManager::global(cx)
                .read(cx)
                .has_selection_in(scope)
        }) {
            SelectionManager::clear_global(cx);
        }
        cx.notify();
    }

    fn workspace_focus_handle(&self, cx: &App) -> FocusHandle {
        match self.route {
            WorkspaceRoute::Home => self.home_view.read(cx).focus_handle(cx),
            WorkspaceRoute::Main => self.main_view.read(cx).focus_handle.clone(),
            WorkspaceRoute::Search => self.source_search.read(cx).focus_handle(cx),
            WorkspaceRoute::Unqueued => self.unqueued_view.read(cx).focus_handle(),
            WorkspaceRoute::Routines => self.routines_view.read(cx).focus_handle(),
            WorkspaceRoute::SavedItems => self.saved_items_view.read(cx).focus_handle(),
        }
    }

    pub(super) fn close_library_drawer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.library_drawer_open {
            return;
        }
        self.dismiss_library_drawer(cx);
        self.workspace_focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    pub(super) fn toggle_library_drawer(
        &mut self,
        route: WorkspaceRoute,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.layout_state.read(cx).is_compact() {
            self.open_source_full_view(route, window, cx);
            return;
        }
        if self.library_drawer_open && self.library_drawer_route == route {
            self.close_library_drawer(window, cx);
            return;
        }

        self.route = self.route.drawer_background(self.library_background_route);
        self.library_background_route = self.route;
        self.library_drawer_route = route;
        self.library_drawer_open = true;
        self.pending_full_source_route = None;
        let focus = self.prepare_source_route(route, cx);
        SelectionManager::clear_global(cx);
        cx.on_next_frame(window, move |view, window, cx| {
            if view.library_drawer_open && view.library_drawer_route == route {
                focus.focus(window, cx);
            }
        });
        cx.notify();
    }

    pub(super) fn open_source_full_view(
        &mut self,
        route: WorkspaceRoute,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_navigation_drawer(cx);
        self.library_background_route = self.route.drawer_background(self.library_background_route);
        self.library_drawer_open = false;
        if self.layout_state.read(cx).is_compact() {
            self.pending_full_source_route = None;
            self.route = route;
            self.prepare_source_route(route, cx);
            self.pending_navigation_focus = true;
        } else {
            self.pending_full_source_route = Some(route);
        }
        SelectionManager::clear_global(cx);
        cx.notify();
    }

    pub(super) fn select_navigation(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_navigation_drawer(cx);
        self.cancel_drag_navigation();
        if id == WorkspaceRoute::Home.id() {
            SelectionManager::clear_global(cx);
            self.library_drawer_open = false;
            self.pending_full_source_route = None;
            self.route = WorkspaceRoute::Home;
            self.home_view.read(cx).focus_handle(cx).focus(window, cx);
            cx.notify();
            return;
        }

        if let Some(view) = SelectedMainView::from_id(id) {
            if self.route != WorkspaceRoute::Main || self.library_drawer_open {
                SelectionManager::clear_global(cx);
            }
            self.library_drawer_open = false;
            self.pending_full_source_route = None;
            self.route = WorkspaceRoute::Main;
            self.main_view
                .update(cx, |main, cx| main.select_view(view, window, cx));
            cx.notify();
            return;
        }

        if let Some(route) = Self::source_route(id) {
            self.toggle_library_drawer(route, window, cx);
        }
    }

    pub(super) fn navigate_to_view(
        &mut self,
        destination: NavigationDestination,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if destination != self.active_navigation_destination(cx) {
            let manager = ItemManager::global(cx);
            manager.update(cx, |manager, cx| manager.commit_open_edit(window, cx));
            if manager.read(cx).is_editing() {
                return;
            }
        }

        self.cancel_drag_navigation();
        self.pending_main_command = None;
        self.pending_navigation_focus = matches!(
            destination,
            NavigationDestination::Home | NavigationDestination::Main(_)
        );
        match destination {
            NavigationDestination::Home => {
                self.select_navigation(WorkspaceRoute::Home.id(), window, cx);
            }
            NavigationDestination::Main(view) => {
                self.select_navigation(view.id(), window, cx);
            }
            destination => self.open_source_full_view(destination.route(), window, cx),
        }
    }

    pub(super) fn restore_navigation_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_navigation_focus {
            self.pending_navigation_focus = false;
            let route = self.route;
            let selected_view = self.main_view.read(cx).selected_view();
            cx.on_next_frame(window, move |view, window, cx| {
                if view.route != route
                    || view.navigation_drawer_open
                    || view.library_drawer_open
                    || view.pending_full_source_route.is_some()
                    || view.current_overlay.is_some()
                    || !AppDatabaseStore::global(cx).read(cx).is_ready()
                    || (route == WorkspaceRoute::Main
                        && view.main_view.read(cx).selected_view() != selected_view)
                {
                    return;
                }
                let focus = if route == WorkspaceRoute::Main {
                    view.main_view.read(cx).selected_view_focus_handle(cx)
                } else {
                    view.workspace_focus_handle(cx)
                };
                if !focus.contains_focused(window, cx) {
                    focus.focus(window, cx);
                }
            });
        }
    }
}
