use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use crate::components::ext::InteractiveElementExt;
use crate::components::menu::MenuBuilder;
use crate::icons::Icon;
use chrono::{Local, NaiveDate};
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, DragMoveEvent, ElementId, Entity,
    FocusHandle, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Pixels, Render,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_kit_theme::ActiveTheme;
use subroutine_core::{Action, AnyItem};
use uuid::Uuid;

use crate::{
    AppIcon,
    components::{
        CardMeta, DragData, DraggedItems, DropZone, DynamicList, DynamicListCardStates,
        DynamicListDelegate, DynamicListState, EmptyState, ItemCard, MarqueeSelection, MarqueeView,
        SIDEBAR_GUTTER, SIDEBAR_ITEM_GAP, SIDEBAR_ITEM_HEIGHT, SidebarDraft, SidebarPanel, marquee,
    },
    item_manager::{DraftView, ItemManager},
    presentation::UxColor,
    selection::{
        DismissExt as _, SelectionManager, SelectionOrder, SelectionScope, focus_item,
        focus_item_extending,
    },
    settings::Settings,
    stores::{AppDatabaseStore, DataChanged, WorkspaceContent},
    views::main_view::format_item_duration,
};

use super::{
    drop_confirmation::{confirm_drop, resolve_dragged},
    library_drop::{LibraryDropItem, LibraryDropTarget},
};

const ITEM_HEIGHT: Pixels = SIDEBAR_ITEM_HEIGHT;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActionLibraryKind {
    Unqueued,
    Unscheduled,
}

impl ActionLibraryKind {
    fn id(self) -> &'static str {
        match self {
            Self::Unqueued => "unqueued",
            Self::Unscheduled => "unscheduled",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Unqueued => "Unqueued",
            Self::Unscheduled => "Unscheduled",
        }
    }

    fn scope(self) -> SelectionScope {
        match self {
            Self::Unqueued => SelectionScope::Unqueued,
            Self::Unscheduled => SelectionScope::Unscheduled,
        }
    }

    fn draft_view(self) -> DraftView {
        match self {
            Self::Unqueued => DraftView::Unqueued,
            Self::Unscheduled => DraftView::Unscheduled,
        }
    }

    fn actions(self, store: &Entity<AppDatabaseStore>, cx: &App) -> Vec<Action> {
        let settings = Settings::global(cx);
        let pipeline = store.read(cx).pipeline(&settings);
        let actions = match self {
            Self::Unqueued => pipeline.waiting(),
            Self::Unscheduled => pipeline
                .queue()
                .into_iter()
                .filter(|action| action.start.is_none())
                .collect(),
        };
        actions.into_iter().cloned().collect()
    }
}

fn action_meta(action: &Action, cx: &App) -> Vec<CardMeta> {
    let mut meta = Vec::new();

    if let Some(date) = action.start.map(|start| start.date_naive()) {
        let today = Local::now().date_naive();
        let (text, role) = date_meta(date, today);
        meta.push(
            CardMeta::new(text)
                .icon(AppIcon::CalendarClock)
                .color(role.text(cx.theme())),
        );
    }

    if let Some(duration) = action.duration {
        let text = format_item_duration(duration, Local::now());
        if !text.is_empty() {
            meta.push(CardMeta::new(text).icon(AppIcon::Clock));
        }
    }

    meta
}

fn date_meta(date: NaiveDate, today: NaiveDate) -> (String, UxColor) {
    let text = if date < today {
        format!("Overdue · {}", date.format("%b %-d"))
    } else if date == today {
        "Today".to_string()
    } else if Some(date) == today.succ_opt() {
        "Tomorrow".to_string()
    } else {
        date.format("%b %-d").to_string()
    };
    (text, UxColor::Neutral)
}

#[derive(Clone)]
struct ActionLibraryDelegate {
    kind: ActionLibraryKind,
    actions: Vec<Action>,

    query: String,
    items: Vec<Action>,
    cards: DynamicListCardStates,
    focus_handles: HashMap<Uuid, FocusHandle>,
    // Departure delegates must not report their old selection order.
    order: Rc<RefCell<SelectionOrder>>,
}

impl ActionLibraryDelegate {
    fn new(kind: ActionLibraryKind, cx: &mut App) -> Self {
        let mut this = Self {
            kind,
            actions: Vec::new(),

            query: String::new(),
            items: Vec::new(),
            cards: DynamicListCardStates::default(),
            focus_handles: HashMap::new(),
            order: Rc::new(RefCell::new(SelectionOrder::new(kind.scope(), []))),
        };
        this.reload(cx);
        this
    }

    fn reload(&mut self, cx: &mut App) {
        let store = AppDatabaseStore::global(cx);
        if self
            .cards
            .set_generation(store.read(cx).workspace_generation())
        {
            self.focus_handles.clear();
        }
        self.actions = self.kind.actions(&store, cx);

        self.rebuild(cx);
    }

