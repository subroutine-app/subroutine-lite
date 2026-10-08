use std::{
    collections::{HashMap, HashSet},
    time::Duration as StdDuration,
};

use crate::components::menu::MenuBuilder;

use crate::components::virtual_list::{VirtualListScrollHandle, v_virtual_list};
use crate::item_manager::{DiscardDraft, DraftSubmitted, DraftView, ItemManager, next_batch_draft};
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveTime, TimeZone, Utc};
use chronoutil::RelativeDuration;
use gpui::{
    App, AsyncApp, Bounds, Context, DragMoveEvent, FocusHandle, Focusable, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, ParentElement, Pixels, Point, Render, ScrollStrategy,
    Styled, Task, Window, div, point, prelude::FluentBuilder as _, px,
};
use gpui_kit::foundation::StyledExt as _;
use subroutine_core::{Action, AnyItem, Event, ItemType, Marker, SchedulePoint, Signal};
use uuid::Uuid;

mod agenda;
mod rows;
mod scroll;
mod sections;
use agenda::*;
use scroll::RowAnchor;
use sections::{QueueSection, QueueSections};

use crate::{
    components::{
        DragData, DraggedItems, EmptyState, ItemCardStates, MarqueeSelection, MarqueeView, marquee,
    },
    icons::{AppIcon, Icon},
    selection::{
        SelectionManager, SelectionOrder, SelectionScope, focus_item, focus_item_extending,
    },
    settings::{Settings, TimelineCreationKind},
    stores::AppDatabaseStore,
    views::{
        GoToNow, LIST_VIEW_MAX_WIDTH, LIST_VIEW_MIN_WIDTH, RefreshPipeline, StartItemCreator,
        StartItemCreatorOnDate, StartQueuedActionCreator,
        drop_confirmation::{confirm_drop, resolve_dragged, scheduled_item_count},
    },
};

use super::{
    action_start_on_date, local_time_on,
    tab::{MainViewTab, SelectedMainView},
};

fn queue_context_menu(date: Option<NaiveDate>) -> MenuBuilder {
    MenuBuilder::new()
        .when_some(date, |menu, date| {
            menu.label(date.format("%A, %B %-d").to_string())
        })
        .item("New queued action", move |window, cx| {
            window.dispatch_action(Box::new(StartQueuedActionCreator(date)), cx);
        })
        .when(date.is_none(), |menu| {
            menu.item_with_keybinding("New item…", StartItemCreator, |window, cx| {
                window.dispatch_action(Box::new(StartItemCreator), cx);
            })
        })
        .when_some(date, |menu, date| {
            menu.submenu("New item", move |menu| {
                menu.item("Event", move |window, cx| {
                    window.dispatch_action(
                        Box::new(StartItemCreatorOnDate(ItemType::Event, date)),
                        cx,
                    );
                })
                .item("Routine", move |window, cx| {
                    window.dispatch_action(
                        Box::new(StartItemCreatorOnDate(ItemType::Routine, date)),
                        cx,
                    );
                })
                .item("Marker", move |window, cx| {
                    window.dispatch_action(
                        Box::new(StartItemCreatorOnDate(ItemType::Marker, date)),
                        cx,
                    );
                })
                .item("Signal", move |window, cx| {
                    window.dispatch_action(
                        Box::new(StartItemCreatorOnDate(ItemType::Signal, date)),
                        cx,
                    );
                })
            })
        })
        .separator()
        .item_with_keybinding("Go to today", GoToNow, |window, cx| {
            window.dispatch_action(Box::new(GoToNow), cx);
        })
        .item_with_keybinding("Refresh queue", RefreshPipeline, |window, cx| {
            window.dispatch_action(Box::new(RefreshPipeline), cx);
        })
}
const AUTOSCROLL_REFRESH_RATE: f64 = 25.0;
const EDGE_SCROLL_ZONE: Pixels = px(96.);
const EDGE_SCROLL_MAX_SPEED: f32 = 22.;

