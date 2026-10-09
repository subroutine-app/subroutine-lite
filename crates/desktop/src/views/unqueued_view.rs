use std::collections::{HashMap, HashSet};

use crate::components::ext::InteractiveElementExt;
use crate::components::menu::MenuBuilder;
use crate::icons::Icon;
use chrono::{Local, NaiveDate};
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, DragMoveEvent, ElementId, Entity,
    FocusHandle, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Pixels, Render,
    StatefulInteractiveElement, Styled, Window, div,
};
use gpui_kit_theme::ActiveTheme;
use subroutine_core::{Action, AnyItem};
use uuid::Uuid;

use crate::{
    AppIcon,
    components::{
        CardMeta, DragData, DraggedItems, DropZone, DynamicList, DynamicListCardStates,
        DynamicListDelegate, DynamicListState, EmptyState, ItemCard, MarqueeSelection, MarqueeView,
        SIDEBAR_GUTTER, SIDEBAR_ITEM_GAP, SIDEBAR_ITEM_HEIGHT, SidebarAddButton, SidebarPanel,
        marquee,
    },
    item_manager::{DiscardDraft, ItemManager},
    presentation::UxColor,
    selection::{
        DismissExt as _, SelectionOrder, SelectionScope, focus_item, focus_item_extending,
    },
    settings::Settings,
    stores::{AppDatabaseStore, DataChanged},
    views::main_view::format_item_duration,
};

const ITEM_HEIGHT: Pixels = SIDEBAR_ITEM_HEIGHT;
const ITEM_GAP: Pixels = SIDEBAR_ITEM_GAP;

fn unqueued_actions(store: &Entity<AppDatabaseStore>, cx: &App) -> Vec<Action> {
    let settings = Settings::global(cx);
    store
        .read(cx)
        .pipeline(&settings)
        .waiting()
        .into_iter()
        .cloned()
        .collect()
}

