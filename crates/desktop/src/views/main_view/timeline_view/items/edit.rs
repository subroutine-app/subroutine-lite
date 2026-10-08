use std::collections::HashSet;

use anyhow::anyhow;
use chrono::{DateTime, Local, Utc};
use gpui::{App, AsyncApp, Context, FocusHandle, Pixels, Window, px};
use subroutine_core::{Action, AnyItem, Event, Marker, SchedulePoint, Signal};
use uuid::Uuid;

use crate::item_manager::{DraftView, ItemManager, next_batch_draft};
use crate::selection::{
    FocusHandoff, SelectionManager, SelectionOrder, SelectionScope, focus_item,
    focus_item_extending,
};
use crate::settings::{Settings, TimelineCreationKind};
use crate::stores::AppDatabaseStore;
use crate::utils::LogErr;
use crate::views::TOP_EDGE_INSET;

use super::super::{EDGE_HORIZON, TimelineView};
use super::{
    COMPLETE_CHECKBOX_DURATION, ItemNavigation, Lane, NavigationHandoffs, SIGNAL_CARD_HEIGHT,
    TimelineItem, TimelineSlot, TransitionState, item_timeline_span, visual_duration_at,
};

#[derive(Clone)]
struct NavigationEntry {
    id: Uuid,
    focus_handle: FocusHandle,
    start: DateTime<Local>,
    visual_top: Pixels,
    visual_bottom: Pixels,
    mounted: bool,
}

fn adjacent_index(len: usize, current: usize, direction: ItemNavigation) -> Option<usize> {
    match direction {
        ItemNavigation::Previous => current.checked_sub(1),
        ItemNavigation::Next => (current + 1 < len).then_some(current + 1),
    }
}

fn entry_index<T: Ord>(positions: &[T], centre: &T, direction: ItemNavigation) -> Option<usize> {
    if positions.is_empty() {
        return None;
    }
    Some(match direction {
        ItemNavigation::Previous => positions
            .iter()
            .rposition(|position| position <= centre)
            .unwrap_or(0),
        ItemNavigation::Next => positions
            .iter()
            .position(|position| position >= centre)
            .unwrap_or(positions.len() - 1),
    })
}

impl TimelineView {
    pub(in crate::views::main_view::timeline_view) fn is_being_edited(
        &self,
        id: Uuid,
        cx: &App,
    ) -> bool {
        ItemManager::global(cx).read(cx).is_being_edited(id)
    }