fn edge_scroll_speed(local_y: Pixels, height: Pixels) -> Option<Pixels> {
    if local_y < EDGE_SCROLL_ZONE {
        let t = 1. - (local_y / EDGE_SCROLL_ZONE).clamp(0., 1.);
        Some(px(t * t * EDGE_SCROLL_MAX_SPEED))
    } else if local_y > height - EDGE_SCROLL_ZONE {
        let t = 1. - ((height - local_y) / EDGE_SCROLL_ZONE).clamp(0., 1.);
        Some(px(-(t * t * EDGE_SCROLL_MAX_SPEED)))
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum QueueDropTarget {
    Unscheduled,
    Day(NaiveDate),
    OnDayItem {
        date: NaiveDate,
        item: Uuid,
    },
    Before {
        date: NaiveDate,
        item: Uuid,
        anchor: Option<DateTime<Utc>>,
    },
    After {
        date: NaiveDate,
        item: Uuid,
        anchor: Option<DateTime<Utc>>,
    },
}

fn queue_item_drop_target(
    item: &AnyItem,
    bounds: Bounds<Pixels>,
    position: Point<Pixels>,
    dragged: &DraggedItems,
    cx: &App,
) -> Option<QueueDropTarget> {
    let id = item.id();
    if !bounds.contains(&position)
        || dragged.items.is_empty()
        || dragged.items.iter().any(|dragged| dragged.id() == id)
    {
        return None;
    }

    let date = item.start_date()?;
    if dragged.source_anchor.is_some()
        || dragged
            .items
            .iter()
            .any(|item| matches!(item, AnyItem::Marker(_) | AnyItem::Routine(_)))
    {
        return Some(QueueDropTarget::OnDayItem { date, item: id });
    }

    let Some(anchor) = item.start().and_then(|start| match start {
        SchedulePoint::DateTime(start) => Some(start),
        SchedulePoint::Date(_) => None,
    }) else {
        return Some(QueueDropTarget::OnDayItem { date, item: id });
    };
    let duration = item.duration().or_else(|| {
        matches!(item, AnyItem::Action(_))
            .then_some(Settings::global(cx).schedule.default_action_duration)
    });
    let after_anchor = duration
        .map(|duration| (anchor.with_timezone(&Local) + duration).with_timezone(&Utc))
        .unwrap_or(anchor + Duration::minutes(1));
    let before = position.y < bounds.origin.y + bounds.size.height / 2.;
    Some(if before {
        QueueDropTarget::Before {
            date,
            item: id,
            anchor: Some(anchor),
        }
    } else {
        QueueDropTarget::After {
            date,
            item: id,
            anchor: Some(after_anchor),
        }
    })
}

#[derive(Clone, Copy)]
enum QueueReveal {
    Item { id: Uuid, section: QueueSection },
    Day(NaiveDate),
}

pub struct QueueView {
    pub(super) focus_handle: FocusHandle,
    scroll_handle: VirtualListScrollHandle,
    items: Vec<AnyItem>,
    draft_items: Vec<AnyItem>,
    active_draft: Option<Uuid>,
    reveal: Option<QueueReveal>,
    markers: Vec<Marker>,
    signals: Vec<Signal>,
    agenda: Agenda,
    sections: QueueSections,
    details: ItemCardStates,
    workspace_generation: u64,
    layout: AgendaLayout,
    agenda_stale: bool,
    today: NaiveDate,
    empty_day: Option<NaiveDate>,

    order: SelectionOrder,
    item_focus_handles: HashMap<Uuid, FocusHandle>,
    drop_target: Option<QueueDropTarget>,
    dragged_items: Option<DraggedItems>,
    marquee: MarqueeSelection,
    edge_scroll_speed: Option<Pixels>,
    edge_scroll_task: Option<Task<()>>,
}

impl QueueView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = AppDatabaseStore::global(cx);
        cx.observe(&store, |view, store, cx| {
            view.sync_workspace(store.read(cx).workspace_generation(), cx);
        })
        .detach();
        let item_manager = ItemManager::global(cx);
        cx.observe(&item_manager, |view, manager, cx| {
            if let Some(item) = view
                .active_draft
                .and_then(|id| manager.read(cx).draft_item(id))
                && let Some(draft) = view
                    .draft_items
                    .iter_mut()
                    .find(|draft| draft.id() == item.id())
                && draft.item_type() != item.item_type()
            {
                let id = item.id();
                let section = QueueSection::for_item(item, Local::now().date_naive());
                *draft = item.clone();
                view.sections.reveal(section);
                view.reveal = Some(QueueReveal::Item { id, section });
                view.invalidate(cx);
            } else {
                cx.notify();
            }
        })
        .detach();
        cx.subscribe(&item_manager, |view, _, event: &DiscardDraft, cx| {
            view.draft_items.retain(|item| item.id() != event.0);
            if view.active_draft == Some(event.0) {
                view.active_draft = None;
            }
            view.invalidate(cx);
        })
        .detach();

        cx.subscribe_in(
            &item_manager,
            window,
            |view, _, event: &DraftSubmitted, window, cx| {
                if event.view == DraftView::Queue
                    && view
                        .active_draft
                        .take_if(|id| *id == event.item.id())
                        .is_some()
                {
                    view.sections.reveal(QueueSection::for_item(
                        &event.item,
                        Local::now().date_naive(),
                    ));
                    view.invalidate(cx);
                    let settings = Settings::global(cx);
                    let store = AppDatabaseStore::global(cx);
                    let context = store.read(cx).pipeline(&settings);
                    if let Some((draft, cursor)) = next_batch_draft(&event.item, &context) {
                        view.edit_draft(draft, cursor, window, cx);
                    }
                }
            },
        )
        .detach();

        let mut view = Self {
            focus_handle: cx.focus_handle(),
            scroll_handle: VirtualListScrollHandle::new(),
            items: Vec::new(),
            draft_items: Vec::new(),
            active_draft: None,
            reveal: None,
            markers: Vec::new(),
            signals: Vec::new(),
            agenda: Agenda::default(),
            sections: QueueSections::default(),
            details: ItemCardStates::default(),
            workspace_generation: store.read(cx).workspace_generation(),
            layout: AgendaLayout::default(),
            agenda_stale: false,
            today: Local::now().date_naive(),
            empty_day: None,

            order: SelectionOrder::new(SelectionScope::Queue, []),
            item_focus_handles: HashMap::new(),
            drop_target: None,
            dragged_items: None,
            marquee: MarqueeSelection::new(SelectionScope::Queue),
            edge_scroll_speed: None,
            edge_scroll_task: None,
        };
        view.rebuild(cx);
        view
    }

    fn sync_workspace(&mut self, generation: u64, cx: &mut Context<Self>) {
        if self.workspace_generation == generation {
            return;
        }
        self.workspace_generation = generation;
        self.details = ItemCardStates::default();
        self.draft_items.clear();
        self.active_draft = None;
        self.reveal = None;
        self.sections = QueueSections::default();
        self.empty_day = None;
        self.item_focus_handles.clear();
        self.marquee.end();
        self.clear_drop_target(cx);
        self.scroll_handle = VirtualListScrollHandle::new();
        self.invalidate(cx);
    }

    pub fn refresh_items(&mut self, items: Vec<AnyItem>, cx: &mut Context<Self>) {
        self.draft_items
            .retain(|draft| !items.iter().any(|item| item.id() == draft.id()));
        self.items = items;
        self.invalidate(cx);
    }

    pub fn refresh_markers(&mut self, markers: Vec<Marker>, cx: &mut Context<Self>) {
        self.markers = markers;
        self.invalidate(cx);
    }

    pub fn refresh_signals(&mut self, signals: Vec<Signal>, cx: &mut Context<Self>) {
        self.signals = signals;
        self.invalidate(cx);
    }

    fn open_context_menu(
        &self,
        date: Option<NaiveDate>,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.focus_handle.focus(window, cx);
        crate::components::menu::open_context_menu(
            queue_context_menu(date),
            event.position,
            window,
            cx,
        );
    }

    pub(super) fn add_draft(
        &mut self,
        date: Option<NaiveDate>,
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
        let item = queue_draft(Settings::global(cx).queue_creation, date);
        self.edit_draft(item, None, window, cx);
    }

    fn edit_draft(
        &mut self,
        item: AnyItem,
        cursor: Option<DateTime<Utc>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = item.id();
        self.active_draft = Some(id);
        let section = QueueSection::for_item(&item, Local::now().date_naive());
        self.sections.reveal(section);
        self.reveal = Some(QueueReveal::Item { id, section });
        self.draft_items.push(item.clone());
        self.invalidate(cx);
        self.focus_handle.focus(window, cx);
        ItemManager::global(cx).update(cx, |manager, cx| {
            manager.begin_view_draft(&item, DraftView::Queue, cursor, window, cx);
        });
    }

    pub(super) fn scroll_to_item_or_date(
        &mut self,
        item_id: Uuid,
        date: NaiveDate,
        cx: &mut Context<Self>,
    ) {
        let section = self
            .items
            .iter()
            .chain(&self.draft_items)
            .find(|item| item.id() == item_id)
            .map_or(QueueSection::Day(date), |item| {
                QueueSection::for_item(item, Local::now().date_naive())
            });
        self.sections.reveal(section);
        self.reveal = Some(QueueReveal::Item {
            id: item_id,
            section,
        });
        self.invalidate(cx);
    }

    fn invalidate(&mut self, cx: &mut Context<Self>) {
        self.agenda_stale = true;
        cx.notify();
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        self.agenda_stale = false;
        self.today = Local::now().date_naive();
        let items: Vec<_> = self
            .items
            .iter()
            .chain(self.draft_items.iter())
            .cloned()
            .collect();
        self.agenda = Agenda::build_revealing(
            &items,
            &self.markers,
            &self.signals,
            self.today,
            self.sections.revealed_dates(),
        );

        self.details
            .retain(self.agenda.rows.iter().filter_map(|row| match row {
                QueueRow::Item { item, .. } => Some(item.id()),
                _ => None,
            }));

        let live: HashSet<Uuid> = self
            .agenda
            .rows
            .iter()
            .filter_map(|row| match row {
                QueueRow::Item {
                    item,
                    projected: false,
                    ..
                } => Some(item.id()),
                _ => None,
            })
            .collect();
        self.item_focus_handles.retain(|id, _| live.contains(id));
        for id in live {
            self.item_focus_handles
                .entry(id)
                .or_insert_with(|| cx.focus_handle());
        }
        self.update_layout();
    }

    fn card_height(&self, item: &AnyItem) -> Pixels {
        self.details.height(item.id(), item_card_height(item))
    }

    fn row_height(&self, row: &QueueRow) -> Pixels {
        row.height(|item| self.card_height(item))
    }

    fn row_interactive(&self, ix: usize) -> bool {
        let source = self.layout.rows[ix];
        let row = &self.agenda.rows[source];
        self.sections.is_expanded(row.section())
            && self.layout.sizes[ix].height == self.row_height(row) + row_top_inset(source)
    }

    fn update_layout(&mut self) {
        self.layout = self.agenda.layout(
            |section| self.sections.layout_state(section),
            |item| self.card_height(item),
        );
        self.order = SelectionOrder::new(SelectionScope::Queue, self.layout.order.clone());
    }

    fn item_scroll_strategy(&self, row: usize) -> ScrollStrategy {
        if self.layout.sizes[row].height > self.scroll_handle.bounds().size.height {
            ScrollStrategy::Top
        } else {
            ScrollStrategy::Center
        }
    }

    fn reveal_row(&mut self) -> Option<(usize, ScrollStrategy)> {
        if self.sections.is_animating() {
            return None;
        }
        let (section, strategy) = match self.reveal.take()? {
            QueueReveal::Item { id, section } => {
                if let Some(row) = self.layout.positions.get(&id) {
                    return Some((*row, self.item_scroll_strategy(*row)));
                }
                (section, ScrollStrategy::Center)
            }
            QueueReveal::Day(date) => (QueueSection::Day(date), ScrollStrategy::Top),
        };
        self.layout
            .rows
            .iter()
            .position(|index| {
                let row = &self.agenda.rows[*index];
                row.is_heading() && row.section() == section
            })
            .map(|row| (row, strategy))
    }

    fn toggle_section(
        &mut self,
        section: QueueSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let expanded = !self.sections.is_expanded(section);
        if !expanded && !self.prepare_collapse(&[section], window, cx) {
            return;
        }
        self.sections.set_expanded(section, expanded);
        cx.notify();
    }

    fn set_all_expanded(&mut self, expanded: bool, window: &mut Window, cx: &mut Context<Self>) {
        let sections: Vec<_> = self
            .agenda
            .rows
            .iter()
            .filter(|row| row.is_heading())
            .map(QueueRow::section)
            .collect();
        if expanded {
            for section in sections {
                self.sections.set_expanded(section, true);
            }
        } else {
            if !self.prepare_collapse(&sections, window, cx) {
                return;
            }
            self.sections.collapse_all();
        }
        cx.notify();
    }

    fn prepare_collapse(
        &mut self,
        sections: &[QueueSection],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let hidden: HashSet<_> = self
            .agenda
            .rows
            .iter()
            .filter_map(|row| match row {
                QueueRow::Item {
                    item,
                    projected: false,
                    ..
                } if sections.contains(&row.section()) => Some(item.id()),
                _ => None,
            })
            .collect();
        let manager = ItemManager::global(cx);
        if hidden
            .iter()
            .any(|id| manager.read(cx).is_being_edited(*id))
        {
            manager.update(cx, |manager, cx| manager.commit_open_edit(window, cx));
            if manager.read(cx).is_editing() {
                return false;
            }
        }
        if hidden.iter().any(|id| {
            self.item_focus_handles
                .get(id)
                .is_some_and(|handle| handle.contains_focused(window, cx))
        }) {
            self.focus_handle.focus(window, cx);
        }
        SelectionManager::global(cx).update(cx, |selection, cx| {
            if selection.has_selection_in(SelectionScope::Queue) {
                let remaining = selection
                    .ids()
                    .iter()
                    .copied()
                    .filter(|id| !hidden.contains(id))
                    .collect();
                selection.select_many(SelectionScope::Queue, remaining, cx);
            }
        });
        self.active_draft.take_if(|id| hidden.contains(id));
        self.reveal.take_if(|target| {
            let section = match target {
                QueueReveal::Item { section, .. } => *section,
                QueueReveal::Day(date) => QueueSection::Day(*date),
            };
            sections.contains(&section)
        });
        self.clear_drop_target(cx);
        true
    }

    fn move_cursor_from(
        &mut self,
        id: Uuid,
        delta: i32,
        extend_selection: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        let order = self.layout.order.clone();
        let Some(current) = order.iter().position(|other| *other == id) else {
            return;
        };
        let next = (current as i32 + delta).clamp(0, order.len() as i32 - 1) as usize;
        if next == current {
            return;
        }
        let next = order[next];
        let Some(handle) = self.item_focus_handles.get(&next).cloned() else {
            return;
        };
        if extend_selection {
            focus_item_extending(&self.order, id, next, &handle, window, cx);
        } else {
            focus_item(SelectionScope::Queue, next, &handle, window, cx);
        }
        if let Some(row) = self.layout.positions.get(&next) {
            self.scroll_handle
                .scroll_to_item(*row, self.item_scroll_strategy(*row));
        }
    }

    fn set_edge_scroll_speed(&mut self, speed: Option<Pixels>, cx: &mut Context<Self>) -> bool {
        if self.edge_scroll_speed == speed {
            return false;
        }

        self.edge_scroll_speed = speed;
        if speed.is_some() {
            if self.edge_scroll_task.is_none() {
                let interval = StdDuration::from_secs_f64(1.0 / AUTOSCROLL_REFRESH_RATE);
                self.edge_scroll_task = Some(cx.spawn(async move |view, cx: &mut AsyncApp| {
                    loop {
                        cx.background_executor().timer(interval).await;
                        if view.update(cx, |_, cx| cx.notify()).is_err() {
                            break;
                        }
                    }
                }));
            }
        } else {
            self.edge_scroll_task = None;
        }
        cx.notify();
        true
    }

    fn edge_scroll_speed_at(&self, position: Point<Pixels>) -> Option<Pixels> {
        let bounds = self.scroll_handle.bounds();
        let local = bounds.localize(&position)?;
        edge_scroll_speed(local.y, bounds.size.height)
    }

    fn set_drop_target_option(&mut self, target: Option<QueueDropTarget>, cx: &mut Context<Self>) {
        if self.drop_target != target {
            self.drop_target = target;
            cx.notify();
        }
    }

    pub(super) fn set_drop_target(&mut self, target: QueueDropTarget, cx: &mut Context<Self>) {
        self.set_drop_target_option(Some(target), cx);
    }

    fn drop_target_at(
        &self,
        position: Point<Pixels>,
        dragged: &DraggedItems,
        cx: &App,
    ) -> Option<QueueDropTarget> {
        if dragged.items.is_empty() {
            return None;
        }
        let (ix, mut row_bounds) = self.scroll_handle.row_at_position(position)?;
        let width = row_bounds
            .size
            .width
            .clamp(LIST_VIEW_MIN_WIDTH, LIST_VIEW_MAX_WIDTH);
        row_bounds.origin.x += ((row_bounds.size.width - width) / 2.).max(px(0.));
        row_bounds.size.width = width;
        if !row_bounds.contains(&position) {
            return None;
        }
        let row = self.agenda.rows.get(*self.layout.rows.get(ix)?)?;
        if !row.is_heading() && !self.row_interactive(ix) {
            return None;
        }
        match row {
            QueueRow::Missed => None,
            QueueRow::Day { date, .. } | QueueRow::Create { date: Some(date) } => {
                Some(QueueDropTarget::Day(*date))
            }
            QueueRow::AnyTime | QueueRow::Create { date: None } => dragged
                .actions()
                .next()
                .map(|_| QueueDropTarget::Unscheduled),
            QueueRow::Item { item, .. } => {
                queue_item_drop_target(item, row_bounds, position, dragged, cx)
            }
        }
    }

    fn handle_drag_move(
        &mut self,
        event: &DragMoveEvent<DragData<DraggedItems>>,
        cx: &mut Context<Self>,
    ) {
        self.dragged_items = Some(event.drag(cx).data.clone());
        self.set_drop_target_option(None, cx);
        let speed = self.edge_scroll_speed_at(event.event.position);
        self.set_edge_scroll_speed(speed, cx);
    }

    fn apply_edge_scroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(speed) = self.edge_scroll_speed else {
            return;
        };
        let moved = self.scroll_handle.scroll_by_y(speed);
        if moved == px(0.) {
            self.set_edge_scroll_speed(None, cx);
            return;
        }

        let pointer = window.mouse_position();
        if self.marquee.is_active() {
            self.marquee.scrolled_by(point(px(0.), moved));
            self.marquee.drag_to(pointer, cx);
        } else if let Some(dragged) = self.dragged_items.clone() {
            self.drop_target = self.drop_target_at(pointer, &dragged, cx);
        }
    }

    pub(super) fn clear_drop_target(&mut self, cx: &mut Context<Self>) {
        let target_changed = self.drop_target.take().is_some();
        let was_dragging = self.dragged_items.take().is_some();
        let speed_changed = self.set_edge_scroll_speed(None, cx);
        if (target_changed || was_dragging) && !speed_changed {
            cx.notify();
        }
    }

    pub(super) fn commit_drop(
        &mut self,
        dragged: &DraggedItems,
        target: QueueDropTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dragged = resolve_dragged(dragged, cx);
        let items: Vec<AnyItem> = dragged
            .items
            .iter()
            .filter(|item| match item {
                AnyItem::ActionTemplate(_) | AnyItem::EventTemplate(_) => false,
                _ => target != QueueDropTarget::Unscheduled || matches!(item, AnyItem::Action(_)),
            })
            .cloned()
            .collect();
        let count = scheduled_item_count(&items);
        if count == 0 {
            self.clear_drop_target(cx);
            return;
        }

        let default_duration = Settings::global(cx).schedule.default_action_duration;
        let starts = dropped_starts(&items, dragged.source_anchor, target, default_duration);
        let mut detail = match target {
            QueueDropTarget::Unscheduled => {
                "Move actions to the Unscheduled queue, with no date or time.".to_owned()
            }
            QueueDropTarget::Before {
                anchor: Some(anchor),
                ..
            } => format!(
                "Schedule in the queue immediately before {}. Items are placed consecutively.",
                anchor
                    .with_timezone(&Local)
                    .format("%A, %B %-d, %Y at %H:%M")
            ),
            QueueDropTarget::After {
                anchor: Some(anchor),
                ..
            } => format!(
                "Schedule in the queue starting at {} (after the target item). Items are placed consecutively.",
                anchor
                    .with_timezone(&Local)
                    .format("%A, %B %-d, %Y at %H:%M")
            ),
            QueueDropTarget::Day(date)
            | QueueDropTarget::OnDayItem { date, .. }
            | QueueDropTarget::Before { date, .. }
            | QueueDropTarget::After { date, .. } => format!(
                "Schedule in the queue on {}. Existing times and multi-day spans stay the same.",
                date.format("%A, %B %-d, %Y")
            ),
        };
        if items.len() > 1
            && dragged.source_anchor.is_some()
            && matches!(
                target,
                QueueDropTarget::Day(_) | QueueDropTarget::OnDayItem { .. }
            )
        {
            detail.push_str(" Items on different days keep their spacing.");
        }
        let verb = if target == QueueDropTarget::Unscheduled {
            "Unschedule"
        } else {
            "Schedule"
        };

        self.clear_drop_target(cx);
        confirm_drop(count, verb, detail, window, cx, move |_, cx| {
            AppDatabaseStore::global(cx).update(cx, |store, cx| {
                for (item, start) in items.into_iter().zip(starts) {
                    place_dropped_item(item, start, store, cx);
                }
            });
        });
    }
}

