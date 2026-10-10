use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, ElementId, Entity, FocusHandle,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseExitEvent, MouseMoveEvent,
    ParentElement, Pixels, Styled, Window, canvas, div, prelude::FluentBuilder as _, px,
};
use subroutine_core::AnyItem;
use uuid::Uuid;

use crate::{
    components::{
        DynamicListCardStates, DynamicListDelegate, DynamicListState, ItemCard,
        ext::ElementExt as _,
        menu::{MenuBuilder, open_context_menu, open_context_menu_at_bounds},
    },
    selection::{
        CompleteSelected, ITEM_CARD_KEY_CONTEXT, OpenItemContextMenu, SelectionManager,
        SelectionOrder, SelectionScope, focus_item, focus_item_extending,
    },
    views::OpenItemInspector,
};

use super::super::{format_item_meta, queue_system::QueueLane};

pub(super) const CARD_HEIGHT: Pixels = px(76.);

pub(super) const LIST_PADDING: Pixels = px(12.);

pub(super) struct QueueColumn {
    pub lane: QueueLane,
    pub list: Entity<DynamicListState<QueueDelegate>>,
    mouse_exited: Rc<Cell<bool>>,
}

impl QueueColumn {
    pub(super) fn new(lane: QueueLane, window: &mut Window, cx: &mut App) -> Self {
        let list = cx.new(|cx| {
            DynamicListState::new(QueueDelegate::new(), window, cx)
                .gap(px(0.))
                .content_padding(LIST_PADDING)
                .scrollbar_visible(false)
        });
        Self {
            lane,
            list,
            mouse_exited: Rc::new(Cell::new(false)),
        }
    }

    pub(super) fn hover_observer(&self) -> impl IntoElement {
        let list = self.list.clone();
        let mouse_exited = self.mouse_exited.clone();
        canvas(
            |bounds, window, _| bounds.intersect(&window.content_mask().bounds),
            move |_, bounds, window, cx| {
                let visible = !mouse_exited.get()
                    && window.is_window_hovered()
                    && bounds.contains(&window.mouse_position());
                let reconciled_list = list.clone();
                cx.defer(move |cx| {
                    reconciled_list.update(cx, |list, cx| list.set_scrollbar_visible(visible, cx));
                });

                let moved_list = list.clone();
                let moved_exited = mouse_exited.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase.capture() {
                        moved_exited.set(false);
                        moved_list.update(cx, |list, cx| {
                            list.set_scrollbar_visible(bounds.contains(&event.position), cx);
                        });
                    }
                });
                window.on_mouse_event(move |_: &MouseExitEvent, phase, _, cx| {
                    if phase.capture() {
                        // GPUI retains the last mouse position after leaving the window.
                        mouse_exited.set(true);
                        list.update(cx, |list, cx| list.set_scrollbar_visible(false, cx));
                    }
                });
            },
        )
        .absolute()
        .inset_0()
    }

    pub(super) fn update(
        &mut self,
        items: Vec<AnyItem>,
        focus_handles: &HashMap<Uuid, FocusHandle>,
        order: SelectionOrder,
        blocked: bool,
        generation: u64,
        cx: &mut App,
    ) {
        let delegate = self.list.read(cx).delegate();
        let generation_changed = delegate.cards.generation() != generation;
        let items_changed = !same_candidates(&delegate.items, &items);
        let current_order = delegate.order();
        if !generation_changed
            && !items_changed
            && delegate.blocked == blocked
            && current_order.scope == order.scope
            && current_order.ids() == order.ids()
        {
            return;
        }

        self.list.update(cx, |list, cx| {
            if generation_changed {
                list.reset_items(cx, |delegate, _| {
                    delegate.replace(items, focus_handles, order, blocked, generation);
                });
            } else if items_changed {
                list.update_items(cx, |delegate, _| {
                    delegate.replace(items, focus_handles, order, blocked, generation);
                });
            } else {
                let delegate = list.delegate_mut();
                *delegate.order.borrow_mut() = order;
                delegate.blocked = blocked;
                cx.notify();
            }
        });
    }

    pub(super) fn len(&self, cx: &App) -> usize {
        self.list.read(cx).delegate().items.len()
    }

    pub(super) fn priority_id(&self, cx: &App) -> Option<Uuid> {
        self.list.read(cx).delegate().items.last().map(AnyItem::id)
    }

    pub(super) fn suppress_departure(&self, id: Uuid, cx: &mut App) {
        self.list.update(cx, |list, _| {
            list.delegate_mut().suppressed = Some(id);
        });
    }

    pub(super) fn is_at_bottom(&self, cx: &App) -> bool {
        let list = self.list.read(cx);
        let scroll = list.scroll_handle();
        let max_offset = (list.content_height() - scroll.bounds().size.height).max(px(0.));
        scroll.offset().y <= -max_offset + px(1.)
    }

    pub(super) fn reveal_item(&self, id: Uuid, cx: &mut App) {
        let index = self
            .list
            .read(cx)
            .delegate()
            .items
            .iter()
            .position(|item| item.id() == id);
        if let Some(index) = index {
            self.list
                .update(cx, |list, cx| list.reveal_item_for_focus(index, cx));
        }
    }

    pub(super) fn scroll_to_priority(&self, window: &Window, cx: &mut App) {
        let delegate = self.list.read(cx).delegate();
        let target = delegate
            .items
            .iter()
            .position(|item| {
                delegate
                    .focus_handles
                    .get(&item.id())
                    .is_some_and(|handle| handle.contains_focused(window, cx))
            })
            .or_else(|| delegate.items.len().checked_sub(1));
        if let Some(index) = target {
            self.list.update(cx, |list, cx| {
                list.scroll_item_into_view(index, cx);
            });
        }
    }
}

