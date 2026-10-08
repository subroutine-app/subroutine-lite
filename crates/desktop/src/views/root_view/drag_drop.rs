use super::{RootView, navigation::WorkspaceRoute};
use crate::{
    components::{DraggedItems, timed_toast},
    selection::SelectionManager,
    stores::AppDatabaseStore,
    views::{
        SelectedMainView,
        drag_navigation::DRAG_NAVIGATION_DELAY,
        drop_confirmation::confirm_drop,
        library_drop::{LibraryDropItem, LibraryDropTarget},
    },
};
use gpui::{App, Context, IntoElement, MouseButton, Pixels, Window, canvas};
use gpui_kit::{display::badge::Tone, overlay::toast};
use std::collections::HashSet;
use subroutine_core::AnyItem;

impl RootView {
    fn clear_drag_feedback(&mut self, cx: &mut Context<Self>) {
        self.main_view
            .update(cx, |main, cx| main.finish_item_drag(cx));
        self.unqueued_view
            .update(cx, |unqueued, cx| unqueued.clear_drop_target(cx));
        self.saved_items_view
            .update(cx, |saved, cx| saved.clear_drag_feedback(cx));
    }

    pub(super) fn handle_item_drag_move(&mut self, cx: &mut Context<Self>) {
        self.item_drag_active = true;
        if !self.library_drawer_open {
            return;
        }
        self.library_drawer_open = false;
        SelectionManager::clear_global(cx);
        self.unqueued_view
            .update(cx, |unqueued, cx| unqueued.clear_drop_target(cx));
        self.saved_items_view
            .update(cx, |saved, cx| saved.clear_drop_target(cx));
        cx.notify();
    }

    pub(super) fn cancel_drag_navigation(&mut self) {
        self.drag_navigation.clear();
        self.drag_navigation_task = None;
    }

    pub(super) fn hover_scheduler_view(
        &mut self,
        target: SelectedMainView,
        position: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sidebar_open = self.navigation_open(cx);
        if self.current_overlay.is_some()
            || self.context_menu.entity().read(cx).is_open()
            || !sidebar_open
        {
            self.cancel_drag_navigation();
            return;
        }
        if self.route == WorkspaceRoute::Main && self.main_view.read(cx).selected_view() == target {
            if self.drag_navigation.pending() == Some(target) {
                self.cancel_drag_navigation();
            }
            return;
        }
        if let Some(ticket) = self.drag_navigation.hover(target, position) {
            self.drag_navigation_task = Some(cx.spawn_in(window, async move |view, cx| {
                cx.background_executor().timer(DRAG_NAVIGATION_DELAY).await;
                let _ = view.update_in(cx, |view, window, cx| {
                    let sidebar_open = view.navigation_open(cx);
                    let active = view.item_drag_active
                        && cx.has_active_drag()
                        && window.is_window_active()
                        && sidebar_open
                        && view.current_overlay.is_none()
                        && !view.context_menu.entity().read(cx).is_open()
                        && AppDatabaseStore::global(cx).read(cx).is_ready();
                    let target =
                        view.drag_navigation
                            .complete(ticket, window.mouse_position(), active);
                    if view.drag_navigation.pending().is_none() {
                        view.drag_navigation_task = None;
                    }
                    if let Some(target) = target {
                        view.clear_drag_feedback(cx);
                        view.select_navigation(target.id(), window, cx);
                    }
                });
            }));
        } else if self.drag_navigation.pending().is_none() {
            self.drag_navigation_task = None;
        }
    }

    pub(super) fn finish_item_drag(&mut self, cx: &mut Context<Self>) {
        self.cancel_drag_navigation();
        self.item_drag_active = false;
        self.clear_drag_feedback(cx);
        cx.notify();
    }