fn queue_draft(kind: TimelineCreationKind, date: Option<NaiveDate>) -> AnyItem {
    match (kind, date) {
        (TimelineCreationKind::Action, _) | (_, None) => AnyItem::Action(
            Action::new("")
                .with_queued(true)
                .with_start(date.map(SchedulePoint::Date)),
        ),
        (TimelineCreationKind::Event, Some(date)) => {
            let time = NaiveTime::from_hms_opt(9, 0, 0).expect("9am is a valid time");
            let naive = date.and_time(time);
            let start = Local
                .from_local_datetime(&naive)
                .earliest()
                .map(|time| time.with_timezone(&Utc))
                .unwrap_or_else(|| naive.and_utc());
            AnyItem::Event(Event::new("", start, Duration::hours(1)))
        }
    }
}

fn dropped_starts(
    items: &[AnyItem],
    source_anchor: Option<SchedulePoint>,
    target: QueueDropTarget,
    default_duration: RelativeDuration,
) -> Vec<Option<SchedulePoint>> {
    match target {
        QueueDropTarget::Unscheduled => items
            .iter()
            .map(|item| match item {
                AnyItem::Action(_) => None,
                _ => item.start(),
            })
            .collect(),
        QueueDropTarget::Day(date) | QueueDropTarget::OnDayItem { date, .. } => {
            let source_date = source_anchor.map(NaiveDate::from);
            items
                .iter()
                .map(|item| {
                    let item_date = item.start_date();
                    let target_date = source_date
                        .zip(item_date)
                        .and_then(|(anchor, item_date)| date.checked_add_signed(item_date - anchor))
                        .unwrap_or(date);
                    Some(item_start_on_date(item, target_date))
                })
                .collect()
        }
        QueueDropTarget::Before {
            date,
            anchor: Some(anchor),
            ..
        } => {
            let mut cursor = anchor;
            let mut starts = vec![None; items.len()];
            for (ix, item) in items.iter().enumerate().rev() {
                if matches!(item, AnyItem::Marker(_)) {
                    starts[ix] = Some(SchedulePoint::Date(date));
                } else {
                    cursor = start_before(item, cursor, default_duration);
                    starts[ix] = Some(SchedulePoint::DateTime(cursor));
                }
            }
            starts
        }
        QueueDropTarget::After {
            date,
            anchor: Some(anchor),
            ..
        } => {
            let mut cursor = anchor;
            items
                .iter()
                .map(|item| {
                    if matches!(item, AnyItem::Marker(_)) {
                        Some(SchedulePoint::Date(date))
                    } else {
                        let start = cursor;
                        cursor += effective_duration(item, cursor, default_duration);
                        Some(SchedulePoint::DateTime(start))
                    }
                })
                .collect()
        }
        QueueDropTarget::Before { date, .. } | QueueDropTarget::After { date, .. } => items
            .iter()
            .map(|item| Some(item_start_on_date(item, date)))
            .collect(),
    }
}

