use std::{collections::HashMap, ops::Range};

use chrono::{Duration as ChronoDuration, Utc};
use gpui::{
    AnyElement, App, FocusHandle, FontWeight, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder, px,
};
use gpui_kit::foundation::StyledExt as _;
use gpui_kit_theme::ActiveTheme;
use subroutine_core::{ActionTemplate, AnyItem, EventTemplate};
use uuid::Uuid;

use super::{SavedItemDropTarget, SavedItemKind, SavedItemsFilter, SavedItemsView};
use crate::{
    AppIcon,
    components::{
        CardMeta, DragData, DraggedItems, EmptyState, ItemCard, ItemCardStates, Label,
        SIDEBAR_GUTTER, SIDEBAR_ITEM_GAP, SIDEBAR_ITEM_HEIGHT, menu::MenuBuilder,
        virtual_list::ScrollFocus,
    },
    icons::Icon,
    item_manager::ItemManager,
    item_subject::SavedItem,
    selection::{
        DeleteSelected, DuplicateSelected, SelectionManager, SelectionOrder, SelectionScope, bulk,
        focus_item, focus_item_extending,
    },
    stores::AppDatabaseStore,
    views::OpenSavedItemInspector,
};

const ITEM_HEIGHT: gpui::Pixels = SIDEBAR_ITEM_HEIGHT;

fn duration_str(duration: ChronoDuration) -> Option<SharedString> {
    let total_minutes = duration.num_minutes();
    if total_minutes <= 0 {
        return None;
    }
    let hours = total_minutes / 60;
    let minutes = total_minutes % 60;
    Some(
        match (hours, minutes) {
            (0, m) => format!("{m}m"),
            (h, 0) => format!("{h}h"),
            (h, m) => format!("{h}h {m}m"),
        }
        .into(),
    )
}

fn delete_saved_items(ids: Vec<Uuid>, window: &mut Window, cx: &mut App) {
    bulk::delete_saved(&ids, window, cx);
    SelectionManager::clear_global(cx);
}

fn create_saved_items(items: Vec<AnyItem>, cx: &mut App) {
    AppDatabaseStore::global(cx).update(cx, |store, cx| {
        store.create_items(items, cx);
    });
}

pub(crate) fn saved_items_context_menu(templates: Vec<AnyItem>) -> MenuBuilder {
    let ids: Vec<_> = templates.iter().map(AnyItem::id).collect();
    let items: Vec<_> = templates
        .iter()
        .filter_map(|item| match item.clone() {
            AnyItem::ActionTemplate(template) => Some(AnyItem::Action(template.build())),
            AnyItem::EventTemplate(template) => Some(AnyItem::Event(template.build(Utc::now()))),
            _ => None,
        })
        .collect();
    let count = ids.len();
    let inspect_id = ids.first().copied();
    let duplicate = templates.clone();
    MenuBuilder::new()
        .when_some((count == 1).then_some(inspect_id).flatten(), |menu, id| {
            menu.item("Edit saved item", move |window, cx| {
                window.dispatch_action(Box::new(OpenSavedItemInspector(id)), cx);
            })
        })
        .item(
            if count == 1 {
                "Create item".to_owned()
            } else {
                format!("Create {count} items")
            },
            move |_, cx| create_saved_items(items.clone(), cx),
        )
        .item_with_keybinding(
            if count == 1 {
                "Duplicate saved item".to_owned()
            } else {
                format!("Duplicate {count} saved items")
            },
            DuplicateSelected,
            move |_, cx| {
                bulk::duplicate(&duplicate, cx);
            },
        )
        .separator()
        .item_with_keybinding(
            if count == 1 {
                "Delete saved item".to_owned()
            } else {
                format!("Delete {count} saved items")
            },
            DeleteSelected,
            move |window, cx| delete_saved_items(ids.clone(), window, cx),
        )
        .when(count > 1, |menu| {
            menu.item("Clear selection", |_, cx| {
                SelectionManager::clear_global(cx)
            })
        })
}