#[derive(Clone)]
pub(super) struct QueueDelegate {
    items: Vec<AnyItem>,
    cards: DynamicListCardStates,
    focus_handles: HashMap<Uuid, FocusHandle>,
    // Departure delegates must not report their old selection order.
    order: Rc<RefCell<SelectionOrder>>,
    blocked: bool,
    suppressed: Option<Uuid>,
}

impl QueueDelegate {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            cards: DynamicListCardStates::default(),
            focus_handles: HashMap::new(),
            order: Rc::new(RefCell::new(SelectionOrder::new(SelectionScope::Queue, []))),
            blocked: false,
            suppressed: None,
        }
    }

    fn replace(
        &mut self,
        items: Vec<AnyItem>,
        focus_handles: &HashMap<Uuid, FocusHandle>,
        order: SelectionOrder,
        blocked: bool,
        generation: u64,
    ) {
        self.cards.set_generation(generation);
        self.items = items;
        *self.order.borrow_mut() = order;
        self.blocked = blocked;
        self.suppressed = None;
        self.cards.retain(self.items.iter().filter_map(|item| {
            item.content()
                .filter(|content| !content.trim().is_empty())
                .map(|_| item.id())
        }));
        self.focus_handles = self
            .items
            .iter()
            .filter_map(|item| {
                focus_handles
                    .get(&item.id())
                    .cloned()
                    .map(|handle| (item.id(), handle))
            })
            .collect();
    }

    fn order(&self) -> SelectionOrder {
        self.order.borrow().clone()
    }
}

impl DynamicListDelegate for QueueDelegate {
    type Item = AnyElement;

    fn items_count(&self, _cx: &App) -> usize {
        self.items.len()
    }

    fn item_id(&self, ix: usize, _cx: &App) -> ElementId {
        self.items
            .get(ix)
            .map(|item| ElementId::Uuid(item.id()))
            .unwrap_or_else(|| ElementId::Integer(ix as u64))
    }

    fn layout_epoch(&self) -> u64 {
        self.cards.generation()
    }

    fn prepare_layout(&mut self, window: &mut Window, cx: &mut App) {
        self.cards.animate(CARD_HEIGHT, window, cx);
    }

    fn pause_layout(&mut self) {
        self.cards.pause();
    }

    fn item_height(&self, ix: usize, _cx: &App) -> Pixels {
        self.items.get(ix).map_or(CARD_HEIGHT, |item| {
            self.cards.height(item.id(), CARD_HEIGHT)
        })
    }

    fn can_drag(&self, _ix: usize, _cx: &App) -> bool {
        false
    }

    fn move_item(
        &mut self,
        _from: usize,
        _to: usize,
        _window: &mut Window,
        _cx: &mut Context<DynamicListState<Self>>,
    ) {
    }