    pub(super) fn drag_lifecycle_observer(&self, cx: &Context<Self>) -> impl IntoElement {
        let root = cx.entity().downgrade();
        canvas(
            |_, _, _| (),
            move |_, _, window, cx| {
                SelectionManager::settle_geometry(cx);
                SelectionManager::track_background_clicks(window);
                let pressed = root.clone();
                window.on_mouse_event(move |event: &gpui::MouseDownEvent, phase, _, cx| {
                    if phase.capture() && event.button == MouseButton::Left {
                        let _ = pressed.update(cx, |view, cx| {
                            view.cancel_drag_navigation();
                            if view.item_drag_active {
                                view.finish_item_drag(cx);
                            }
                        });
                    }
                });
                let released = root.clone();
                window.on_mouse_event(move |event: &gpui::MouseUpEvent, phase, window, cx| {
                    if phase.capture() && event.button == MouseButton::Left {
                        let _ = released.update(cx, |view, cx| {
                            view.cancel_drag_navigation();
                            if view.item_drag_active {
                                cx.defer_in(window, |view, _, cx| {
                                    if view.item_drag_active {
                                        view.finish_item_drag(cx);
                                    }
                                });
                            }
                        });
                    }
                });
                let cancelled = root.clone();
                window.on_mouse_event(move |_: &gpui::MouseCancelEvent, phase, _, cx| {
                    if phase.capture() {
                        let _ = cancelled.update(cx, |view, cx| {
                            if view.item_drag_active || view.drag_navigation.pending().is_some() {
                                view.finish_item_drag(cx);
                            }
                        });
                    }
                });
            },
        )
    }

    pub(super) fn library_drop_items(dragged: &DraggedItems, cx: &App) -> Vec<LibraryDropItem> {
        let store = AppDatabaseStore::global(cx);
        let store = store.read(cx);
        if !store.is_ready() {
            return Vec::new();
        }
        dragged
            .items
            .iter()
            .filter_map(|item| {
                let current = store.get_item(item.id());
                let template_exists = current.is_none()
                    && match item {
                        AnyItem::Action(action) => action
                            .template_id
                            .is_some_and(|id| store.get_action_template(id).is_some()),
                        AnyItem::Event(event) => event
                            .template_id
                            .is_some_and(|id| store.get_event_template(id).is_some()),
                        _ => false,
                    };
                LibraryDropItem::resolve(
                    item,
                    dragged.saved_item_ids.as_deref(),
                    dragged.materialized_item_ids.contains(&item.id()),
                    current,
                    template_exists,
                )
            })
            .collect()
    }

    pub(super) fn commit_library_drop(
        &mut self,
        target: LibraryDropTarget,
        dragged: &DraggedItems,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let items = Self::library_drop_items(dragged, cx);
        let Some(plan) = target.plan(&items) else {
            return;
        };
        let total = dragged
            .items
            .iter()
            .map(AnyItem::id)
            .collect::<HashSet<_>>()
            .len();
        let ignored = total.saturating_sub(plan.accepted);
        let partial_result = (ignored > 0).then(|| target.success_message(plan.accepted, ignored));
        let (verb, detail) = match target {
            LibraryDropTarget::Unqueued => (
                "Move",
                "Move these actions to Unqueued, removing their scheduled dates and pins.",
            ),
            LibraryDropTarget::SavedItems => (
                "Save",
                "Save these items for reuse in Saved Items. The original items will stay where they are.",
            ),
            LibraryDropTarget::Routines => (
                "Save",
                "Create one routine from these actions. The original actions will stay where they are.",
            ),
        };
        self.close_navigation_drawer(window, cx);
        self.finish_item_drag(cx);
        confirm_drop(
            plan.accepted,
            verb,
            detail.into(),
            window,
            cx,
            move |window, cx| {
                let committed = AppDatabaseStore::global(cx)
                    .update(cx, |store, cx| store.update_items(plan.items, cx))
                    .is_some();
                if committed {
                    SelectionManager::clear_global(cx);
                    if let Some(message) = partial_result {
                        toast::push(
                            window,
                            cx,
                            timed_toast("library-drop.partial", message).tone(Tone::Warning),
                        );
                    }
                } else {
                    toast::push(
                        window,
                        cx,
                        timed_toast(
                            "library-drop.failed",
                            "The items could not be moved or saved. Try again.",
                        )
                        .tone(Tone::Warning),
                    );
                }
            },
        );
    }
}