    fn rebuild(&mut self, cx: &mut App) {
        let mut items = self.actions.clone();

        let tokens = self
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect::<Vec<_>>();
        if !tokens.is_empty() {
            items.retain(|action| {
                let text = format!(
                    "{} {}",
                    action.title.to_lowercase(),
                    action.content.as_deref().unwrap_or_default().to_lowercase()
                );
                tokens.iter().all(|token| text.contains(token))
            });
        }
        items.sort_by(|a, b| {
            let a_date = a.start.map(|start| start.date_naive());
            let b_date = b.start.map(|start| start.date_naive());
            match (a_date, b_date) {
                (Some(a), Some(b)) => a.cmp(&b),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.id.cmp(&b.id),
            }
        });
        self.items = items;
        *self.order.borrow_mut() =
            SelectionOrder::new(self.kind.scope(), self.items.iter().map(|action| action.id));

        let current: HashSet<Uuid> = self.items.iter().map(|action| action.id).collect();
        self.cards.retain(self.items.iter().filter_map(|action| {
            action
                .content
                .as_deref()
                .filter(|content| !content.trim().is_empty())
                .map(|_| action.id)
        }));
        self.focus_handles.retain(|id, _| current.contains(id));
        for id in current {
            self.focus_handles
                .entry(id)
                .or_insert_with(|| cx.focus_handle());
        }
    }

    fn order(&self) -> SelectionOrder {
        self.order.borrow().clone()
    }

    fn index_of(&self, id: Uuid) -> Option<usize> {
        self.items.iter().position(|action| action.id == id)
    }

    fn focus_handle_at(&self, ix: usize) -> Option<FocusHandle> {
        let action = self.items.get(ix)?;
        self.focus_handles.get(&action.id).cloned()
    }
}

impl DynamicListDelegate for ActionLibraryDelegate {
    type Item = AnyElement;

    fn items_count(&self, _cx: &App) -> usize {
        self.items.len()
    }

    fn item_id(&self, ix: usize, _cx: &App) -> ElementId {
        match self.items.get(ix) {
            Some(action) => ElementId::Uuid(action.id),
            None => ElementId::Integer(ix as u64),
        }
    }

    fn layout_epoch(&self) -> u64 {
        self.cards.generation()
    }

    fn prepare_layout(&mut self, window: &mut Window, cx: &mut App) {
        self.cards.animate(ITEM_HEIGHT, window, cx);
    }

    fn pause_layout(&mut self) {
        self.cards.pause();
    }