    pub(crate) fn add_draft_action(
        &mut self,
        time: DateTime<Local>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Uuid {
        self.add_draft_action_with_duration(time, None, window, cx)
    }

    pub(super) fn add_draft_action_with_duration(
        &mut self,
        time: DateTime<Local>,
        duration: Option<chrono::Duration>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Uuid {
        let action = draft_action(time, duration);
        let id = action.id;
        self.edit_draft(AnyItem::Action(action), None, window, cx);
        id
    }

    pub(crate) fn add_draft_from_double_click(
        &mut self,
        time: DateTime<Local>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match Settings::global(cx).timeline_creation.double_click {
            TimelineCreationKind::Action => {
                self.add_draft_action(time, window, cx);
            }
            TimelineCreationKind::Event => self.add_draft_event(time, window, cx),
        }
    }

    pub(crate) fn add_draft_event(
        &mut self,
        time: DateTime<Local>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.add_draft_event_with_duration(time, chrono::Duration::minutes(60), window, cx);
    }

    pub(super) fn add_draft_event_with_duration(
        &mut self,
        time: DateTime<Local>,
        duration: chrono::Duration,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let time_utc = time.with_timezone(&chrono::Utc);
        let event = Event::new("", time_utc, duration);
        self.edit_draft(AnyItem::Event(event), None, window, cx);
    }

    fn edit_draft(
        &mut self,
        item: AnyItem,
        cursor: Option<DateTime<Utc>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let manager = ItemManager::global(cx);
        manager.update(cx, |manager, cx| {
            manager.commit_open_edit(window, cx);
        });
        if manager.read(cx).is_editing() {
            return;
        }
        self.active_draft = Some(item.id());
        self.items.push(TimelineItem::new(item.clone(), cx));
        self.items
            .sort_by_key(|entry| entry.item.start().map(|start| start.timestamp()));
        self.focus_handle.focus(window, cx);
        manager.update(cx, |manager, cx| {
            manager.begin_view_draft(&item, DraftView::Timeline, cursor, window, cx);
        });
        cx.notify();
    }

    pub(in crate::views::main_view::timeline_view) fn continue_batch(
        &mut self,
        item: &AnyItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_draft.take_if(|id| *id == item.id()).is_none() {
            return;
        }
        let settings = Settings::global(cx);
        let store = AppDatabaseStore::global(cx);
        let context = store.read(cx).pipeline(&settings);
        let Some((draft, cursor)) = next_batch_draft(item, &context) else {
            return;
        };
        let Some(start) = draft
            .start_datetime()
            .map(|time| time.with_timezone(&Local))
        else {
            return;
        };
        self.scroll_to(start, cx);
        self.edit_draft(draft, cursor, window, cx);
    }

    pub(crate) fn add_draft_marker(
        &mut self,
        time: DateTime<Local>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let marker = Marker::new("", time.date_naive());
        let any_item = AnyItem::Marker(marker.clone());
        self.marker_panel_draft = Some(marker.id);
        self.marker_panel_scroll
            .set_offset(gpui::point(px(0.), px(0.)));
        self.draft_markers.push(marker);
        self.marker_panel_date = Some(time.date_naive());
        self.invalidate_annotations();
        ItemManager::global(cx).update(cx, |h, cx| {
            h.begin_edit(&any_item, true, window, cx);
        });
        cx.notify();
    }

    pub(crate) fn add_draft_signal(
        &mut self,
        time: DateTime<Local>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let signal = Signal::new("", time.with_timezone(&chrono::Utc));
        let any_item = AnyItem::Signal(signal.clone());
        self.signal_focus_handles
            .insert(signal.id, cx.focus_handle());
        self.draft_signals.push(signal);
        self.invalidate_annotations();
        ItemManager::global(cx).update(cx, |h, cx| {
            h.begin_edit(&any_item, true, window, cx);
        });
        cx.notify();
    }

    fn navigation_entries(&self, cx: &App, excluded: Option<Uuid>) -> Vec<NavigationEntry> {
        let viewport_height = self
            .bounds
            .map(|bounds| bounds.size.height)
            .unwrap_or_default();
        let scroll_y = self.scroll_offset + self.center_relative().y;
        let is_mounted = |top: Pixels, bottom: Pixels, margin| {
            let top = top + scroll_y;
            let bottom = bottom + scroll_y;
            bottom + margin >= px(0.) && top - margin <= viewport_height
        };
        let independently_drawn: HashSet<usize> = self
            .layout_slots(cx)
            .into_iter()
            .filter_map(|slot| match slot {
                TimelineSlot::Item { index, .. } => Some(index),
                TimelineSlot::Bin(_) | TimelineSlot::Drop { .. } => None,
            })
            .collect();
        let item_manager = ItemManager::global(cx);
        let item_manager = item_manager.read(cx);

        let mut entries: Vec<NavigationEntry> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                excluded != Some(entry.item.id())
                    && entry.transition_state == TransitionState::Attached
                    && !item_manager.is_completing(entry.item.id())
            })
            .filter_map(|(index, entry)| {
                let (start, duration) = item_timeline_span(&entry.item)?;
                let bounds = self.item_bounds(&entry.item, start, duration, Lane::FULL);
                Some(NavigationEntry {
                    id: entry.item.id(),
                    focus_handle: entry.focus_handle.clone(),
                    start,
                    visual_top: bounds.top(),
                    visual_bottom: bounds.bottom(),
                    mounted: self.expanded_bin_contains(entry.item.id())
                        || (independently_drawn.contains(&index)
                            && !self
                                .active_resize
                                .as_ref()
                                .is_some_and(|resize| resize.item_id == entry.item.id())
                            && (self.item_details.is_open(entry.item.id())
                                || is_mounted(bounds.top(), bounds.bottom(), EDGE_HORIZON))),
                })
            })
            .collect();

        let signal_layout = self.signal_layout(cx);
        entries.extend(
            self.known_signals()
                .filter(|signal| excluded != Some(signal.id))
                .filter_map(|signal| {
                    let focus_handle = self.signal_focus_handles.get(&signal.id)?.clone();
                    let start = signal.datetime.with_timezone(&Local);
                    let layout = signal_layout
                        .iter()
                        .find(|entry| !entry.projection && entry.signal.id == signal.id);
                    let height = self.signal_details.height(signal.id, SIGNAL_CARD_HEIGHT);
                    let top =
                        layout.map_or(self.time_to_offset(start) - height / 2., |entry| entry.top);
                    let bottom = layout.map_or(top + height, |entry| entry.top + entry.height);
                    Some(NavigationEntry {
                        id: signal.id,
                        focus_handle,
                        start,
                        visual_top: top,
                        visual_bottom: bottom,
                        mounted: layout.is_some() && is_mounted(top, bottom, px(0.)),
                    })
                }),
        );

        entries.sort_by_key(|entry| (entry.start, entry.id));
        entries
    }