fn item_start_on_date(item: &AnyItem, date: NaiveDate) -> SchedulePoint {
    match item {
        AnyItem::Action(action) => action_start_on_date(action, date),
        AnyItem::Marker(_) => SchedulePoint::Date(date),
        _ => match item.start() {
            Some(SchedulePoint::DateTime(start)) => {
                let local = start.with_timezone(&Local);
                SchedulePoint::DateTime(local_time_on(date, local.time()))
            }
            _ => SchedulePoint::Date(date),
        },
    }
}

fn start_before(
    item: &AnyItem,
    end: DateTime<Utc>,
    default_duration: RelativeDuration,
) -> DateTime<Utc> {
    let duration = item
        .duration()
        .or_else(|| matches!(item, AnyItem::Action(_)).then_some(default_duration));
    match duration {
        Some(duration) => {
            let start = (end.with_timezone(&Local) - duration).with_timezone(&Utc);
            if start < end {
                start
            } else {
                end - Duration::minutes(1)
            }
        }
        None => end - Duration::minutes(1),
    }
}

fn effective_duration(
    item: &AnyItem,
    at: DateTime<Utc>,
    default_duration: RelativeDuration,
) -> Duration {
    let at = at.with_timezone(&Local);
    let duration = item
        .duration()
        .or_else(|| matches!(item, AnyItem::Action(_)).then_some(default_duration))
        .map(|duration| (at + duration) - at)
        .unwrap_or_else(|| Duration::minutes(1));
    duration.max(Duration::minutes(1))
}