pub(super) fn render_draft(
    draft: &SavedItem,
    details: &ItemCardStates,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let item = AnyItem::from(draft.clone());
    let height = details.height(draft.id(), ITEM_HEIGHT);
    let duration = match draft {
        SavedItem::Action(template) => template.duration,
        SavedItem::Event(template) => Some(template.duration),
    };
    let now = Utc::now();
    let meta = duration
        .and_then(|duration| duration_str((now + duration) - now))
        .map(|text| CardMeta::new(text).icon(AppIcon::Clock));

    div()
        .id(("saved-item-draft", draft.id().as_u64_pair().1))
        .h(height + SIDEBAR_ITEM_GAP)
        .flex_none()
        .px(SIDEBAR_GUTTER)
        .pb(SIDEBAR_ITEM_GAP)
        .child(
            ItemCard::new_with_id(
                ("saved-item-card", draft.id().as_u64_pair().1),
                &item,
                None,
                window,
                cx,
            )
            .details(details.get(draft.id()))
            .meta(meta.into_iter().collect())
            .border(false)
            .draggable(false, None)
            .block_mouse_except_scroll()
            .w_full()
            .h(height)
            .flex_none(),
        )
}

pub struct SavedItemsList {
    pub(super) details: ItemCardStates,
    pub action_templates: Vec<ActionTemplate>,
    pub event_templates: Vec<EventTemplate>,
    pub filter: SavedItemsFilter,
    pub query: String,
    pub loading: bool,
}

impl SavedItemsList {
    pub fn new() -> Self {
        Self {
            details: ItemCardStates::default(),
            action_templates: vec![],
            event_templates: vec![],
            filter: SavedItemsFilter::All,
            query: String::new(),
            loading: true,
        }
    }

    fn hit(&self, title: &str) -> Option<Option<Range<usize>>> {
        if self.query.trim().is_empty() {
            return Some(None);
        }
        let needle = self.query.trim().to_lowercase();
        let start = title.to_lowercase().find(&needle)?;
        Some(Some(start..start + needle.len()))
    }

    fn matching_actions(&self) -> Vec<(&ActionTemplate, Option<Range<usize>>)> {
        if self.filter == SavedItemsFilter::Events {
            return Vec::new();
        }
        self.action_templates
            .iter()
            .filter_map(|template| Some((template, self.hit(&template.title)?)))
            .collect()
    }

    fn matching_events(&self) -> Vec<(&EventTemplate, Option<Range<usize>>)> {
        if self.filter == SavedItemsFilter::Actions {
            return Vec::new();
        }
        self.event_templates
            .iter()
            .filter_map(|template| Some((template, self.hit(&template.title)?)))
            .collect()
    }

    pub fn matching_ids(&self) -> Vec<Uuid> {
        self.matching_actions()
            .into_iter()
            .map(|(template, _)| template.id)
            .chain(
                self.matching_events()
                    .into_iter()
                    .map(|(template, _)| template.id),
            )
            .collect()
    }

    pub fn empty_state(&self) -> Option<EmptyState> {
        if self.loading {
            return Some(EmptyState::new(
                Icon::new(AppIcon::Archive),
                "Loading saved items…",
            ));
        }
        if !self.matching_actions().is_empty() || !self.matching_events().is_empty() {
            return None;
        }
        let title = if !self.query.trim().is_empty() {
            "No matches"
        } else {
            match self.filter {
                SavedItemsFilter::Actions => "No saved actions",
                SavedItemsFilter::Events => "No saved events",
                SavedItemsFilter::All => "Nothing saved",
            }
        };
        Some(EmptyState::new(Icon::new(AppIcon::Archive), title))
    }

    fn section_header(&self, label: &'static str, cx: &App) -> impl IntoElement {
        div()
            .row()
            .w_full()
            .pt_3()
            .pb_1()
            .px(SIDEBAR_GUTTER + px(4.))
            .child(
                Label::new(label)
                    .h_auto()
                    .text_xs()
                    .text_color(cx.theme().colors.text_muted)
                    .font_weight(FontWeight::BOLD),
            )
    }

    fn built_item(&self, id: Uuid) -> Option<AnyItem> {
        if let Some(template) = self.action_templates.iter().find(|item| item.id == id) {
            return Some(AnyItem::Action(template.clone().build()));
        }
        self.event_templates
            .iter()
            .find(|item| item.id == id)
            .map(|template| AnyItem::Event(template.clone().build(Utc::now())))
    }

    fn selected_ids_for(&self, primary_id: Uuid, cx: &App) -> Vec<Uuid> {
        let manager = SelectionManager::global(cx);
        let selection = manager.read(cx);
        let visible = self.matching_ids();
        if selection.is_selected_in(SelectionScope::SavedItems, primary_id) {
            let selected: Vec<_> = visible
                .into_iter()
                .filter(|id| selection.ids().contains(id))
                .collect();
            if !selected.is_empty() {
                return selected;
            }
        }
        vec![primary_id]
    }

