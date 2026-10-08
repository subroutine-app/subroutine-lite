use super::{
    CurrentOverlay, OpenItemInspector, OpenSavedItemInspector, RootView, navigation::WorkspaceRoute,
};
use crate::{
    components::{CloseOverlay, timed_toast},
    item_subject::ItemSubjectKey,
    selection::{SelectionManager, SelectionScope},
    stores::{AppDatabaseStore, DataChanged},
    views::{ItemInspector, SelectedMainView},
};
use gpui::{
    Context, Entity, InteractiveElement, ParentElement, Styled, Window, div,
    prelude::FluentBuilder, px,
};
use gpui_kit::{
    display::badge::Tone,
    overlay::{GlassExt as _, GlassPreset, toast},
};
use gpui_kit_theme::{ActiveTheme, Radius, Surface};
use subroutine_core::AnyItem;

fn item_card_inspector_scope(
    scope: Option<SelectionScope>,
    selected_main_view: SelectedMainView,
) -> Option<SelectionScope> {
    scope.or(match selected_main_view {
        SelectedMainView::Timeline => Some(SelectionScope::Timeline),
        SelectedMainView::Calendar => Some(SelectionScope::Calendar),
        SelectedMainView::Queue => Some(SelectionScope::Queue),
        SelectedMainView::Focus => Some(SelectionScope::Focus),
    })
}

fn inspector_switch_blocked(
    dirty: bool,
    current: Option<ItemSubjectKey>,
    requested: ItemSubjectKey,
) -> bool {
    dirty && current != Some(requested)
}

impl RootView {
    pub(super) fn observe_inspected_item(
        inspector: &Entity<ItemInspector>,
        store: &Entity<AppDatabaseStore>,
        cx: &mut Context<Self>,
    ) {
        let inspector_for_store = inspector.clone();
        cx.subscribe(store, move |_view, store, _: &DataChanged, cx| {
            let Some(key) = inspector_for_store.read(cx).current_key() else {
                return;
            };
            inspector_for_store.update(cx, |inspector, cx| match key {
                ItemSubjectKey::Live(id) => match store.read(cx).get_item(id) {
                    Some(item) => inspector.show(Some(item), cx),
                    None if inspector.is_dirty(cx) => inspector.mark_unavailable(cx),
                    None => inspector.show(None, cx),
                },
                ItemSubjectKey::Saved(id) => match store.read(cx).get_saved_item(id) {
                    Some(item) => inspector.show_saved(Some(item), cx),
                    None if inspector.is_dirty(cx) => inspector.mark_unavailable(cx),
                    None => inspector.show_saved(None, cx),
                },
            });
        })
        .detach();
    }

    pub(super) fn open_item_inspector(
        &mut self,
        request: &OpenItemInspector,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = AppDatabaseStore::global(cx).read(cx).get_item(request.0) else {
            toast::push(
                window,
                cx,
                timed_toast(
                    "item-inspector.unavailable",
                    "This item is no longer available to edit.",
                )
                .tone(Tone::Warning),
            );
            return;
        };
        let scope = if self.route == WorkspaceRoute::Main {
            item_card_inspector_scope(request.1, self.main_view.read(cx).selected_view())
        } else {
            request.1
        };
        self.open_inspected_item(item, scope, window, cx);
    }

    pub(super) fn open_inspected_item(
        &mut self,
        item: AnyItem,
        scope: Option<SelectionScope>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .current_overlay
            .as_ref()
            .is_some_and(|(overlay, _)| !matches!(overlay, CurrentOverlay::ItemEditor))
        {
            return;
        }

        let current = self.inspector.read(cx).current_key();
        let blocked = inspector_switch_blocked(
            self.inspector.read(cx).is_dirty(cx),
            current,
            ItemSubjectKey::Live(item.id()),
        );
        let persisted = AppDatabaseStore::global(cx)
            .read(cx)
            .get_item(item.id())
            .is_some();
        self.inspector.update(cx, |inspector, cx| {
            if persisted || blocked {
                inspector.show(Some(item.clone()), cx);
            } else {
                inspector.show_transient(item.clone(), cx);
            }
        });

        if blocked {
            self.inspector
                .read(cx)
                .editor_focus_handle(cx)
                .focus(window, cx);
            toast::push(
                window,
                cx,
                timed_toast(
                    "item-inspector.unsaved",
                    "Save or revert the current changes before editing another item.",
                )
                .tone(Tone::Warning),
            );
            return;
        }

        if let Some(scope) = scope {
            SelectionManager::select_for_inspection(scope, item.id(), cx);
        }
        let previous_focus = self
            .current_overlay
            .as_ref()
            .and_then(|(_, focus)| focus.clone())
            .or_else(|| window.focused(cx));
        self.current_overlay = Some((CurrentOverlay::ItemEditor, previous_focus));

        let inspector = self.inspector.clone();
        cx.on_next_frame(window, move |_, window, cx| {
            inspector.read(cx).editor_focus_handle(cx).focus(window, cx);
        });
        cx.notify();
    }