fn unqueued_meta(action: &Action, cx: &App) -> Vec<CardMeta> {
    let mut meta = Vec::new();

    if let Some(date) = action.start.map(|start| start.date_naive()) {
        let today = Local::now().date_naive();
        let (text, role) = unqueued_date_meta(date, today);
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

fn unqueued_date_meta(date: NaiveDate, today: NaiveDate) -> (String, UxColor) {
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
struct UnqueuedDelegate {
    unqueued: Vec<Action>,
    drafts: Vec<Action>,
    query: String,
    items: Vec<Action>,
    cards: DynamicListCardStates,
    focus_handles: HashMap<Uuid, FocusHandle>,
    order: SelectionOrder,
}

impl UnqueuedDelegate {
    fn new(cx: &mut App) -> Self {
        let mut this = Self {
            unqueued: Vec::new(),
            drafts: Vec::new(),
            query: String::new(),
            items: Vec::new(),
            cards: DynamicListCardStates::default(),
            focus_handles: HashMap::new(),
            order: SelectionOrder::new(SelectionScope::Unqueued, []),
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
            self.drafts.clear();
            self.focus_handles.clear();
        }
        self.unqueued = unqueued_actions(&store, cx);
        self.drafts
            .retain(|draft| !self.unqueued.iter().any(|action| action.id == draft.id));
        self.rebuild(cx);
    }

    fn push_draft(&mut self, action: Action, cx: &mut App) {
        self.drafts.push(action);
        self.rebuild(cx);
    }

    fn discard_draft(&mut self, id: Uuid, cx: &mut App) {
        self.drafts.retain(|draft| draft.id != id);
        self.rebuild(cx);
    }

    fn rebuild(&mut self, cx: &mut App) {
        let mut items = self.unqueued.clone();
        items.extend(self.drafts.iter().cloned());
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
        self.order = SelectionOrder::new(
            SelectionScope::Unqueued,
            self.items.iter().map(|action| action.id),
        );

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

    fn index_of(&self, id: Uuid) -> Option<usize> {
        self.items.iter().position(|action| action.id == id)
    }

    fn focus_handle_at(&self, ix: usize) -> Option<FocusHandle> {
        let action = self.items.get(ix)?;
        self.focus_handles.get(&action.id).cloned()
    }
}

impl DynamicListDelegate for UnqueuedDelegate {
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
        let meta = unqueued_meta(&action, cx);
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
                        .details(self.cards.get(action_id))
                        .meta(meta)
                        .schedule_navigation()
                        .with_focus_handle(handle)
                        .selectable(self.order.clone())
                        .size_full()
                        .border(false)
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
        if self.query.trim().is_empty() {
            EmptyState::new(Icon::new(AppIcon::Inbox), "Nothing unqueued")
        } else {
            EmptyState::new(Icon::new(AppIcon::Inbox), "No matches")
        }
    }
}

fn focus_sibling(
    list: &mut DynamicListState<UnqueuedDelegate>,
    id: Uuid,
    offset: isize,
    extend_selection: bool,
    window: &mut Window,
    cx: &mut Context<DynamicListState<UnqueuedDelegate>>,
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
    let order = delegate.order.clone();

    if extend_selection {
        focus_item_extending(&order, id, next_id, &handle, window, cx);
    } else {
        focus_item(SelectionScope::Unqueued, next_id, &handle, window, cx);
    }
    list.scroll_item_into_view(next, cx);
}

pub struct UnqueuedView {
    list: Entity<DynamicListState<UnqueuedDelegate>>,
    focus_handle: FocusHandle,
    drop_active: bool,
    marquee: MarqueeSelection,
}

impl UnqueuedView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let list = cx.new(|cx| {
            DynamicListState::new(UnqueuedDelegate::new(cx), window, cx)
                .gap(ITEM_GAP)
                .content_inset_top(crate::views::LIBRARY_HEADER_HEIGHT + ITEM_GAP)
                .scrollbar_visible(false)
        });

        let db_store = AppDatabaseStore::global(cx);
        cx.subscribe(&db_store, |view, _store, _: &DataChanged, cx| {
            view.list.update(cx, |list, cx| {
                list.update_items(cx, |delegate, cx| delegate.reload(cx));
            });
        })
        .detach();

        let item_manager = ItemManager::global(cx);
        cx.observe(&item_manager, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&item_manager, |view, _, event: &DiscardDraft, cx| {
            let id = event.0;
            view.list.update(cx, |list, cx| {
                list.update_items(cx, |delegate, cx| delegate.discard_draft(id, cx));
            });
        })
        .detach();

        Self {
            list,
            focus_handle: cx.focus_handle(),
            drop_active: false,
            marquee: MarqueeSelection::new(SelectionScope::Unqueued),
        }
    }

    pub(crate) fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
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

    fn add_draft_unqueued_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let action = Action::new("");
        let id = action.id;
        let any_item = AnyItem::Action(action.clone());

        self.list.update(cx, |list, cx| {
            list.update_items(cx, |delegate, cx| delegate.push_draft(action, cx));
            if let Some(ix) = list.delegate().index_of(id) {
                list.scroll_item_into_view(ix, cx);
            }
        });

        ItemManager::global(cx).update(cx, |handler, cx| {
            handler.begin_fixed_type_draft(&any_item, window, cx);
        });
        cx.notify();
    }

    pub(crate) fn clear_drop_target(&mut self, cx: &mut Context<Self>) {
        if self.drop_active {
            self.drop_active = false;
            cx.notify();
        }
    }

    fn drop_zone(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> DropZone<DragData<DraggedItems>> {
        DropZone::new("unqueued-drop")
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

fn unqueued_context_menu(view: Entity<UnqueuedView>) -> MenuBuilder {
    MenuBuilder::new()
        .label("Unqueued Actions")
        .item("New action", move |window, cx| {
            view.update(cx, |view, cx| view.add_draft_unqueued_action(window, cx));
        })
}

impl MarqueeView for UnqueuedView {
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

impl Render for UnqueuedView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.list.update(cx, |list, cx| {
            list.scroll_marquee(&mut self.marquee, window, cx);
        });
        let entity = cx.entity();

        let body = SidebarPanel::new("unqueued-backdrop")
            .footer(
                SidebarAddButton::new("new-unqueued-item", "New action")
                    .tooltip("Add an action to the unqueued list")
                    .on_click({
                        let entity = entity.clone();
                        move |_event, window, cx| {
                            entity.update(cx, |view, cx| {
                                view.add_draft_unqueued_action(window, cx);
                            });
                        }
                    }),
            )
            .on_double_click({
                let entity = entity.clone();
                move |_event: &ClickEvent, window, cx| {
                    entity.update(cx, |view, cx| {
                        view.add_draft_unqueued_action(window, cx);
                    });
                }
            })
            .on_aux_click(cx.listener(move |_view, event: &ClickEvent, window, cx| {
                if event.is_right_click() {
                    let builder = unqueued_context_menu(cx.entity().clone());
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
            .on_dismiss(SelectionScope::Unqueued, Some(self.focus_handle.clone()))
    }
}
