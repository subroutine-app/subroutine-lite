mod availability;
mod content;
mod details;
mod draft_type;
mod drag;
mod menu;
mod notes;
mod render;
mod title;

use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, Div, ElementId, Entity, FocusHandle, Focusable, Hsla,
    InteractiveElement, Interactivity, IntoElement, KeyBinding, ParentElement, Pixels, Point,
    SharedString, Size, Stateful, StatefulInteractiveElement, StyleRefinement, Styled, Window,
    actions, div, point, px, size,
};
use gpui_kit::foundation::Selectable;
use gpui_kit_theme::{ActiveTheme, Theme};
use subroutine_core::{AnyItem, ItemType};
use uuid::Uuid;

use crate::{
    AppIcon, icons::Icon, selection::SelectionOrder, stores::AppDatabaseStore, utils::ButtonColors,
};

pub use details::{ItemCardDetails, ItemCardStates};
pub use drag::{DraggedItems, create_drag_data};
pub(crate) use menu::item_context_menu;
pub(super) use title::{ItemCardTitleScale, render_dynamic_title};

actions!(item_card, [ToggleItemDetails]);

pub(super) fn init(cx: &mut App) {
    crate::keys::init_draft_shortcuts(cx);
    cx.bind_keys([KeyBinding::new(
        "alt-enter",
        ToggleItemDetails,
        Some("ItemCardDetails"),
    )]);
}

pub const DEFAULT_ITEM_HEIGHT: Pixels = px(16. * 4.);
pub const DEFAULT_ITEM_WIDTH: Pixels = px(64. * 4.);

type ItemCardClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

fn neutral_card_colors(theme: &Theme) -> ButtonColors {
    ButtonColors {
        bg: theme.colors.raised,
        fg: theme.colors.text,
        hover: theme.colors.raised.blend(theme.colors.hover),
        active: theme.colors.raised.blend(theme.colors.active),
        border: Some(theme.colors.hairline),
    }
}

pub fn item_icon(item: ItemType) -> Icon {
    match item {
        ItemType::Action | ItemType::ActionTemplate => Icon::new(AppIcon::Check),
        ItemType::Event | ItemType::EventTemplate => Icon::new(AppIcon::CalendarClock),
        ItemType::Routine => Icon::new(AppIcon::Repeat),
        ItemType::Marker => Icon::new(AppIcon::Calendar),
        ItemType::Signal => Icon::new(AppIcon::Cable),
    }
}

pub fn card_background(cx: &App) -> Hsla {
    cx.theme().colors.raised
}

#[derive(Clone)]
pub struct CardMeta {
    text: SharedString,
    icon: Option<AppIcon>,
    color: Option<Hsla>,
}