    pub(super) fn open_saved_item_inspector(
        &mut self,
        request: &OpenSavedItemInspector,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = AppDatabaseStore::global(cx)
            .read(cx)
            .get_saved_item(request.0)
        else {
            toast::push(
                window,
                cx,
                timed_toast(
                    "saved-item-inspector.unavailable",
                    "This saved item is no longer available to edit.",
                )
                .tone(Tone::Warning),
            );
            return;
        };
        if self
            .current_overlay
            .as_ref()
            .is_some_and(|(overlay, _)| !matches!(overlay, CurrentOverlay::ItemEditor))
        {
            return;
        }

        let key = ItemSubjectKey::Saved(item.id());
        let blocked = inspector_switch_blocked(
            self.inspector.read(cx).is_dirty(cx),
            self.inspector.read(cx).current_key(),
            key,
        );
        self.inspector.update(cx, |inspector, cx| {
            inspector.show_saved(Some(item.clone()), cx)
        });
        if blocked {
            self.inspector
                .read(cx)
                .editor_focus_handle(cx)
                .focus(window, cx);
            toast::push(
                window,
                cx,
                timed_toast(
                    "item-inspector.unsaved",
                    "Save or revert the current changes before editing another item.",
                )
                .tone(Tone::Warning),
            );
            return;
        }

        SelectionManager::select_for_inspection(SelectionScope::SavedItems, item.id(), cx);
        let previous_focus = self
            .current_overlay
            .as_ref()
            .and_then(|(_, focus)| focus.clone())
            .or_else(|| window.focused(cx));
        self.current_overlay = Some((CurrentOverlay::ItemEditor, previous_focus));
        let inspector = self.inspector.clone();
        cx.on_next_frame(window, move |_, window, cx| {
            inspector.read(cx).editor_focus_handle(cx).focus(window, cx);
        });
        cx.notify();
    }

    pub(super) fn toggle_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(
            self.current_overlay.as_ref(),
            Some((CurrentOverlay::ItemEditor, _))
        ) {
            self.close_overlay(window, cx);
            return;
        }

        let selection = SelectionManager::global(cx);
        let (scope, saved_id) = {
            let selected = selection.read(cx);
            let scope = selected.scope();
            let saved_id = (scope == Some(SelectionScope::SavedItems))
                .then(|| selected.ids().last().copied())
                .flatten();
            (scope, saved_id)
        };
        if let Some(id) = saved_id {
            self.open_saved_item_inspector(&OpenSavedItemInspector(id), window, cx);
            return;
        }
        let Some(item) = SelectionManager::selected_items(cx).into_iter().last() else {
            toast::push(
                window,
                cx,
                timed_toast("item-editor.no-selection", "Select an item to edit.")
                    .tone(Tone::Warning),
            );
            return;
        };
        self.open_inspected_item(item, scope, window, cx);
    }

    pub(super) fn render_item_editor(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let editor_height = (window.viewport_size().height - px(32.))
            .max(px(560.))
            .min(px(760.));
        let frame = div()
            .id("item-editor")
            .w_full()
            .h(editor_height)
            .rounded(px(cx.theme().radius(Radius::Dialog)))
            .border_1()
            .border_color(cx.theme().colors.hairline)
            .overflow_hidden()
            .on_any_mouse_down(|_, _, cx| SelectionManager::claim_press(cx))
            .capture_action::<gpui_kit::controls::input::Cancel>(|_, window, cx| {
                cx.stop_propagation();
                window.dispatch_action(Box::new(CloseOverlay), cx);
            })
            .child(self.inspector.clone());

        div()
            .w_full()
            .min_w(px(640.))
            .max_w(px(760.))
            .h(editor_height)
            .m_4()
            .child(
                frame
                    .bg_glass()
                    .glass_surface(Surface::Panel)
                    .glass_radius(Radius::Dialog)
                    .glass(|glass| glass.protect_text_contrast(false))
                    .when(cfg!(not(target_os = "macos")), |frame| {
                        frame.glass_preset(GlassPreset::Frosted)
                    }),
            )
    }
}
