mod commands;
mod drag_drop;
mod editor;
mod holiday_setup;
mod input;
mod lifecycle;
mod navigation;
mod navigation_drawer;
mod overlays;
mod panels;
mod sidebar;
mod sources;
mod status;

use super::{command_palette::AppCommandPalette, drag_navigation::DragNavigation};
use crate::{
    components::{DragData, DraggedItems, menu::ContextMenuHost, panel_group::PanelGroupState},
    dates::InclusiveDateRange,
    keys::{ConfigurableCommand, VIEW_SWITCH_KEY_CONTEXT, key},
    selection::{self, KEY_CONTEXT, SelectionScope},
    views::{
        ActionLibraryView, CreatorMode, HomeView, ItemCreator, ItemInspector, MainView, NextTab,
        PreviousTab, RoutinesView, SavedItemsView, ScheduledItemDestination, SearchView,
        SelectedMainView, SettingsView, SourceKind,
    },
    window_material::WindowMaterial,
};
use chrono::NaiveDate;
use commands::ManualSync;
use gpui::{
    Action, App, Context, DragMoveEvent, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Render, Styled, Task, Window,
    WindowControlArea, actions, div, prelude::FluentBuilder, px,
};
use gpui_kit::{
    controls::search::SearchInput,
    foundation::StyledExt as _,
    overlay::{Dialog, ToastLayer},
};
use gpui_kit_theme::ActiveTheme;
use holiday_setup::HolidaySetup;
use navigation::WorkspaceRoute;
use navigation_drawer::NavigationDrawer;
use sources::{SourceFilterPicker, SourceSortPicker};
use subroutine_core::SchedulePoint;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NavigationDestination {
    Home,
    Main(SelectedMainView),
    Search,
    Unqueued,
    Unscheduled,
    Routines,
    SavedItems,
}

#[derive(Action, Clone, PartialEq)]
#[action(namespace = root_view, no_json)]
pub(crate) struct StartItemCreatorOnDate(pub CreatorMode, pub NaiveDate);

#[derive(Action, Clone, PartialEq)]
#[action(namespace = root_view, no_json)]
pub(crate) struct StartQueuedActionCreator(pub Option<NaiveDate>);

#[derive(Action, Clone, PartialEq)]
#[action(namespace = root_view, no_json)]
pub(crate) struct StartMarkerCreatorOnRange(pub InclusiveDateRange);

#[derive(Action, Clone, PartialEq)]
#[action(namespace = root_view, no_json)]
pub(crate) struct ViewScheduledItem(
    pub ScheduledItemDestination,
    pub uuid::Uuid,
    pub SchedulePoint,
);

#[derive(Action, Clone, PartialEq)]
#[action(namespace = root_view, no_json)]
pub(crate) struct OpenItemInspector(pub uuid::Uuid, pub Option<SelectionScope>);

#[derive(Action, Clone, PartialEq)]
#[action(namespace = root_view, no_json)]
pub(crate) struct OpenSavedItemInspector(pub uuid::Uuid);

actions!(
    root_view,
    [
        StartCommandPalette,
        StartItemCreator,
        StartEventCreator,
        StartRoutineCreator,
        StartMarkerCreator,
        StartSignalCreator,
        StartRoutine,
        UseSavedAction,
        UseSavedEvent,
        ChooseTheme,
        SyncNow,
        OpenConfigurationFolder,
        EditSelectedItem,
        GoToHome,
        GoToTimeline,
        GoToCalendar,
        GoToUpcoming,
        ToggleFocusMode,
        GoToUnqueued,
        GoToUnscheduled,
        GoToRoutines,
        GoToSavedItems,
        ToggleLeftSidebar,
        ToggleRightSidebar,
        Undo,
        Redo,
        FocusNext,
        FocusPrevious,
    ]
);