fn place_dropped_action(action: Action, start: Option<SchedulePoint>) -> Action {
    action.with_queued(true).with_start(start)
}

fn place_dropped_item(
    item: AnyItem,
    start: Option<SchedulePoint>,
    store: &mut AppDatabaseStore,
    cx: &mut Context<AppDatabaseStore>,
) {
    match item {
        AnyItem::Action(action) => {
            store.upsert_action(place_dropped_action(action, start), cx);
        }
        AnyItem::Event(mut event) => {
            if let Some(SchedulePoint::DateTime(start)) = start {
                event.start = start;
                store.upsert_event(event, cx);
            }
        }
        AnyItem::Routine(routine) => {
            let start = start.map(|start| match start {
                SchedulePoint::DateTime(start) => start,
                SchedulePoint::Date(date) => local_time_on(date, NaiveTime::MIN),
            });
            store.instantiate_routine(routine.id, start, cx);
        }
        AnyItem::Marker(marker) => {
            if let Some(start) = start {
                store.upsert_marker(marker_on_date(marker, NaiveDate::from(start)), cx);
            }
        }
        AnyItem::Signal(mut signal) => {
            if let Some(SchedulePoint::DateTime(start)) = start {
                signal.datetime = start;
                store.upsert_signal(signal, cx);
            }
        }
        AnyItem::ActionTemplate(_) | AnyItem::EventTemplate(_) => {}
    }
}

