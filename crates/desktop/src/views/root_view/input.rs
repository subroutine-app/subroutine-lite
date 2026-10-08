use super::{
    ChooseTheme, CurrentOverlay, EditSelectedItem, FocusNext, FocusPrevious, GoToCalendar,
    GoToFocus, GoToHome, GoToQueue, GoToRoutines, GoToSavedItems, GoToTimeline, GoToUnqueued,
    NavigationDestination, NextTab, OpenConfigurationFolder, OpenItemInspector,
    OpenSavedItemInspector, PreviousTab, Redo, RootView, StartCommandPalette, StartEventCreator,
    StartItemCreator, StartItemCreatorOnDate, StartMarkerCreator, StartMarkerCreatorOnRange,
    StartQueuedActionCreator, StartRoutine, StartRoutineCreator, StartSignalCreator, SyncNow,
    ToggleLeftSidebar, ToggleRightSidebar, Undo, UseSavedAction, UseSavedEvent, ViewScheduledItem,
    navigation::{ViewDirection, WorkspaceRoute},
};
use crate::{
    components::CloseOverlay,
    keys::ConfigurableCommand,
    selection::{
        self, CompleteSelected, CopySelected, CutSelected, DeleteSelected, Dismiss,
        DuplicateSelected, PasteItems, SelectAllItems, SelectionManager, TogglePinnedSelected,
        ToggleQueuedSelected,
    },
    stores::AppDatabaseStore,
    views::{CreatorMode, DEFAULT_CREATOR_MODE, SelectedMainView},
};
use gpui::{App, Context, Div, FocusHandle, InteractiveElement, Window, prelude::FluentBuilder};

pub(super) fn bind_dismiss(
    root: Div,
    listener: impl Fn(&Dismiss, &mut Window, &mut App) + 'static,
) -> Div {
    root.on_action(listener).on_key_down(|event, window, cx| {
        if event.keystroke.key == "escape" && !event.keystroke.modifiers.modified() {
            window.dispatch_action(Box::new(Dismiss), cx);
            cx.stop_propagation();
        }
    })
}

pub(super) fn dismiss_root(focus: &FocusHandle, window: &mut Window, cx: &mut App) {
    let scope = SelectionManager::global(cx).read(cx).scope();
    if !selection::dismiss_view(scope, Some(focus), window, cx) {
        cx.propagate();
    }
}

impl RootView {
    fn on_root_dismiss(&mut self, _: &Dismiss, window: &mut Window, cx: &mut Context<Self>) {
        if self.current_overlay.is_none() && self.navigation_drawer_open {
            self.close_navigation_drawer(window, cx);
        } else if self.current_overlay.is_none() && self.library_drawer_open {
            self.close_library_drawer(window, cx);
        } else {
            dismiss_root(&self.focus_handle, window, cx);
        }
    }

    pub(super) fn bind_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {

        if matches!(self.current_overlay, Some((CurrentOverlay::BulkDrop(_), _))) {
            return root;
        }
        if matches!(self.current_overlay, Some((CurrentOverlay::Settings(_), _))) {
            return root.on_action(cx.listener(|view, _: &CloseOverlay, window, cx| {
                view.close_overlay(window, cx);
            }));
        }

        bind_dismiss(root, cx.listener(Self::on_root_dismiss))
            .on_action(cx.listener(|view, _: &StartCommandPalette, window, cx| {
                view.open_command_palette(window, cx);
            }))
            .on_action(cx.listener(|view, _: &SyncNow, window, cx| {
                view.sync_now(window, cx);
            }))
            .on_action(
                cx.listener(|view, _: &OpenConfigurationFolder, window, cx| {
                    view.open_configuration_folder(window, cx);
                }),
            )
            .on_action(cx.listener(|view, _: &EditSelectedItem, window, cx| {
                view.edit_selected_item(window, cx);
            }))
            .on_action(cx.listener(|view, _: &StartRoutine, window, cx| {
                view.open_command_palette_page(Some(ConfigurableCommand::StartRoutine), window, cx);
            }))
            .on_action(cx.listener(|view, _: &ChooseTheme, window, cx| {
                view.open_command_palette_page(Some(ConfigurableCommand::ChooseTheme), window, cx);
            }))
            .on_action(cx.listener(|view, _: &UseSavedAction, window, cx| {
                view.open_command_palette_page(
                    Some(ConfigurableCommand::UseSavedAction),
                    window,
                    cx,
                );
            }))
            .on_action(cx.listener(|view, _: &UseSavedEvent, window, cx| {
                view.open_command_palette_page(
                    Some(ConfigurableCommand::UseSavedEvent),
                    window,
                    cx,
                );
            }))
            .map(|root| self.bind_item_actions(root, cx))
            .map(|root| self.bind_navigation_actions(root, cx))
            .map(|root| self.bind_selection_actions(root, cx))
    }

