use crate::components::Button;
use crate::components::ButtonVariants as _;
use crate::components::ext::InteractiveElementExt;
use crate::icons::Icon;
use gpui::{
    AnyElement, App, ClickEvent, Div, ElementId, InteractiveElement, Interactivity, IntoElement,
    ParentElement, Pixels, RenderOnce, SharedString, Stateful, StatefulInteractiveElement,
    StyleRefinement, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_kit::foundation::Sizable as _;
use gpui_kit_theme::ActiveTheme;

use crate::AppIcon;
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
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
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
                        .border_t_1()
                        .border_color(cx.theme().colors.hairline)
                        .child(footer),
                )
            })
    }
}

#[derive(IntoElement)]
pub struct SidebarAddButton {
    id: ElementId,
    label: SharedString,
    tooltip: Option<SharedString>,
    on_click: Option<Box<ClickHandler>>,
}

impl SidebarAddButton {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            tooltip: None,
            on_click: None,
        }
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for SidebarAddButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        Button::new(self.id)
            .ghost()
            .small()
            .w_full()
            .icon(Icon::new(AppIcon::Plus).size_4())
            .label(self.label)
            .text_color(cx.theme().colors.text_muted)
            .bg(cx.theme().colors.raised)
            .hover(|this| this.bg(cx.theme().colors.hover))
            .occlude()
            .when_some(self.tooltip, |this, tooltip| this.tooltip(tooltip))
            .when_some(self.on_click, |this, handler| this.on_click(handler))
    }
}