    fn drag_payload(&self, primary_id: Uuid, cx: &App) -> DraggedItems {
        let ids = self.selected_ids_for(primary_id, cx);
        let items: Vec<_> = ids.iter().filter_map(|id| self.built_item(*id)).collect();
        let primary = ids
            .iter()
            .position(|id| *id == primary_id)
            .and_then(|index| items.get(index))
            .cloned()
            .expect("a rendered saved item can be built");
        DraggedItems::from_saved_items(primary, items, ids)
    }

    pub fn render(
        &self,
        view: gpui::Entity<SavedItemsView>,
        focus_handles: &HashMap<Uuid, FocusHandle>,
        drop_target: Option<SavedItemDropTarget>,
        window: &mut Window,
        cx: &mut gpui::Context<SavedItemsView>,
    ) -> impl IntoElement {
        let actions = self.matching_actions();
        let events = self.matching_events();
        let order = SelectionOrder::new(SelectionScope::SavedItems, self.matching_ids());

        div()
            .column()
            .w_full()
            .when(!actions.is_empty(), |this| {
                this.child(self.section_header("Actions", cx))
                    .children(actions.iter().filter_map(|(template, _)| {
                        let handle = focus_handles.get(&template.id)?.clone();
                        Some(self.render_action_template(
                            template,
                            handle,
                            order.clone(),
                            view.clone(),
                            drop_target,
                            window,
                            cx,
                        ))
                    }))
            })
            .when(!events.is_empty(), |this| {
                this.child(self.section_header("Events", cx))
                    .children(events.iter().filter_map(|(template, _)| {
                        let handle = focus_handles.get(&template.id)?.clone();
                        Some(self.render_event_template(
                            template,
                            handle,
                            order.clone(),
                            view.clone(),
                            drop_target,
                            window,
                            cx,
                        ))
                    }))
            })
    }