    pub(in crate::views::main_view::timeline_view) fn navigation_handoffs(
        &self,
        cx: &App,
    ) -> NavigationHandoffs {
        let entries: Vec<_> = self
            .navigation_entries(cx, None)
            .into_iter()
            .filter(|entry| entry.mounted)
            .collect();
        entries
            .iter()
            .enumerate()
            .map(|(position, entry)| {
                let arriving = entries
                    .get(position + 1)
                    .or_else(|| position.checked_sub(1).and_then(|prev| entries.get(prev)))
                    .map(|next| (next.id, next.focus_handle.clone()));
                (entry.id, arriving)
            })
            .collect()
    }

    pub(in crate::views::main_view::timeline_view) fn navigate_items(
        &mut self,
        direction: ItemNavigation,
        extend_selection: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let item_manager = ItemManager::global(cx);
        let discarded_draft = if item_manager.read(cx).is_editing() {
            item_manager.update(cx, |manager, cx| manager.commit_open_edit(window, cx))
        } else {
            None
        };

        let entries = self.navigation_entries(cx, discarded_draft);
        if entries.is_empty() {
            return false;
        }
        let current = self
            .pending_item_focus
            .and_then(|pending| entries.iter().position(|entry| entry.id == pending))
            .or_else(|| {
                entries
                    .iter()
                    .position(|entry| entry.focus_handle.contains_focused(window, cx))
            });
        let target = match current {
            Some(position) => adjacent_index(entries.len(), position, direction),
            None => match self.bin_navigation_target(direction, window, cx) {
                Some(id) => entries.iter().position(|entry| entry.id == id),
                None => {
                    let centre = self.scroll_position();
                    let positions: Vec<_> = entries.iter().map(|entry| entry.start).collect();
                    entry_index(&positions, &centre, direction)
                }
            },
        };
        let Some(target) = target.and_then(|index| entries.get(index)).cloned() else {
            return false;
        };
        let origin = current
            .and_then(|index| entries.get(index))
            .map(|entry| entry.id)
            .unwrap_or(target.id);
        let order = SelectionOrder::new(
            SelectionScope::Timeline,
            entries.iter().map(|entry| entry.id),
        );

        if self.reveal_binned_item(target.id, cx) {
            self.pending_item_focus = Some(target.id);
            SelectionManager::global(cx).update(cx, |selection, cx| {
                if extend_selection {
                    selection.select_keyboard_range(&order, origin, target.id, cx);
                } else {
                    selection.select_only(SelectionScope::Timeline, target.id, cx);
                }
            });
            cx.notify();
            return true;
        }

        let scroll_y = self.scroll_offset + self.center_relative().y;
        let top = target.visual_top + scroll_y;
        let bottom = target.visual_bottom + scroll_y;
        let visible_top = TOP_EDGE_INSET;
        let visible_bottom = self
            .bounds
            .map(|bounds| bounds.size.height)
            .unwrap_or_default();
        if bottom <= visible_top || top >= visible_bottom {
            let target_time = self.now
                + visual_duration_at(
                    self.pixel_duration,
                    (target.visual_top + target.visual_bottom) / 2.,
                );
            self.scroll_to(target_time, cx);
        }

        if target.mounted {
            self.pending_item_focus = None;
            if extend_selection {
                focus_item_extending(&order, origin, target.id, &target.focus_handle, window, cx);
            } else {
                focus_item(
                    SelectionScope::Timeline,
                    target.id,
                    &target.focus_handle,
                    window,
                    cx,
                );
            }
        } else {
            self.pending_item_focus = Some(target.id);
            SelectionManager::global(cx).update(cx, |selection, cx| {
                if extend_selection {
                    selection.select_keyboard_range(&order, origin, target.id, cx);
                } else {
                    selection.select_only(SelectionScope::Timeline, target.id, cx);
                }
            });
        }
        cx.notify();
        true
    }

    pub(super) fn begin_complete_item(
        &mut self,
        action_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next_focus = self.navigation_handoffs(cx).remove(&action_id).flatten();
        let Some(item) = self
            .items
            .iter_mut()
            .find(|item| item.item.id() == action_id)
            .ok_or(anyhow!("No item found with id {action_id}"))
            .log_err()
        else {
            return;
        };
        item.transition_state = TransitionState::Completing;
        FocusHandoff::new(Some(SelectionScope::Timeline), action_id, next_focus).take(window, cx);
        cx.notify();
        cx.spawn(async move |this, cx: &mut AsyncApp| {
            cx.background_executor()
                .timer(COMPLETE_CHECKBOX_DURATION)
                .await;
            let _ = this.update(cx, |_view, cx| {
                let store = AppDatabaseStore::global(cx);
                store.update(cx, |store, cx| {
                    store.complete_action(action_id, cx);
                });
            });
        })
        .detach();
    }
}
fn draft_action(time: DateTime<Local>, duration: Option<chrono::Duration>) -> Action {
    Action::new("")
        .with_queued(true)
        .with_start(Some(SchedulePoint::DateTime(
            time.with_timezone(&chrono::Utc),
        )))
        .with_duration(duration.map(Into::into))
}