    fn bind_item_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        root.on_action(cx.listener(|view, _: &StartItemCreator, window, cx| {
            view.open_creator(DEFAULT_CREATOR_MODE, window, cx);
        }))
        .on_action(
            cx.listener(|view, action: &StartItemCreatorOnDate, window, cx| {
                view.open_creator_on_date(action.0, action.1, window, cx);
            }),
        )
        .on_action(
            cx.listener(|view, action: &StartQueuedActionCreator, window, cx| {
                view.open_queued_creator(action.0, window, cx);
            }),
        )
        .on_action(
            cx.listener(|view, action: &StartMarkerCreatorOnRange, window, cx| {
                view.open_marker_creator_on_range(action.0, window, cx);
            }),
        )
        .on_action(cx.listener(|view, action: &ViewScheduledItem, window, cx| {
            let destination = action.0;
            let item_id = action.1;
            let start = action.2;
            if view.library_drawer_open {
                SelectionManager::clear_global(cx);
            }
            view.dismiss_navigation_drawer(cx);
            view.library_drawer_open = false;
            view.pending_full_source_route = None;
            view.route = WorkspaceRoute::Main;
            cx.on_next_frame(window, move |view, window, cx| {
                view.main_view.update(cx, |main, cx| {
                    main.view_scheduled_item(destination, item_id, start, window, cx);
                });
            });
        }))
        .on_action(cx.listener(|view, action: &OpenItemInspector, window, cx| {
            view.open_item_inspector(action, window, cx);
        }))
        .on_action(
            cx.listener(|view, action: &OpenSavedItemInspector, window, cx| {
                view.open_saved_item_inspector(action, window, cx);
            }),
        )
        .on_action(cx.listener(|view, _: &StartEventCreator, window, cx| {
            view.open_creator(CreatorMode::Event, window, cx);
        }))
        .on_action(cx.listener(|view, _: &StartRoutineCreator, window, cx| {
            view.open_creator(CreatorMode::Routine, window, cx);
        }))
        .on_action(cx.listener(|view, _: &StartMarkerCreator, window, cx| {
            view.open_creator(CreatorMode::Marker, window, cx);
        }))
        .on_action(cx.listener(|view, _: &StartSignalCreator, window, cx| {
            view.open_creator(CreatorMode::Signal, window, cx);
        }))
    }

    fn bind_navigation_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        let root = root.when(self.current_overlay.is_none(), |root| {
            root.on_action(cx.listener(|view, _: &NextTab, window, cx| {
                view.switch_view(ViewDirection::Next, window, cx);
            }))
            .on_action(cx.listener(|view, _: &PreviousTab, window, cx| {
                view.switch_view(ViewDirection::Previous, window, cx);
            }))
        });
        root.on_action(cx.listener(|view, _: &GoToHome, window, cx| {
            view.navigate_to_view(NavigationDestination::Home, window, cx);
        }))
        .on_action(cx.listener(|view, _: &GoToTimeline, window, cx| {
            view.navigate_to_view(
                NavigationDestination::Main(SelectedMainView::Timeline),
                window,
                cx,
            );
        }))
        .on_action(cx.listener(|view, _: &GoToCalendar, window, cx| {
            view.navigate_to_view(
                NavigationDestination::Main(SelectedMainView::Calendar),
                window,
                cx,
            );
        }))
        .on_action(cx.listener(|view, _: &GoToQueue, window, cx| {
            view.navigate_to_view(
                NavigationDestination::Main(SelectedMainView::Queue),
                window,
                cx,
            );
        }))
        .on_action(cx.listener(|view, _: &GoToFocus, window, cx| {
            view.navigate_to_view(
                NavigationDestination::Main(SelectedMainView::Focus),
                window,
                cx,
            );
        }))
        .on_action(cx.listener(|view, _: &GoToUnqueued, window, cx| {
            view.navigate_to_view(NavigationDestination::Unqueued, window, cx);
        }))
        .on_action(cx.listener(|view, _: &GoToRoutines, window, cx| {
            view.navigate_to_view(NavigationDestination::Routines, window, cx);
        }))
        .on_action(cx.listener(|view, _: &GoToSavedItems, window, cx| {
            view.navigate_to_view(NavigationDestination::SavedItems, window, cx);
        }))
        .on_action(cx.listener(|view, _: &CloseOverlay, window, cx| {
            view.close_overlay(window, cx);
        }))
        .on_action(cx.listener(|view, _: &ToggleLeftSidebar, window, cx| {
            view.toggle_navigation(window, cx);
        }))
        .on_action(cx.listener(|view, _: &ToggleRightSidebar, window, cx| {
            view.toggle_inspector(window, cx);
        }))
    }

    fn bind_selection_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        root.on_action(cx.listener(|_view, _: &Undo, _window, cx| {
            let store = AppDatabaseStore::global(cx);
            store.update(cx, |store, cx| {
                store.undo(cx);
            });
        }))
        .on_action(cx.listener(|_view, _: &Redo, _window, cx| {
            let store = AppDatabaseStore::global(cx);
            store.update(cx, |store, cx| {
                store.redo(cx);
            });
        }))
        .on_action(|_: &FocusNext, window, cx| window.focus_next(cx))
        .on_action(|_: &FocusPrevious, window, cx| window.focus_prev(cx))
        .on_action(cx.listener(|view, _: &CompleteSelected, window, cx| {
            let handled = view.route.owns_main_commands(
                view.library_drawer_open || view.pending_full_source_route.is_some(),
            ) && view
                .main_view
                .update(cx, |main, cx| main.complete_focus_item(window, cx));
            if !handled {
                selection::complete_selected(window, cx);
            }
        }))
        .on_action(|_: &ToggleQueuedSelected, _, cx| selection::toggle_queued_selected(cx))
        .on_action(|_: &TogglePinnedSelected, _, cx| selection::toggle_pinned_selected(cx))
        .on_action(|_: &DeleteSelected, window, cx| selection::delete_selected(window, cx))
        .on_action(|_: &CopySelected, _, cx| selection::copy_selected(cx))
        .on_action(|_: &CutSelected, window, cx| selection::cut_selected(window, cx))
        .on_action(|_: &PasteItems, _, cx| selection::paste_items(cx))
        .on_action(|_: &DuplicateSelected, _, cx| selection::duplicate_selected(cx))
        .on_action(|_: &SelectAllItems, _, cx| selection::select_all_items(cx))
    }

    pub(super) fn dispatch_pending_main_command(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(command) = self.pending_main_command.take()
            && self.route == WorkspaceRoute::Main
            && !self.library_drawer_open
        {
            cx.on_next_frame(window, move |view, window, cx| {
                if view.route == WorkspaceRoute::Main && !view.library_drawer_open {
                    let focus = view.main_view.read(cx).selected_view_focus_handle(cx);
                    focus.focus(window, cx);
                    window.dispatch_action(command.action(), cx);
                }
            });
        }
    }
}