pub fn init(cx: &mut App) {
    sidebar::init(cx);
    cx.bind_keys([
        key("cmd-k", StartCommandPalette, Some(KEY_CONTEXT)),
        key("cmd-n", StartItemCreator, Some(KEY_CONTEXT)),
        key("cmd-shift-n", StartEventCreator, Some(KEY_CONTEXT)),
        key("cmd-alt-n", StartRoutineCreator, Some(KEY_CONTEXT)),
        key("cmd-b", ToggleLeftSidebar, Some(KEY_CONTEXT)),
        key("cmd-alt-f", ToggleFocusMode, Some(KEY_CONTEXT)),
        key("cmd-alt-b", ToggleRightSidebar, Some(KEY_CONTEXT)),
        key("ctrl-tab", NextTab, Some(VIEW_SWITCH_KEY_CONTEXT)),
        key("ctrl-shift-tab", PreviousTab, Some(VIEW_SWITCH_KEY_CONTEXT)),
        key("cmd-z", Undo, Some(KEY_CONTEXT)),
        key("cmd-shift-z", Redo, Some(KEY_CONTEXT)),
        key("tab", FocusNext, Some(KEY_CONTEXT)),
        key("shift-tab", FocusPrevious, Some(KEY_CONTEXT)),
    ]);
}

pub const TOP_EDGE_INSET: gpui::Pixels = if cfg!(target_os = "macos") {
    px(44.)
} else if cfg!(target_os = "windows") {
    px(38.)
} else {
    px(40.)
};

pub enum CurrentOverlay {
    CommandPalette(Entity<AppCommandPalette>),
    ItemCreator(Entity<ItemCreator>),
    ItemEditor,
    Settings(Entity<SettingsView>),
    BulkDrop(Entity<Dialog>),
}

pub struct RootView {
    focus_handle: FocusHandle,
    pending_main_command: Option<ConfigurableCommand>,
    manual_sync: Option<ManualSync>,
    pending_navigation_focus: bool,
    toasts: Entity<ToastLayer>,
    context_menu: ContextMenuHost,
    holiday_setup: Entity<HolidaySetup>,
    home_view: Entity<HomeView>,
    main_view: Entity<MainView>,
    inspector: Entity<ItemInspector>,
    source_search: Entity<SearchInput>,
    source_filter_picker: Entity<SourceFilterPicker>,
    source_sort_picker: Entity<SourceSortPicker>,
    search_view: Entity<SearchView>,
    unqueued_view: Entity<ActionLibraryView>,
    unscheduled_view: Entity<ActionLibraryView>,
    routines_view: Entity<RoutinesView>,
    saved_items_view: Entity<SavedItemsView>,
    source_kinds: Vec<SourceKind>,
    route: WorkspaceRoute,
    window_material: WindowMaterial,

    library_drawer_route: WorkspaceRoute,
    library_drawer_open: bool,
    library_drawer_width: gpui::Pixels,
    library_background_route: WorkspaceRoute,
    pending_full_source_route: Option<WorkspaceRoute>,
    item_drag_active: bool,
    drag_navigation: DragNavigation,
    drag_navigation_task: Option<Task<()>>,
    layout_state: Entity<PanelGroupState>,
    navigation_drawer: NavigationDrawer,
    navigation_drawer_extent: gpui::Pixels,
    navigation_pointer: Option<gpui::Point<gpui::Pixels>>,
    navigation_dismiss_task: Option<Task<()>>,
    navigation_focus: FocusHandle,
    sidebar_scroll: gpui::ScrollHandle,
    current_overlay: Option<(CurrentOverlay, Option<FocusHandle>)>,
}

impl EventEmitter<()> for RootView {}