    fn item_height(&self, ix: usize, _cx: &App) -> Pixels {
        self.items.get(ix).map_or(ITEM_HEIGHT, |action| {
            self.cards.height(action.id, ITEM_HEIGHT)
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
        let action = self.items.get(ix)?.clone();
        let action_id = action.id;
        let meta = action_meta(&action, cx);
        let handle = self
            .focus_handles
            .get(&action_id)
            .cloned()
            .unwrap_or_else(|| cx.focus_handle());

        let item = AnyItem::Action(action);
        let is_editing = ItemManager::global(cx).read(cx).is_being_edited(item.id());

        Some(
            div()
                .size_full()
                .px(SIDEBAR_GUTTER)
                .child(
                    ItemCard::new(&item, None, window, cx)
                        .neighbors(
                            ix.checked_sub(1)
                                .and_then(|previous| self.items.get(previous))
                                .map(|action| action.id),
                            self.items.get(ix + 1).map(|action| action.id),
                        )
                        .details(self.cards.get(action_id))
                        .meta(meta)
                        .schedule_navigation()
                        .with_focus_handle(handle)
                        .selectable(self.order())
                        .size_full()
                        .draggable(true, None)
                        .block_mouse_except_scroll()
                        .on_key_down(cx.listener(move |list, event: &KeyDownEvent, window, cx| {
                            if event.is_held {
                                return;
                            }
                            let item_manager = ItemManager::global(cx);
                            let key = event.keystroke.key.as_str();
                            let extend_selection =
                                event.keystroke.modifiers.shift && matches!(key, "up" | "down");
                            match key {
                                "up" | "k" if !is_editing => {
                                    cx.stop_propagation();
                                    focus_sibling(
                                        list,
                                        action_id,
                                        -1,
                                        extend_selection,
                                        window,
                                        cx,
                                    );
                                }
                                "down" | "j" if !is_editing => {
                                    cx.stop_propagation();
                                    focus_sibling(list, action_id, 1, extend_selection, window, cx);
                                }
                                "enter" if !is_editing => {
                                    cx.stop_propagation();
                                    item_manager.update(cx, |handler, cx| {
                                        handler.begin_edit(&item, false, window, cx);
                                    });
                                    cx.notify();
                                }
                                _ => {}
                            }
                        })),
                )
                .into_any_element(),
        )
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<DynamicListState<Self>>,
    ) -> impl IntoElement {
        let label = if self.query.trim().is_empty() {
            match self.kind {
                ActionLibraryKind::Unqueued => "Nothing unqueued",
                ActionLibraryKind::Unscheduled => "Nothing unscheduled",
            }
        } else {
            "No matches"
        };
        EmptyState::new(Icon::new(AppIcon::Inbox), label)
    }
}

fn focus_sibling(
    list: &mut DynamicListState<ActionLibraryDelegate>,
    id: Uuid,
    offset: isize,
    extend_selection: bool,
    window: &mut Window,
    cx: &mut Context<DynamicListState<ActionLibraryDelegate>>,
) {
    let delegate = list.delegate();
    let Some(pos) = delegate.index_of(id) else {
        return;
    };
    let Some(next) = pos
        .checked_add_signed(offset)
        .filter(|next| *next < delegate.items.len())
    else {
        return;
    };
    let Some(handle) = delegate.focus_handle_at(next) else {
        return;
    };
    let Some(next_id) = delegate.items.get(next).map(|action| action.id) else {
        return;
    };
    let order = delegate.order();

    if extend_selection {
        focus_item_extending(&order, id, next_id, &handle, window, cx);
    } else {
        focus_item(delegate.kind.scope(), next_id, &handle, window, cx);
    }
    list.scroll_item_into_view(next, cx);
}

pub struct ActionLibraryView {
    kind: ActionLibraryKind,
    list: Entity<DynamicListState<ActionLibraryDelegate>>,
    focus_handle: FocusHandle,
    drop_active: bool,
    marquee: MarqueeSelection,
}

impl ActionLibraryView {
    pub fn new(kind: ActionLibraryKind, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let list = cx.new(|cx| {
            DynamicListState::new(ActionLibraryDelegate::new(kind, cx), window, cx)
                .gap(px(0.))
                .content_inset_top(crate::views::LIBRARY_HEADER_HEIGHT + SIDEBAR_ITEM_GAP)
                .scrollbar_visible(false)
        });

        let db_store = AppDatabaseStore::global(cx);
        cx.subscribe(&db_store, |view, _store, _: &DataChanged, cx| {
            view.list.update(cx, |list, cx| {
                list.update_items(cx, |delegate, cx| delegate.reload(cx));
            });
            cx.notify();
        })
        .detach();

        let item_manager = ItemManager::global(cx);
        cx.observe(&item_manager, |_, _, cx| cx.notify()).detach();

        Self {
            kind,
            list,
            focus_handle: cx.focus_handle(),
            drop_active: false,
            marquee: MarqueeSelection::new(kind.scope()),
        }
    }

    pub(crate) fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    pub(crate) fn item_count(&self, cx: &App) -> usize {
        self.list.read(cx).delegate().items_count(cx)
    }

    pub(crate) fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| {
            list.update_items(cx, |delegate, cx| {
                if delegate.query != query {
                    delegate.query = query.to_owned();
                    delegate.rebuild(cx);
                }
            });
        });
        cx.notify();
    }