fn marker_on_date(mut marker: Marker, date: NaiveDate) -> Marker {
    let span = marker.end_date.map(|end| end - marker.date);
    marker.date = date;
    marker.end_date = span.map(|span| date + span);
    marker
}

impl Focusable for QueueView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl MainViewTab for QueueView {
    const TAB: SelectedMainView = SelectedMainView::Queue;

    fn scope() -> Option<SelectionScope> {
        Some(SelectionScope::Queue)
    }

    fn go_to_now(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let today = Local::now().date_naive();
        self.sections.reveal(QueueSection::Day(today));
        self.reveal = Some(QueueReveal::Day(today));
        self.invalidate(cx);
    }
}

impl MarqueeView for QueueView {
    fn marquee(&self) -> &MarqueeSelection {
        &self.marquee
    }

    fn marquee_mut(&mut self) -> &mut MarqueeSelection {
        &mut self.marquee
    }

    fn marquee_focus(&self, _cx: &App) -> Option<FocusHandle> {
        Some(self.focus_handle.clone())
    }

    fn marquee_dragged(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let speed = self.edge_scroll_speed_at(position);
        self.set_edge_scroll_speed(speed, cx);
    }

    fn marquee_ended(&mut self, cx: &mut Context<Self>) {
        self.set_edge_scroll_speed(None, cx);
    }
}