impl Focusable for RootView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let window_tint = self.window_material.background(window, cx);
        if let Some(status) = self.render_status(cx) {
            let root = div()
                .absolute()
                .inset_0()
                .bg(window_tint)
                .track_focus(&self.focus_handle)
                .map(|root| self.bind_actions(root, cx))
                .child(status)
                .children(self.render_overlay(window, cx))
                .child(self.toasts.clone());
            return crate::views::platform_material_scope(root);
        }

        self.prepare_shell_layout(window, cx);
        if self.navigation_drawer == NavigationDrawer::Hover {
            self.observe_navigation_pointer(self.navigation_pointer, window, cx);
        }
        self.restore_navigation_focus(window, cx);

        let content_owns_top_material = self.content_owns_top_material(cx);
        let show_navigation_toggle = !self.focus_mode_active(cx);
        let left_panel_open =
            self.navigation_panel_open(cx) || self.navigation_drawer == NavigationDrawer::Compact;
        let left_panel_width = self.layout_state.read(cx).animated_left_px;
        let toggle_left = Self::navigation_toggle_left(left_panel_width);
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let window_header = self.render_workspace_header(cx);
        let show_holidays = crate::stores::AppDatabaseStore::global(cx)
            .read(cx)
            .holiday_setup_pending();
        let workspace = self.render_workspace(content_owns_top_material, cx);
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let workspace = self.with_workspace_header(workspace, window_header, cx);
        let (navigation_drawer, navigation_drawer_width) =
            self.render_navigation_drawer(window, cx);
        self.navigation_drawer_extent = navigation_drawer_width;
        let library_drawer =
            self.render_library_drawer(left_panel_width.max(navigation_drawer_width), window, cx);

        let root = div()
            .column()
            .track_focus(&self.focus_handle)
            .when(
                !matches!(
                    self.current_overlay,
                    Some((CurrentOverlay::Settings(_) | CurrentOverlay::BulkDrop(_), _))
                ),
                |root| root.key_context(selection::KEY_CONTEXT),
            )
            .absolute()
            .inset_0()
            .bg(window_tint)
            .text_color(cx.theme().colors.text)
            .on_drag_move::<DragData<DraggedItems>>(cx.listener(
                |view, _event: &DragMoveEvent<DragData<DraggedItems>>, _, cx| {
                    view.handle_item_drag_move(cx)
                },
            ))
            .on_drop::<DragData<DraggedItems>>(
                cx.listener(|view, _, _, cx| view.finish_item_drag(cx)),
            )
            .child(self.drag_lifecycle_observer(cx))
            .child(self.navigation_hover_observer(cx))
            .map(|root| self.bind_actions(root, cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(workspace),
            )
            .when(show_holidays, |root| root.child(self.holiday_setup.clone()))
            .children(library_drawer)
            .children(navigation_drawer)
            .when(cfg!(not(target_os = "windows")), |root| {
                root.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(TOP_EDGE_INSET)
                        .window_control_area(WindowControlArea::Drag)
                        .on_mouse_down(MouseButton::Left, |_event, window, _cx| {
                            window.start_window_move();
                        }),
                )
                .when(show_navigation_toggle, |root| {
                    root.child(
                        div()
                            .absolute()
                            .top(if cfg!(target_os = "macos") {
                                px(14.)
                            } else {
                                px(10.)
                            })
                            .left(toggle_left)
                            .window_control_area(WindowControlArea::Client)
                            .child(self.render_navigation_toggle(left_panel_open, cx)),
                    )
                })
            })
            .children(self.render_overlay(window, cx))
            .when(cfg!(target_os = "windows"), |root| {
                root.child(Self::render_windows_titlebar(
                    left_panel_width,
                    self.current_overlay.is_some(),
                ))
                .child(Self::render_windows_title(Self::windows_title_left(
                    left_panel_width,
                )))
                .when(show_navigation_toggle, |root| {
                    root.child(Self::render_windows_navigation_toggle(
                        self.render_navigation_toggle(left_panel_open, cx),
                        toggle_left,
                    ))
                })
            })
            .child(self.toasts.clone())
            .child(self.context_menu.entity().clone());

        let root = div().absolute().inset_0().child(root).when(
            self.current_overlay.is_none()
                && self.navigation_drawer != NavigationDrawer::Compact
                && self.route == WorkspaceRoute::Main
                && self.pending_full_source_route.is_none(),
            |root| {
                self.main_view
                    .update(cx, |view, cx| view.view_command_scope(root, cx))
            },
        );

        self.dispatch_pending_main_command(window, cx);
        crate::views::platform_material_scope(root)
    }
}
