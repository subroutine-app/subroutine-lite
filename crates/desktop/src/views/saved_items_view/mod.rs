use std::collections::{HashMap, HashSet};

use chronoutil::RelativeDuration;
use gpui::{
    App, Context, FocusHandle, InteractiveElement, IntoElement, ParentElement, Render,
    ScrollHandle, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder as _,
};
use subroutine_core::{ActionTemplate, AnyItem, EventTemplate};
use uuid::Uuid;

use crate::{
    components::{
        DragData, DraggedItems, ItemCardStates, MarqueeSelection, MarqueeView, SidebarAddButton,
        SidebarPanel,
        elastic_overscroll::ElasticOverscroll,
        marquee,
        menu::{MenuBuilder, open_context_menu},
    },
    item_manager::{DiscardDraft, ItemManager},
    item_subject::SavedItem,
    selection::{DismissExt as _, SelectionManager, SelectionScope},
    stores::{AppDatabaseStore, DataChanged},
};

mod list;
pub(crate) use list::saved_items_context_menu;
use list::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SavedItemsFilter {
    All,
    Actions,
    Events,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SavedItemKind {
    Action,
    Event,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SavedItemDropTarget {
    kind: SavedItemKind,
    id: Uuid,
    before: bool,
}

pub struct SavedItemsView {
    items: SavedItemsList,
    workspace_generation: u64,
    draft: Option<SavedItem>,
    scroll_handle: ScrollHandle,
    focus_handle: FocusHandle,
    item_focus_handles: HashMap<Uuid, FocusHandle>,
    marquee: MarqueeSelection,
    drop_target: Option<SavedItemDropTarget>,
    overscroll: ElasticOverscroll,
}

impl SavedItemsView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let db_store = AppDatabaseStore::global(cx);
        let mut items = SavedItemsList::new();
        {
            let store = db_store.read(cx);
            items.action_templates = store.action_templates();
            items.event_templates = store.event_templates();
            items.loading = !store.is_ready();
        }

        cx.observe(&db_store, |view, store, cx| {
            view.sync_workspace(store.read(cx).workspace_generation(), cx);
        })
        .detach();
        cx.subscribe(&db_store, |view, store, _: &DataChanged, cx| {
            view.sync_workspace(store.read(cx).workspace_generation(), cx);
            let store = store.read(cx);
            view.items.action_templates = store.action_templates();
            view.items.event_templates = store.event_templates();
            view.items.loading = false;
            if view.draft.as_ref().is_some_and(|draft| match draft {
                SavedItem::Action(template) => view
                    .items
                    .action_templates
                    .iter()
                    .any(|item| item.id == template.id),
                SavedItem::Event(template) => view
                    .items
                    .event_templates
                    .iter()
                    .any(|item| item.id == template.id),
            }) {
                view.draft = None;
            }
            view.retain_details();
            view.refresh_focus_handles(cx);
            cx.notify();
        })
        .detach();

        let item_manager = ItemManager::global(cx);
        cx.observe(&item_manager, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&item_manager, |view, _, event: &DiscardDraft, cx| {
            if view
                .draft
                .as_ref()
                .is_some_and(|draft| draft.id() == event.0)
            {
                view.draft = None;
                view.retain_details();
                cx.notify();
            }
        })
        .detach();

        let mut view = Self {
            items,
            workspace_generation: db_store.read(cx).workspace_generation(),
            draft: None,
            scroll_handle: ScrollHandle::new(),
            focus_handle: cx.focus_handle(),
            item_focus_handles: HashMap::new(),
            marquee: MarqueeSelection::new(SelectionScope::SavedItems),
            drop_target: None,
            overscroll: ElasticOverscroll::default(),
        };
        view.refresh_focus_handles(cx);
        view
    }

    fn sync_workspace(&mut self, generation: u64, cx: &mut Context<Self>) {
        if self.workspace_generation == generation {
            return;
        }
        self.workspace_generation = generation;
        self.items.details = ItemCardStates::default();
        self.draft = None;
        self.item_focus_handles.clear();
        self.refresh_focus_handles(cx);
        self.scroll_handle = ScrollHandle::new();
        self.overscroll.reset();
        self.marquee.end();
        self.drop_target = None;
        cx.notify();
    }

    fn retain_details(&self) {
        self.items.details.retain(
            self.items
                .action_templates
                .iter()
                .map(|item| item.id)
                .chain(self.items.event_templates.iter().map(|item| item.id))
                .chain(self.draft.iter().map(SavedItem::id)),
        );
    }

    pub(crate) fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    pub(crate) fn set_filter(&mut self, filter: SavedItemsFilter, cx: &mut Context<Self>) {
        if self.items.filter == filter {
            return;
        }
        self.items.filter = filter;
        self.refresh_focus_handles(cx);
        if SelectionManager::global(cx).read(cx).scope() == Some(SelectionScope::SavedItems) {
            SelectionManager::clear_global(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        if self.items.query == query {
            return;
        }
        self.items.query = query.to_owned();
        self.refresh_focus_handles(cx);
        if SelectionManager::global(cx).read(cx).scope() == Some(SelectionScope::SavedItems) {
            SelectionManager::clear_global(cx);
        }
        cx.notify();
    }

    fn add_draft(&mut self, kind: SavedItemKind, window: &mut Window, cx: &mut Context<Self>) {
        let manager = ItemManager::global(cx);
        manager.update(cx, |manager, cx| {
            manager.commit_open_edit(window, cx);
        });
        if manager.read(cx).is_editing() {
            return;
        }

        let draft = match kind {
            SavedItemKind::Action => SavedItem::Action(ActionTemplate::new("")),
            SavedItemKind::Event => {
                SavedItem::Event(EventTemplate::new("", RelativeDuration::minutes(30)))
            }
        };
        let id = draft.id();
        let item = AnyItem::from(draft.clone());
        self.draft = Some(draft);
        SelectionManager::clear_global(cx);
        self.focus_handle.focus(window, cx);
        manager.update(cx, |manager, cx| {
            manager.begin_edit(&item, true, window, cx);
        });
        self.overscroll.reset();
        cx.on_next_frame(window, move |view, _, cx| {
            if view.draft.as_ref().is_some_and(|draft| draft.id() == id) {
                view.scroll_handle.scroll_to_bottom();
                cx.notify();
            }
        });
        cx.notify();
    }

    fn refresh_focus_handles(&mut self, cx: &mut App) {
        let ids: HashSet<_> = self.items.matching_ids().into_iter().collect();
        self.item_focus_handles.retain(|id, _| ids.contains(id));
        for id in ids {
            self.item_focus_handles
                .entry(id)
                .or_insert_with(|| cx.focus_handle());
        }
    }

    fn set_drop_target(&mut self, target: Option<SavedItemDropTarget>, cx: &mut Context<Self>) {
        if self.drop_target != target {
            self.drop_target = target;
            cx.notify();
        }
    }

    pub(crate) fn clear_drop_target(&mut self, cx: &mut Context<Self>) {
        self.set_drop_target(None, cx);
    }

    pub(crate) fn clear_drag_feedback(&mut self, cx: &mut Context<Self>) {
        if self.drop_target.take().is_some() {
            cx.notify();
        }
    }

    fn commit_reorder(
        &mut self,
        source_ids: &[Uuid],
        target: SavedItemDropTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let store = AppDatabaseStore::global(cx);
        let current: Vec<_> = match target.kind {
            SavedItemKind::Action => self
                .items
                .action_templates
                .iter()
                .map(|item| item.id)
                .collect(),
            SavedItemKind::Event => self
                .items
                .event_templates
                .iter()
                .map(|item| item.id)
                .collect(),
        };
        let ordered = reorder_group(&current, source_ids, target.id, target.before);
        self.clear_drop_target(cx);
        if ordered == current {
            return;
        }
        let count = current.iter().filter(|id| source_ids.contains(id)).count();
        let detail = format!(
            "Reorder these saved items {} the dropped-on item in Saved Items.",
            if target.before { "before" } else { "after" },
        );
        super::drop_confirmation::confirm_drop(
            count,
            "Reorder",
            detail,
            window,
            cx,
            move |_, cx| {
                store.update(cx, |store, cx| match target.kind {
                    SavedItemKind::Action => store.reorder_action_templates(&ordered, cx),
                    SavedItemKind::Event => store.reorder_event_templates(&ordered, cx),
                });
            },
        );
    }
}

impl MarqueeView for SavedItemsView {
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

impl Render for SavedItemsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.retain_details();
        if !self.marquee.is_active() {
            self.items.details.animate(window, cx);
        } else {
            self.items.details.pause();
        }
        let entity = cx.entity();
        let overscroll_y = self.overscroll.advance(cx);
        if self.overscroll.needs_frame(cx) {
            window.request_animation_frame();
        }
        let content_inset_top =
            crate::views::LIBRARY_HEADER_HEIGHT + crate::components::SIDEBAR_ITEM_GAP;
        self.marquee
            .scroll(&self.scroll_handle, content_inset_top, window);
        let content = if let Some(empty) = self.items.empty_state().filter(|_| self.draft.is_none())
        {
            div()
                .absolute()
                .top(content_inset_top)
                .bottom_0()
                .left_0()
                .right_0()
                .overflow_hidden()
                .child(empty)
                .into_any_element()
        } else {
            let rows = div()
                .relative()
                .top(overscroll_y)
                .child(div().h(content_inset_top))
                .child(self.items.render(
                    entity.clone(),
                    &self.item_focus_handles,
                    self.drop_target,
                    window,
                    cx,
                ))
                .when_some(self.draft.as_ref(), |rows, draft| {
                    rows.child(render_draft(draft, &self.items.details, window, cx))
                });
            div()
                .id("saved-items-scroll")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&self.scroll_handle)
                .on_scroll_wheel(cx.listener(|view, event, window, cx| {
                    if view.overscroll.handle_scroll(event, window, cx) {
                        cx.notify();
                    }
                }))
                .on_mouse_up(gpui::MouseButton::Left, {
                    let entity = entity.clone();
                    move |_, _, cx| {
                        entity.update(cx, |view, cx| view.set_drop_target(None, cx));
                    }
                })
                .child(rows)
                .into_any_element()
        };
        let label = match self.items.filter {
            SavedItemsFilter::All => "New saved item",
            SavedItemsFilter::Actions => "New saved action",
            SavedItemsFilter::Events => "New saved event",
        };
        let body = SidebarPanel::new("saved-items-panel")
            .footer(
                SidebarAddButton::new("new-saved-item", label).on_click(cx.listener(
                    |view, event: &gpui::ClickEvent, window, cx| match view.items.filter {
                        SavedItemsFilter::All => {
                            let mut menu = MenuBuilder::new();
                            for (label, kind) in [
                                ("Action", SavedItemKind::Action),
                                ("Event", SavedItemKind::Event),
                            ] {
                                let view = cx.entity();
                                menu = menu.item(label, move |window, cx| {
                                    view.update(cx, |view, cx| view.add_draft(kind, window, cx));
                                });
                            }
                            open_context_menu(menu, event.position(), window, cx);
                        }
                        SavedItemsFilter::Actions => {
                            view.add_draft(SavedItemKind::Action, window, cx)
                        }
                        SavedItemsFilter::Events => {
                            view.add_draft(SavedItemKind::Event, window, cx)
                        }
                    },
                )),
            )
            .child(content);

        marquee(body, self, cx)
            .track_focus(&self.focus_handle)
            .on_drag_move::<DragData<DraggedItems>>(cx.listener(|view, _, _, cx| {
                view.set_drop_target(None, cx);
            }))
            .on_dismiss(SelectionScope::SavedItems, Some(self.focus_handle.clone()))
    }
}

fn reorder_group(current: &[Uuid], moving: &[Uuid], target: Uuid, before: bool) -> Vec<Uuid> {
    let moving: HashSet<_> = moving.iter().copied().collect();
    if moving.contains(&target) {
        return current.to_vec();
    }

    let group: Vec<_> = current
        .iter()
        .copied()
        .filter(|id| moving.contains(id))
        .collect();
    if group.is_empty() || !current.contains(&target) {
        return current.to_vec();
    }

    let mut remaining: Vec<_> = current
        .iter()
        .copied()
        .filter(|id| !moving.contains(id))
        .collect();
    let Some(target_index) = remaining.iter().position(|id| *id == target) else {
        return current.to_vec();
    };
    let insert_at = target_index + usize::from(!before);
    remaining.splice(insert_at..insert_at, group);
    remaining
}