impl Render for QueueView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_workspace(
            AppDatabaseStore::global(cx).read(cx).workspace_generation(),
            cx,
        );
        let dragging = self.marquee.is_active() || self.dragged_items.is_some();
        let rebuild = self.agenda_stale || self.today != Local::now().date_naive();
        let anchor =
            (!rebuild && !dragging && !self.sections.is_animating() && self.reveal.is_none())
                .then(|| RowAnchor::capture(&self.agenda, &self.layout, &self.scroll_handle))
                .flatten();
        let previous_rows = self.layout.rows.clone();
        let previous_sizes = self.layout.sizes.clone();
        if !dragging {
            self.details.animate(window, cx);
        } else {
            self.details.pause();
        }
        if rebuild {
            self.rebuild(cx);
        }
        let empty = AppDatabaseStore::global(cx).read(cx).is_ready()
            && self.agenda.rows.iter().all(|row| match row {
                QueueRow::Item { .. } => false,
                QueueRow::Day { markers, .. } => markers.is_empty(),
                QueueRow::Create { .. } | QueueRow::AnyTime | QueueRow::Missed => true,
            });
        if empty && self.empty_day != Some(self.today) {
            self.sections.set_expanded(QueueSection::Unscheduled, true);
            self.sections
                .set_expanded(QueueSection::Day(self.today), true);
        }
        self.empty_day = empty.then_some(self.today);

        if !dragging {
            self.sections.animate(window, cx);
        }
        self.update_layout();
        let anchor = anchor
            .filter(|_| {
                previous_rows == self.layout.rows
                    && previous_sizes != self.layout.sizes
                    && !self.sections.is_animating()
            })
            .and_then(|anchor| anchor.resolve(&self.agenda, &self.layout));
        let reveal = self.reveal_row();

        SelectionManager::report_order(&self.order, cx);

        let list = v_virtual_list(
            cx.entity(),
            "queue-agenda",
            self.layout.sizes.clone(),
            move |view, visible_range, window, cx| {
                let visible_range = if let Some((_, offset)) = anchor {
                    scroll::restore_offset(
                        &view.scroll_handle,
                        offset,
                        visible_range,
                        view.layout.rows.len(),
                    )
                } else {
                    visible_range
                };
                view.apply_edge_scroll(window, cx);
                visible_range
                    .map(|ix| {
                        let source = view.layout.rows[ix];
                        let height = view.layout.sizes[ix].height;
                        let inset = row_top_inset(source);
                        let full_height = view.row_height(&view.agenda.rows[source]) + inset;
                        div()
                            .w_full()
                            .min_w(LIST_VIEW_MIN_WIDTH)
                            .max_w(LIST_VIEW_MAX_WIDTH)
                            .mx_auto()
                            .h(height)
                            .when(height < full_height, |row| row.overflow_hidden())
                            .child(
                                div()
                                    .h(full_height)
                                    .pt(inset)
                                    .child(view.render_row(ix, window, cx)),
                            )
                    })
                    .collect()
            },
        )
        .size_full()
        .track_scroll(&self.scroll_handle)
        .when_some(
            reveal.or_else(|| anchor.map(|(row, _)| (row, ScrollStrategy::Top))),
            |list, (row, strategy)| list.scroll_to_item(row, strategy),
        );

        let body = div()
            .id("queue-body")
            .relative()
            .size_full()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| view.clear_drop_target(cx)),
            )
            .child(list)
            .when(empty, |this| {
                let headings_height = self
                    .layout
                    .sizes
                    .iter()
                    .map(|row| row.height)
                    .sum::<Pixels>();
                this.child(
                    div()
                        .id("queue-empty")
                        .absolute()
                        .top(headings_height)
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .overflow_hidden()
                        .child(EmptyState::new(
                            Icon::new(AppIcon::ListChecks),
                            "Nothing queued",
                        )),
                )
            });
        let body = marquee(body, self, cx);

        self.tab_root(div().row(), cx)
            .size_full()
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|view, event, window, cx| {
                    view.open_context_menu(None, event, window, cx);
                }),
            )
            .on_drag_move::<DragData<DraggedItems>>(cx.listener(
                |view, event: &DragMoveEvent<DragData<DraggedItems>>, _, cx| {
                    view.handle_drag_move(event, cx);
                },
            ))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| view.clear_drop_target(cx)),
            )
            .child(body)
    }
}