    #[allow(clippy::too_many_arguments)]
    fn render_action_template(
        &self,
        template: &ActionTemplate,
        focus_handle: FocusHandle,
        order: SelectionOrder,
        view: gpui::Entity<SavedItemsView>,
        drop_target: Option<SavedItemDropTarget>,
        window: &mut Window,
        cx: &mut gpui::Context<SavedItemsView>,
    ) -> AnyElement {
        let template = template.clone();
        let now = Utc::now();
        let meta = template
            .duration
            .and_then(|duration| duration_str((now + duration) - now));
        let drag_payload = self.drag_payload(template.id, cx);

        self.render_row(
            SavedItem::Action(template.clone()),
            SavedItemKind::Action,
            template.id,
            meta.map(|text| (AppIcon::Clock, text)),
            focus_handle,
            order,
            drag_payload,
            view,
            drop_target,
            window,
            cx,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn render_event_template(
        &self,
        template: &EventTemplate,
        focus_handle: FocusHandle,
        order: SelectionOrder,
        view: gpui::Entity<SavedItemsView>,
        drop_target: Option<SavedItemDropTarget>,
        window: &mut Window,
        cx: &mut gpui::Context<SavedItemsView>,
    ) -> AnyElement {
        let template = template.clone();
        let now = Utc::now();
        let meta = duration_str((now + template.duration) - now);
        let drag_payload = self.drag_payload(template.id, cx);

        self.render_row(
            SavedItem::Event(template.clone()),
            SavedItemKind::Event,
            template.id,
            meta.map(|text| (AppIcon::Clock, text)),
            focus_handle,
            order,
            drag_payload,
            view,
            drop_target,
            window,
            cx,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn render_row(
        &self,
        saved_item: SavedItem,
        kind: SavedItemKind,
        id: Uuid,
        meta: Option<(AppIcon, SharedString)>,
        focus_handle: FocusHandle,
        order: SelectionOrder,
        drag_payload: DraggedItems,
        view: gpui::Entity<SavedItemsView>,
        drop_target: Option<SavedItemDropTarget>,
        window: &mut Window,
        cx: &mut gpui::Context<SavedItemsView>,
    ) -> AnyElement {
        let selected_ids = self.selected_ids_for(id, cx);
        let keyboard_items: Vec<_> = selected_ids
            .iter()
            .filter_map(|selected_id| self.built_item(*selected_id))
            .collect();
        let reorder_target = drop_target.filter(|target| target.kind == kind && target.id == id);
        let section_ids: Vec<_> = match kind {
            SavedItemKind::Action => self.action_templates.iter().map(|item| item.id).collect(),
            SavedItemKind::Event => self.event_templates.iter().map(|item| item.id).collect(),
        };
        let element_id = ("saved-item", id.as_u64_pair().1);
        let focus_order = order.clone();
        let keyboard_view = view.clone();
        let item: AnyItem = saved_item.into();
        let height = self.details.height(id, ITEM_HEIGHT);
        let scroll_focus = ScrollFocus::new(gpui::ElementId::View(view.entity_id()), window, cx);
        let scroll_view = view.clone();

        div()
            .id(element_id)
            .focusable()
            .tab_stop(false)
            .on_focus_resolved(move |bounds, row, window, cx| {
                let scroll = scroll_view.read(cx).scroll_handle.clone();
                scroll_focus.reveal(
                    bounds,
                    row,
                    &scroll,
                    crate::views::LIBRARY_HEADER_HEIGHT + SIDEBAR_ITEM_GAP,
                    window,
                    cx,
                );
            })
            .relative()
            .h(height + SIDEBAR_ITEM_GAP)
            .flex_none()
            .px(SIDEBAR_GUTTER)
            .pb(SIDEBAR_ITEM_GAP)
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                if event.is_held || ItemManager::global(cx).read(cx).is_being_edited(id) {
                    return;
                }
                let Some(position) = focus_order
                    .ids()
                    .iter()
                    .position(|candidate| *candidate == id)
                else {
                    return;
                };
                let key = event.keystroke.key.as_str();
                let extend_selection =
                    event.keystroke.modifiers.shift && matches!(key, "up" | "down");
                let next = match key {
                    "up" | "k" => position.checked_sub(1),
                    "down" | "j" => {
                        (position + 1 < focus_order.ids().len()).then_some(position + 1)
                    }
                    "enter" => {
                        cx.stop_propagation();
                        create_saved_items(keyboard_items.clone(), cx);
                        None
                    }
                    _ => None,
                };
                let Some(next) = next else {
                    return;
                };
                let next_id = focus_order.ids()[next];
                let handle = keyboard_view
                    .read(cx)
                    .item_focus_handles
                    .get(&next_id)
                    .cloned();
                if let Some(handle) = handle {
                    cx.stop_propagation();
                    if extend_selection {
                        focus_item_extending(&focus_order, id, next_id, &handle, window, cx);
                    } else {
                        focus_item(SelectionScope::SavedItems, next_id, &handle, window, cx);
                    }
                }
            })
            .on_drag_move::<DragData<DraggedItems>>({
                let view = view.clone();
                move |event, _, cx| {
                    if !event.bounds.contains(&event.event.position) {
                        return;
                    }
                    let Some(source_ids) = event.drag(cx).data.saved_item_ids.as_ref() else {
                        return;
                    };
                    if !source_ids.iter().any(|source| section_ids.contains(source)) {
                        return;
                    }
                    let before = event.event.position.y
                        < event.bounds.origin.y + event.bounds.size.height / 2.;
                    view.update(cx, |view, cx| {
                        view.set_drop_target(Some(SavedItemDropTarget { kind, id, before }), cx)
                    });
                }
            })
            .on_drop::<DragData<DraggedItems>>({
                let view = view.clone();
                move |data, window, cx| {
                    let Some(source_ids) = data.data.saved_item_ids.as_ref() else {
                        return;
                    };
                    view.update(cx, |view, cx| {
                        let Some(target) = view.drop_target else {
                            return;
                        };
                        if target.kind == kind && target.id == id {
                            view.commit_reorder(source_ids, target, window, cx);
                            cx.stop_propagation();
                        }
                    });
                }
            })
            .when_some(reorder_target, |this, target| {
                this.child(
                    div()
                        .absolute()
                        .left(SIDEBAR_GUTTER)
                        .right(SIDEBAR_GUTTER)
                        .when(target.before, |line| line.top_0())
                        .when(!target.before, |line| line.bottom(SIDEBAR_ITEM_GAP))
                        .h(px(2.))
                        .rounded_full()
                        .bg(cx.theme().colors.accent),
                )
            })
            .child(
                crate::components::ItemCard::new_with_id(
                    ("saved-item-card", id.as_u64_pair().1),
                    &item,
                    meta.as_ref().map(|(_, text)| text.clone()),
                    window,
                    cx,
                )
                .details(self.details.get(id))
                .focus_handle(focus_handle)
                .selectable(order)
                .drag_payload(drag_payload)
                .title_only(true)
                .border(false)
                .w_full()
                .h(height)
                .flex_none(),
            )
            .into_any_element()
    }
}