impl CardMeta {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            icon: None,
            color: None,
        }
    }

    pub fn icon(mut self, icon: AppIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

#[derive(IntoElement)]
pub struct ItemCard {
    base: Stateful<Div>,
    item: AnyItem,
    compact: bool,
    title_only: bool,
    details: Option<ItemCardDetails>,
    tab_stop: bool,
    navigation_context: Option<&'static str>,
    border: bool,
    editable: bool,
    title_scale: ItemCardTitleScale,
    actionable: bool,
    projected_availability_store: Option<Entity<AppDatabaseStore>>,
    schedule_navigation: bool,
    selected: bool,
    element_id: ElementId,
    meta: Vec<CardMeta>,
    trailing: Option<AnyElement>,
    bg: Hsla,
    focus_handle: FocusHandle,
    content_offset: Point<Pixels>,
    positioned_content: bool,
    draggable: bool,
    drag_payload: Option<DraggedItems>,
    drag_size: Size<Pixels>,
    selection: Option<SelectionOrder>,
    next_focus: Option<(Uuid, FocusHandle)>,
    on_click: Option<ItemCardClickHandler>,
}

impl InteractiveElement for ItemCard {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for ItemCard {}

impl Selectable for ItemCard {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

impl Styled for ItemCard {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl ParentElement for ItemCard {
    fn extend(&mut self, elements: impl IntoIterator<Item = gpui::AnyElement>) {
        self.base.extend(elements);
    }
}

impl Focusable for ItemCard {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ItemCard {
    pub fn new(
        item: &AnyItem,
        meta_text: Option<SharedString>,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        Self::new_with_id(("item-card", item.id_u64()), item, meta_text, window, cx)
    }

    pub fn new_with_id(
        element_id: impl Into<ElementId>,
        item: &AnyItem,
        meta_text: Option<SharedString>,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let content_offset = point(px(8.), px(6.));
        let element_id = element_id.into();
        let focus_handle = window
            .use_keyed_state((element_id.clone(), "focus"), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();

        Self {
            base: div().id(element_id.clone()),
            item: item.clone(),
            compact: false,
            title_only: false,
            details: None,
            tab_stop: true,
            navigation_context: None,
            border: true,
            editable: true,
            title_scale: ItemCardTitleScale::Standard,
            actionable: true,
            projected_availability_store: None,
            schedule_navigation: false,
            selected: false,
            element_id,
            meta: meta_text.map(CardMeta::new).into_iter().collect(),
            trailing: None,
            bg: gpui::transparent_black(),
            focus_handle,
            content_offset,
            positioned_content: false,
            draggable: false,
            drag_payload: None,
            drag_size: size(DEFAULT_ITEM_WIDTH, DEFAULT_ITEM_HEIGHT),
            selection: None,
            next_focus: None,
            on_click: None,
        }
    }

    pub fn next_focus(mut self, next: Option<(Uuid, FocusHandle)>) -> Self {
        self.next_focus = next;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    pub fn selectable(mut self, order: SelectionOrder) -> Self {
        self.selection = Some(order);
        self
    }

    pub fn draggable(mut self, draggable: bool, size_opt: Option<Size<Pixels>>) -> Self {
        self.draggable = draggable;
        if let Some(size) = size_opt {
            self.drag_size = size;
        }
        self
    }

    pub fn drag_payload(mut self, payload: DraggedItems) -> Self {
        self.draggable = true;
        self.drag_payload = Some(payload);
        self
    }

    pub fn focus_handle(mut self, focus_handle: FocusHandle) -> Self {
        self.focus_handle = focus_handle;
        self
    }

    pub fn compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        if compact {
            self.content_offset = point(px(2.), px(0.))
        }
        self
    }

    pub fn title_only(mut self, title_only: bool) -> Self {
        self.title_only = title_only;
        if title_only {
            self.compact = false;
            self.content_offset = point(px(8.), px(6.));
        }
        self
    }

    pub fn details(mut self, details: ItemCardDetails) -> Self {
        self.details = Some(details);
        self
    }

    pub fn content_top(mut self, top: Pixels) -> Self {
        self.content_offset.y = top;
        self.positioned_content = true;
        self
    }

    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    pub fn navigation_context(mut self, context: &'static str) -> Self {
        self.navigation_context = Some(context);
        self
    }

    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    pub fn editable(mut self, editable: bool) -> Self {
        self.editable = editable;
        self
    }

    pub fn large_title(mut self, large: bool) -> Self {
        self.title_scale = if large {
            ItemCardTitleScale::Large
        } else {
            ItemCardTitleScale::Standard
        };
        self
    }

    pub fn display_title(mut self) -> Self {
        self.title_scale = ItemCardTitleScale::Display;
        self
    }

    pub fn actionable(mut self, actionable: bool) -> Self {
        self.actionable = actionable;
        if !actionable {
            self.editable = false;
            self.tab_stop = false;
            self.draggable = false;
        }
        self
    }

    pub fn projected_event_availability(mut self, store: Entity<AppDatabaseStore>) -> Self {
        self = self.actionable(false);
        self.projected_availability_store = Some(store);
        self
    }

    pub fn schedule_navigation(mut self) -> Self {
        self.schedule_navigation = true;
        self
    }

    pub fn bg(mut self, bg: Hsla) -> Self {
        self.bg = bg;
        self
    }

    pub fn with_focus_handle(mut self, handle: FocusHandle) -> Self {
        self.focus_handle = handle;
        self
    }

    pub fn meta(mut self, meta: Vec<CardMeta>) -> Self {
        self.meta = meta;
        self
    }

    pub fn trailing(mut self, trailing: impl IntoElement) -> Self {
        self.trailing = Some(trailing.into_any_element());
        self
    }
}
