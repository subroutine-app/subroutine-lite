use std::collections::HashMap;

use gpui::{
    AnyElement, App, Context, ElementId, FocusHandle, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, MousePressureEvent, ParentElement, Pixels, Styled, WeakEntity, Window, div,
    prelude::FluentBuilder as _,
};
use subroutine_core::AnyItem;
use uuid::Uuid;

use crate::{
    components::{
        DynamicListCardStates, DynamicListDelegate, DynamicListState, ItemCard, Label,
        SIDEBAR_ITEM_HEIGHT,
    },
    selection::SelectionOrder,
};

use super::CalendarView;

#[derive(Clone)]
pub(super) struct CalendarInspectorEntry {
    pub(super) item: AnyItem,
    pub(super) projected: bool,
}

#[derive(Clone)]
pub(super) struct CalendarInspectorDelegate {
    view: WeakEntity<CalendarView>,
    entries: Vec<CalendarInspectorEntry>,
    cards: DynamicListCardStates,
    order: SelectionOrder,
    focus_handles: HashMap<Uuid, FocusHandle>,
}

impl CalendarInspectorDelegate {
    pub(super) fn new(view: WeakEntity<CalendarView>, cx: &App) -> Self {
        let mut cards = DynamicListCardStates::default();
        cards.set_generation(
            crate::stores::AppDatabaseStore::global(cx)
                .read(cx)
                .workspace_generation(),
        );
        Self {
            view,
            entries: Vec::new(),
            cards,
            order: SelectionOrder::new(crate::selection::SelectionScope::Calendar, []),
            focus_handles: HashMap::new(),
        }
    }

    pub(super) fn replace(
        &mut self,
        entries: Vec<CalendarInspectorEntry>,
        generation: u64,
        order: SelectionOrder,
        focus_handles: HashMap<Uuid, FocusHandle>,
    ) {
        self.cards.set_generation(generation);
        self.entries = entries;
        self.cards.retain(self.entries.iter().filter_map(|entry| {
            entry
                .item
                .content()
                .filter(|content| !content.trim().is_empty())
                .map(|_| entry.item.id())
        }));
        self.order = order;
        self.focus_handles = focus_handles;
    }

    pub(super) fn workspace_generation(&self) -> u64 {
        self.cards.generation()
    }

    pub(super) fn order(&self) -> &SelectionOrder {
        &self.order
    }

    pub(super) fn index_of(&self, id: Uuid) -> Option<usize> {
        self.entries.iter().position(|entry| entry.item.id() == id)
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.cards.retain([]);
        self.order = SelectionOrder::new(crate::selection::SelectionScope::Calendar, []);
        self.focus_handles.clear();
    }
}

impl DynamicListDelegate for CalendarInspectorDelegate {
    type Item = AnyElement;

    fn items_count(&self, _cx: &App) -> usize {
        self.entries.len()
    }

    fn item_id(&self, ix: usize, _cx: &App) -> ElementId {
        self.entries
            .get(ix)
            .map(|entry| ElementId::Uuid(entry.item.id()))
            .unwrap_or_else(|| ElementId::Integer(ix as u64))
    }

    fn layout_epoch(&self) -> u64 {
        self.cards.generation()
    }

    fn prepare_layout(&mut self, window: &mut Window, cx: &mut App) {
        self.cards.animate(SIDEBAR_ITEM_HEIGHT, window, cx);
    }

    fn pause_layout(&mut self) {
        self.cards.pause();
    }

    fn item_height(&self, ix: usize, _cx: &App) -> Pixels {
        self.entries.get(ix).map_or(SIDEBAR_ITEM_HEIGHT, |entry| {
            self.cards.height(entry.item.id(), SIDEBAR_ITEM_HEIGHT)
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
        let entry = self.entries.get(ix)?.clone();
        let item = entry.item;
        let item_id = item.id();
        let pressure_item = item.clone();
        let pressed_item = item.clone();
        let pressure_view = self.view.clone();
        let pressed_view = self.view.clone();
        let focus_handle = self.focus_handles.get(&item_id).cloned();
        let order = self.order.clone();
        let meta = super::super::format_item_meta(&item);

        Some(
            ItemCard::new_with_id(
                ("calendar-inspector-item", item.id_u64()),
                &item,
                meta,
                window,
                cx,
            )
            .details(self.cards.get(item_id))
            .when_some(focus_handle, |card, handle| {
                card.with_focus_handle(handle).selectable(order)
            })
            .size_full()
            .border(false)
            .actionable(!entry.projected)
            .when(
                entry.projected
                    && matches!(&item, AnyItem::Event(event)
                    if event.source_provider.is_none() && event.recurrence.is_some()),
                |card| {
                    card.projected_event_availability(crate::stores::AppDatabaseStore::global(cx))
                },
            )
            .draggable(!entry.projected, None)
            .when(entry.projected, |card| card.opacity(0.55))
            .block_mouse_except_scroll()
            .on_mouse_pressure(move |event: &MousePressureEvent, _, cx| {
                pressure_view
                    .update(cx, |view, cx| {
                        view.pressure_inspector_item(pressure_item.clone(), event, cx)
                    })
                    .ok();
            })
            .on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, _, cx| {
                pressed_view
                    .update(cx, |view, cx| {
                        view.begin_inspector_item_press(pressed_item.clone(), event, cx)
                    })
                    .ok();
            })
            .into_any_element(),
        )
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<DynamicListState<Self>>,
    ) -> impl IntoElement {
        div()
            .size_full()
            .items_center()
            .justify_center()
            .child(Label::new("No items on these dates").text_sm())
    }
}
