use gpui::{Context, KeyDownEvent, Window};

use crate::{
    item_manager::ItemManager,
    selection::{SelectionScope, focus_item, focus_item_extending},
    stores::{AppDatabaseStore, WorkspaceContent},
};

use super::{UpcomingReveal, UpcomingView};

impl UpcomingView {
    pub(super) fn on_navigation_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = event.keystroke.modifiers;
        if !self.focus_handle.is_focused(window)
            || event.is_held
            || modifiers.control
            || modifiers.platform
            || modifiers.alt
            || ItemManager::global(cx).read(cx).is_editing()
            || !matches!(
                AppDatabaseStore::global(cx).read(cx).workspace_content(),
                WorkspaceContent::Ready
            )
        {
            return;
        }

        let key = event.keystroke.key.as_str();
        let target = match key {
            "down" | "j" => self.layout.order.first(),
            "up" | "k" => self.layout.order.last(),
            _ => return,
        };
        let Some(&id) = target else {
            return;
        };
        let Some(handle) = self.item_focus_handles.get(&id) else {
            return;
        };
        let Some(&row) = self.layout.positions.get(&id) else {
            return;
        };
        let section = self.agenda.rows[self.layout.rows[row]].section();

        if modifiers.shift && matches!(key, "up" | "down") {
            focus_item_extending(&self.order, id, id, handle, window, cx);
        } else {
            focus_item(SelectionScope::Upcoming, id, handle, window, cx);
        }
        self.reveal = Some(UpcomingReveal::Item { id, section });
        window.refresh();
        cx.stop_propagation();
    }
}
