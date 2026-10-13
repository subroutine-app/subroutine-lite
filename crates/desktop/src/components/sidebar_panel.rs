use crate::components::ItemCard;
use crate::components::ext::InteractiveElementExt;
use crate::{
    item_manager::{DraftView, ItemManager},
    selection::SelectionManager,
    stores::{AppDatabaseStore, WorkspaceContent},
};
use gpui::{
    AnyElement, App, ClickEvent, Div, ElementId, InteractiveElement, Interactivity, IntoElement,
    MouseButton, ParentElement, Pixels, RenderOnce, SharedString, Stateful,
    StatefulInteractiveElement, StyleRefinement, Styled, Window, div, prelude::FluentBuilder as _,
    px,
};
use gpui_kit::foundation::Disableable as _;
use gpui_kit::foundation::StyledExt as _;

type ClickHandler = dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static;

pub const SIDEBAR_ITEM_HEIGHT: Pixels = px(56.);

pub const SIDEBAR_ITEM_GAP: Pixels = px(8.);

pub const SIDEBAR_GUTTER: Pixels = px(12.);

pub const SIDEBAR_MIN_WIDTH: Pixels = px(252.);

#[derive(IntoElement)]
pub struct SidebarPanel {
    body: Stateful<Div>,
    footer: Option<AnyElement>,
}

impl SidebarPanel {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            body: div().id(id),
            footer: None,
        }
    }

    pub fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }
}

impl Styled for SidebarPanel {
    fn style(&mut self) -> &mut StyleRefinement {
        self.body.style()
    }
}

impl InteractiveElement for SidebarPanel {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.body.interactivity()
    }
}

impl StatefulInteractiveElement for SidebarPanel {}

impl InteractiveElementExt for SidebarPanel {}

impl ParentElement for SidebarPanel {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.body.extend(elements);
    }
}

impl RenderOnce for SidebarPanel {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .column()
            .size_full()
            .overflow_hidden()
            .child(
                self.body
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .w_full()
                    .min_w(SIDEBAR_MIN_WIDTH),
            )
            .when_some(self.footer, |this, footer| {
                this.child(
                    div()
                        .row()
                        .flex_none()
                        .w_full()
                        .p(SIDEBAR_GUTTER)
                        .child(footer),
                )
            })
    }
}

#[derive(IntoElement)]
pub(crate) struct SidebarDraft {
    id: ElementId,
    label: SharedString,
    owner: DraftView,
    on_click: Option<Box<ClickHandler>>,
}

impl SidebarDraft {
    pub(crate) fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        owner: DraftView,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            owner,
            on_click: None,
        }
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for SidebarDraft {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let draft = ItemManager::global(cx)
            .read(cx)
            .view_draft(self.owner)
            .cloned();
        let frame = div().w_full().h(SIDEBAR_ITEM_HEIGHT).flex_none();
        if let Some(item) = draft {
            return frame.child(
                ItemCard::new_with_id(self.id, &item, None, window, cx)
                    .actionable(false)
                    .editable(true)
                    .draggable(false, None)
                    .size_full()
                    .block_mouse_except_scroll(),
            );
        }
        let ready =
            AppDatabaseStore::global(cx).read(cx).workspace_content() == WorkspaceContent::Ready;
        frame.child(
            ItemCard::create_slot(self.id, self.label, cx)
                .disabled(!ready)
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                    SelectionManager::claim_press(cx);
                    cx.stop_propagation();
                })
                .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .when_some(self.on_click, |slot, handler| {
                    slot.on_click(move |event, window, cx| {
                        cx.stop_propagation();
                        handler(event, window, cx);
                    })
                }),
        )
    }
}
