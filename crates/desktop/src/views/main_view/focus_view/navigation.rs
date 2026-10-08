use gpui::{App, Context, FocusHandle, Window};
use subroutine_core::AnyItem;
use uuid::Uuid;

use crate::{
    item_manager::ItemManager,
    selection::{FocusHandoff, SelectionManager, SelectionOrder, SelectionScope},
};

use super::{FocusMode, FocusView, temporal::EventMoment};

impl FocusView {
    pub(super) fn carousel_active_id(&self) -> Option<Uuid> {
        match self.mode {
            FocusMode::Action => self.active_id,
            FocusMode::Event => self.active_event_id,
        }
    }

    pub(super) fn active_index(&self) -> Option<usize> {
        let active = self.active_id?;
        self.items.iter().position(|item| item.id() == active)
    }

    fn adjacent_index(&self, index: usize, delta: isize, cx: &App) -> Option<usize> {
        let manager = ItemManager::global(cx);
        let manager = manager.read(cx);
        if delta < 0 {
            (0..index)
                .rev()
                .find(|candidate| !manager.is_completing(self.items[*candidate].id()))
        } else {
            ((index + 1)..self.items.len())
                .find(|candidate| !manager.is_completing(self.items[*candidate].id()))
        }
    }

    pub(super) fn completion_target(&self, index: usize, cx: &App) -> Option<(Uuid, FocusHandle)> {
        self.adjacent_index(index, 1, cx)
            .or_else(|| self.adjacent_index(index, -1, cx))
            .and_then(|index| self.items.get(index))
            .and_then(|item| {
                self.item_focus_handles
                    .get(&item.id())
                    .cloned()
                    .map(|handle| (item.id(), handle))
            })
    }

    pub(super) fn activate(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == FocusMode::Event {
            let Some(handle) = self.event_focus_handles.get(&id).cloned() else {
                return;
            };
            self.active_event_id = Some(id);
            self.reset_wheel();
            SelectionManager::clear_global(cx);
            cx.on_next_frame(window, move |view, window, cx| {
                if view.mode == FocusMode::Event && view.active_event_id == Some(id) {
                    handle.focus(window, cx);
                }
            });
            cx.notify();
            return;
        }
        if !self.items.iter().any(|item| item.id() == id) {
            return;
        }

        self.active_id = Some(id);
        self.reset_wheel();
        SelectionManager::global(cx).update(cx, |selection, cx| {
            selection.select_only(SelectionScope::Focus, id, cx);
        });
        cx.notify();

        let Some(handle) = self.item_focus_handles.get(&id).cloned() else {
            return;
        };
        cx.on_next_frame(window, move |view, window, cx| {
            if view.mode == FocusMode::Action && view.active_id == Some(id) {
                handle.focus(window, cx);
            }
        });
    }

    pub(super) fn move_active(
        &mut self,
        delta: isize,
        extend_selection: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.mode == FocusMode::Event {
            let ids = self.navigable_ids(cx);
            if let Some(target) = adjacent_event_id(self.active_event_id, &ids, delta) {
                self.activate(target, window, cx);
            }
            return;
        }
        let Some(current) = self.active_index() else {
            if let Some(first) = self.items.first() {
                self.activate(first.id(), window, cx);
            }
            return;
        };
        let Some(next) = self.adjacent_index(current, delta, cx) else {
            return;
        };
        let origin = self.items[current].id();
        let target = self.items[next].id();
        if !extend_selection {
            self.activate(target, window, cx);
            return;
        }

        let order = SelectionOrder::new(SelectionScope::Focus, self.navigable_ids(cx));
        self.active_id = Some(target);
        self.reset_wheel();
        SelectionManager::global(cx).update(cx, |selection, cx| {
            selection.select_keyboard_range(&order, origin, target, cx)
        });
        cx.notify();
        if let Some(handle) = self.item_focus_handles.get(&target).cloned() {
            cx.on_next_frame(window, move |view, window, cx| {
                if view.mode == FocusMode::Action && view.active_id == Some(target) {
                    handle.focus(window, cx);
                }
            });
        }
    }

    pub(super) fn navigable_ids(&self, cx: &App) -> Vec<Uuid> {
        if self.mode == FocusMode::Event {
            return self
                .event_moments
                .iter()
                .map(EventMoment::notice_id)
                .collect();
        }
        let manager = ItemManager::global(cx);
        let manager = manager.read(cx);
        self.items
            .iter()
            .filter(|item| !manager.is_completing(item.id()))
            .map(AnyItem::id)
            .collect()
    }

    pub(in crate::views::main_view) fn focus_current(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.mode == FocusMode::Event {
            SelectionManager::clear_global(cx);
            if let Some(id) = self.active_event_id {
                self.activate(id, window, cx);
            } else {
                self.focus_handle.focus(window, cx);
            }
            return;
        }

        let target = self.active_index().and_then(|index| {
            let active_id = self.items[index].id();
            let completing = ItemManager::global(cx).read(cx).is_completing(active_id);
            if completing {
                self.adjacent_index(index, 1, cx)
                    .or_else(|| self.adjacent_index(index, -1, cx))
                    .map(|index| self.items[index].id())
            } else {
                Some(active_id)
            }
        });
        match target {
            Some(id) => self.activate(id, window, cx),
            None => {
                SelectionManager::clear_global(cx);
                self.focus_handle.focus(window, cx);
            }
        }
    }

    pub(in crate::views::main_view) fn complete_active(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.mode != FocusMode::Action {
            return;
        }

        let Some(index) = self.active_index() else {
            return;
        };
        let AnyItem::Action(action) = self.items[index].clone() else {
            return;
        };

        let manager = ItemManager::global(cx);
        if manager.read(cx).is_completing(action.id) {
            return;
        }

        SelectionManager::global(cx).update(cx, |selection, cx| {
            selection.select_only(SelectionScope::Focus, action.id, cx);
        });
        let handoff = FocusHandoff::new(
            Some(SelectionScope::Focus),
            action.id,
            self.completion_target(index, cx),
        );
        manager.update(cx, |manager, cx| {
            manager.begin_complete_action(action, Some(handoff), window, cx);
        });
    }
}

fn adjacent_event_id(active: Option<Uuid>, ids: &[Uuid], delta: isize) -> Option<Uuid> {
    let Some(index) = active.and_then(|active| ids.iter().position(|id| *id == active)) else {
        return ids.first().copied();
    };
    index
        .checked_add_signed(delta)
        .and_then(|index| ids.get(index))
        .copied()
}