    fn add_draft_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if AppDatabaseStore::global(cx).read(cx).workspace_content() != WorkspaceContent::Ready {
            return;
        }
        let manager = ItemManager::global(cx);
        manager.update(cx, |manager, cx| manager.commit_open_edit(window, cx));
        if manager.read(cx).is_editing() {
            return;
        }
        SelectionManager::clear_global(cx);
        self.focus_handle.focus(window, cx);
        let item = AnyItem::Action(
            Action::new("").with_queued(self.kind == ActionLibraryKind::Unscheduled),
        );
        manager.update(cx, |manager, cx| {
            manager.begin_view_draft(&item, self.kind.draft_view(), None, window, cx);
        });
        cx.notify();
    }

    pub(crate) fn clear_drop_target(&mut self, cx: &mut Context<Self>) {
        if self.drop_active {
            self.drop_active = false;
            cx.notify();
        }
    }

    fn drop_unscheduled(
        &mut self,
        dragged: &DraggedItems,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let store = AppDatabaseStore::global(cx);
        let items = resolve_dragged(dragged, cx)
            .items
            .into_iter()
            .map(|item| LibraryDropItem {
                from_saved_items: store.read(cx).get_item(item.id()).is_none(),
                item,
            })
            .collect::<Vec<_>>();
        let Some(plan) = LibraryDropTarget::Unscheduled.plan(&items) else {
            return;
        };
        confirm_drop(
            plan.accepted,
            "Move",
            "Move these actions to Unscheduled, queueing them without scheduled dates and preserving their pins."
                .into(),
            window,
            cx,
            move |_, cx| {
                store.update(cx, |store, cx| {
                    let _ = store.update_items(plan.items, cx);
                });
            },
        );
    }

    fn drop_zone(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> DropZone<DragData<DraggedItems>> {
        DropZone::new(format!("{}-drop", self.kind.id()))
            .size_full()
            .active(self.drop_active)
            .transparent_background()
            .rounded_none()
            .on_drag_move(cx.listener(
                move |this, event: &DragMoveEvent<DragData<DraggedItems>>, _window, cx| {
                    let active = event.bounds.contains(&event.event.position);
                    if active != this.drop_active {
                        this.drop_active = active;
                        cx.notify();
                    }
                },
            ))
            .on_drop(
                cx.listener(|this, data: &DragData<DraggedItems>, window, cx| {
                    if !this.drop_active {
                        return;
                    }
                    this.clear_drop_target(cx);
                    if this.kind == ActionLibraryKind::Unscheduled {
                        this.drop_unscheduled(&data.data, window, cx);
                        return;
                    }
                    let db_store = AppDatabaseStore::global(cx);
                    let mut seen = std::collections::HashSet::new();
                    let ids: Vec<_> = data
                        .data
                        .actions()
                        .filter_map(|action| db_store.read(cx).get_item(action.id))
                        .filter_map(|item| match item {
                            AnyItem::Action(action)
                                if (action.queued || action.pinned || action.start.is_some())
                                    && seen.insert(action.id) =>
                            {
                                Some(action.id)
                            }
                            _ => None,
                        })
                        .collect();
                    super::drop_confirmation::confirm_drop(
                        ids.len(),
                        "Move",
                        "Move these actions to Unqueued, removing their scheduled dates and pins."
                            .into(),
                        window,
                        cx,
                        move |_, cx| {
                            db_store.update(cx, |store, cx| {
                                let _ = store.backlog_actions(&ids, cx);
                            });
                        },
                    );
                }),
            )
    }
}

fn action_context_menu(view: Entity<ActionLibraryView>, kind: ActionLibraryKind) -> MenuBuilder {
    MenuBuilder::new()
        .label(format!("{} Actions", kind.label()))
        .item("New action", move |window, cx| {
            view.update(cx, |view, cx| view.add_draft_action(window, cx));
        })
}

impl MarqueeView for ActionLibraryView {
    fn marquee(&self) -> &MarqueeSelection {
        &self.marquee
    }

    fn marquee_mut(&mut self) -> &mut MarqueeSelection {
        &mut self.marquee
    }

    fn marquee_focus(&self, _cx: &App) -> Option<FocusHandle> {
        Some(self.focus_handle.clone())
    }
}

impl Render for ActionLibraryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.list.update(cx, |list, cx| {
            list.scroll_marquee(&mut self.marquee, window, cx);
        });
        SelectionManager::report_order(&self.list.read(cx).delegate().order(), cx);
        if let Some(request) =
            SelectionManager::take_completion_focus(self.kind.scope(), window, cx)
        {
            let target = request.target.and_then(|id| {
                let delegate = self.list.read(cx).delegate();
                let row = delegate.index_of(id)?;
                Some((row, id, delegate.focus_handle_at(row)?))
            });
            if let Some((row, id, handle)) = target {
                self.list
                    .update(cx, |list, cx| list.reveal_item_for_focus(row, cx));
                focus_item(request.scope, id, &handle, window, cx);
            } else {
                self.focus_handle.focus(window, cx);
            }
        }
        let entity = cx.entity();

        let body = SidebarPanel::new(format!("{}-backdrop", self.kind.id()))
            .footer(
                SidebarDraft::new(
                    format!("new-{}-item", self.kind.id()),
                    "New action",
                    self.kind.draft_view(),
                )
                .on_click({
                    let entity = entity.clone();
                    move |_event, window, cx| {
                        entity.update(cx, |view, cx| {
                            view.add_draft_action(window, cx);
                        });
                    }
                }),
            )
            .on_double_click({
                let entity = entity.clone();
                move |_event: &ClickEvent, window, cx| {
                    entity.update(cx, |view, cx| {
                        view.add_draft_action(window, cx);
                    });
                }
            })
            .on_aux_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                if event.is_right_click() {
                    let builder = action_context_menu(cx.entity().clone(), view.kind);
                    crate::components::menu::open_context_menu(
                        builder,
                        event.position(),
                        window,
                        cx,
                    );
                    cx.notify();
                }
            }))
            .child(
                self.drop_zone(window, cx).child(
                    div()
                        .size_full()
                        .child(DynamicList::new(&self.list).size_full()),
                ),
            );
        marquee(body, self, cx)
            .track_focus(&self.focus_handle)
            .on_dismiss(self.kind.scope(), Some(self.focus_handle.clone()))
    }
}
