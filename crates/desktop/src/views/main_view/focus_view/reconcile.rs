use std::{collections::HashSet, time::Duration};

use gpui::{AsyncApp, Context, Window};
use subroutine_core::AnyItem;
use uuid::Uuid;

use crate::{
    item_manager::ItemManager,
    selection::{SelectionManager, SelectionScope},
    settings::Settings,
};

use super::{
    FocusMode, FocusView,
    temporal::{self, EventMoment},
};

const ENTRANCE_STATE_DURATION: Duration = Duration::from_millis(260);

impl FocusView {
    pub(super) fn reconcile_items(&mut self, cx: &mut Context<Self>) {
        let horizon = Settings::global(cx).focus_action_horizon();
        let mut queue =
            super::super::ordered_focus_actions(&self.source_actions, self.now, horizon);
        let previous_active = self.active_id;
        let previous_index = previous_active
            .and_then(|active| self.items.iter().position(|item| item.id() == active));
        let was_loaded = self.loaded;

        let source_ids: HashSet<Uuid> = queue.iter().map(AnyItem::id).collect();
        let departing = {
            let manager = ItemManager::global(cx);
            let manager = manager.read(cx);
            self.items
                .iter()
                .filter(|item| !source_ids.contains(&item.id()) && manager.is_completing(item.id()))
                .cloned()
                .collect::<Vec<_>>()
        };
        queue.extend(departing);
        let entering: HashSet<Uuid> = source_ids.difference(&self.source_ids).copied().collect();
        self.entering_ids.retain(|id| source_ids.contains(id));
        self.entering_ids.extend(entering.iter().copied());
        if !entering.is_empty() {
            cx.spawn(async move |view, cx: &mut AsyncApp| {
                cx.background_executor()
                    .timer(ENTRANCE_STATE_DURATION)
                    .await;
                let _ = view.update(cx, |view, cx| {
                    view.entering_ids.retain(|id| !entering.contains(id));
                    cx.notify();
                });
            })
            .detach();
        }
        self.source_ids = source_ids;
        self.items = queue;

        let live_ids: HashSet<Uuid> = self.items.iter().map(AnyItem::id).collect();
        self.item_focus_handles
            .retain(|id, _| live_ids.contains(id));
        for item in &self.items {
            self.item_focus_handles
                .entry(item.id())
                .or_insert_with(|| cx.focus_handle());
        }

        if self.active_id.is_none_or(|id| !live_ids.contains(&id)) {
            self.active_id = replacement_index(previous_index, self.items.len())
                .and_then(|index| self.items.get(index))
                .map(AnyItem::id);
        }

        if was_loaded
            && previous_active != self.active_id
            && let Some(previous) = previous_active
        {
            let selection = SelectionManager::global(cx);
            let owned_selection = selection
                .read(cx)
                .is_selected_in(SelectionScope::Focus, previous);
            if owned_selection {
                let replacement = self.active_id;
                selection.update(cx, |selection, cx| {
                    selection.hand_off(SelectionScope::Focus, previous, replacement, cx)
                });
                if let Some(selected) = selection
                    .read(cx)
                    .ids()
                    .last()
                    .copied()
                    .filter(|id| live_ids.contains(id))
                {
                    self.active_id = Some(selected);
                }
                self.restore_focus = true;
            }
        }

        let selection = SelectionManager::global(cx);
        let hidden_selection = {
            let selection = selection.read(cx);
            if selection.has_selection_in(SelectionScope::Focus) {
                selection
                    .ids()
                    .iter()
                    .copied()
                    .filter(|id| !live_ids.contains(id))
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        };
        if !hidden_selection.is_empty() {
            let active_id = self.active_id;
            selection.update(cx, |selection, cx| {
                for id in hidden_selection {
                    selection.hand_off(SelectionScope::Focus, id, active_id, cx);
                }
            });
        }

        if self.mode == FocusMode::Action && previous_active != self.active_id {
            self.reset_wheel();
        }
        self.loaded = true;
        cx.notify();
    }

    pub(super) fn reconcile_events(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = Settings::global(cx);
        let moments = temporal::event_carousel_moments(
            &self.events,
            self.now,
            settings.focus_timing_threshold(),
            settings.focus_horizon(),
        );
        let previous_ids: Vec<_> = self
            .event_moments
            .iter()
            .map(EventMoment::notice_id)
            .collect();
        let ids: Vec<_> = moments.iter().map(EventMoment::notice_id).collect();
        let previous = self.active_event_id;
        let restore_focus = self.mode == FocusMode::Event
            && previous
                .and_then(|id| self.event_focus_handles.get(&id))
                .is_some_and(|handle| handle.contains_focused(window, cx));
        self.active_event_id = retained_event_id(previous, &previous_ids, &ids);
        self.event_moments = moments;
        let live: HashSet<_> = ids.into_iter().collect();
        self.event_focus_handles.retain(|id, _| live.contains(id));
        for id in live {
            self.event_focus_handles
                .entry(id)
                .or_insert_with(|| cx.focus_handle());
        }
        if previous != self.active_event_id && self.mode == FocusMode::Event {
            self.reset_wheel();
            if restore_focus {
                let expected = self.active_event_id;
                let handle = expected
                    .and_then(|id| self.event_focus_handles.get(&id).cloned())
                    .unwrap_or_else(|| self.focus_handle.clone());
                cx.on_next_frame(window, move |view, window, cx| {
                    if view.mode == FocusMode::Event && view.active_event_id == expected {
                        handle.focus(window, cx);
                    }
                });
            }
        }
    }
}

fn retained_event_id(active: Option<Uuid>, previous: &[Uuid], current: &[Uuid]) -> Option<Uuid> {
    if let Some(active) = active {
        if current.contains(&active) {
            return Some(active);
        }
        if let Some(index) = previous.iter().position(|id| *id == active)
            && let Some(id) = previous[index + 1..]
                .iter()
                .chain(previous[..index].iter().rev())
                .find(|id| current.contains(id))
        {
            return Some(*id);
        }
    }
    current.first().copied()
}

pub(super) fn replacement_index(previous_index: Option<usize>, new_len: usize) -> Option<usize> {
    (new_len > 0).then(|| previous_index.unwrap_or_default().min(new_len - 1))
}