    fn render_item(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<DynamicListState<Self>>,
    ) -> Option<Self::Item> {
        let item = self.items.get(ix)?;
        let id = item.id();
        if self.suppressed == Some(id) {
            return None;
        }
        let handle = self.focus_handles.get(&id)?.clone();
        let card = ItemCard::new_with_id(
            ElementId::Uuid(id),
            item,
            format_item_meta(item),
            window,
            cx,
        )
        .neighbors(
            ix.checked_sub(1)
                .and_then(|ix| self.items.get(ix))
                .map(AnyItem::id)
                .filter(|id| self.suppressed != Some(*id)),
            self.items
                .get(ix + 1)
                .map(AnyItem::id)
                .filter(|id| self.suppressed != Some(*id)),
        )
        .details(self.cards.get(id))
        .schedule_navigation()
        .with_focus_handle(handle)
        .when(!self.blocked, |card| card.selectable(self.order()))
        .size_full()
        .editable(false)
        .actionable(!self.blocked)
        .tab_stop(true)
        .draggable(true, None)
        .block_mouse_except_scroll()
        .on_key_down(cx.listener(move |list, event: &KeyDownEvent, window, cx| {
            if event.is_held {
                return;
            }
            let key = event.keystroke.key.as_str();
            let offset = match key {
                "up" | "k" => -1,
                "down" | "j" => 1,
                _ => return,
            };
            cx.stop_propagation();
            let extend = event.keystroke.modifiers.shift && matches!(key, "up" | "down");
            focus_sibling(list, id, offset, extend, window, cx);
        }));

        Some(
            div()
                .size_full()
                .when(self.blocked, |row| {
                    let bounds = Rc::new(Cell::new(None::<Bounds<Pixels>>));
                    let measured = bounds.clone();
                    // Non-actionable cards suppress their own menus and overwrite their opacity.
                    row.opacity(0.55)
                        .key_context(ITEM_CARD_KEY_CONTEXT)
                        .on_prepaint(move |bounds, _, _| measured.set(Some(bounds)))
                        .capture_any_mouse_down(move |event, window, cx| {
                            if event.button == MouseButton::Right {
                                cx.stop_propagation();
                                let menu = inspection_menu(id, cx);
                                open_context_menu(menu, event.position, window, cx);
                            }
                        })
                        .on_action(move |_: &OpenItemContextMenu, window, cx| {
                            let Some(bounds) = bounds.get() else {
                                cx.propagate();
                                return;
                            };
                            let menu = inspection_menu(id, cx);
                            open_context_menu_at_bounds(menu, bounds, window, cx);
                        })
                        .capture_action::<CompleteSelected>(|_, _, cx| cx.stop_propagation())
                })
                .child(card)
                .into_any_element(),
        )
    }
}

fn focus_sibling(
    list: &mut DynamicListState<QueueDelegate>,
    id: Uuid,
    offset: isize,
    extend: bool,
    window: &mut Window,
    cx: &mut Context<DynamicListState<QueueDelegate>>,
) {
    let delegate = list.delegate();
    let Some(index) = delegate.items.iter().position(|item| item.id() == id) else {
        return;
    };
    let Some(next) = index.checked_add_signed(offset) else {
        return;
    };
    let Some(next_id) = delegate.items.get(next).map(AnyItem::id) else {
        return;
    };
    let Some(handle) = delegate.focus_handles.get(&next_id) else {
        return;
    };
    if delegate.blocked {
        handle.focus(window, cx);
    } else if extend {
        focus_item_extending(&delegate.order(), id, next_id, handle, window, cx);
    } else {
        focus_item(SelectionScope::Queue, next_id, handle, window, cx);
    }
    list.scroll_item_into_view(next, cx);
}

fn inspection_menu(id: Uuid, cx: &mut App) -> MenuBuilder {
    SelectionManager::claim_press(cx);
    MenuBuilder::new().item("Inspect item", move |window, cx| {
        window.dispatch_action(Box::new(OpenItemInspector(id, None)), cx);
    })
}

fn same_candidates(previous: &[AnyItem], next: &[AnyItem]) -> bool {
    previous.len() == next.len()
        && previous.iter().zip(next).all(|(previous, next)| {
            let (AnyItem::Action(previous), AnyItem::Action(next)) = (previous, next) else {
                return false;
            };
            previous.id == next.id
                && previous.recurrence_id == next.recurrence_id
                && previous.routine_id == next.routine_id
                && previous.template_id == next.template_id
                && previous.title == next.title
                && previous.content == next.content
                && previous.queued == next.queued
                && previous.pinned == next.pinned
                && previous.start == next.start
                && previous.duration == next.duration
                && previous.completion == next.completion
                && previous.recurrence == next.recurrence
                && previous.source_provider == next.source_provider
                && previous.source_external_id == next.source_external_id
        })
}
