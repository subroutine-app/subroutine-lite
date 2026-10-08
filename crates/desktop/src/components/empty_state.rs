use crate::icons::Icon;
use gpui::{
    AnyElement, App, FontWeight, IntoElement, ParentElement, RenderOnce, SharedString, Styled,
    Window, div, px,
};
use gpui_kit::foundation::StyledExt as _;
use gpui_kit_theme::ActiveTheme;

#[derive(IntoElement)]
pub struct EmptyState {
    icon: Icon,
    title: SharedString,

    children: Vec<AnyElement>,
}

impl EmptyState {
    pub fn new(icon: Icon, title: impl Into<SharedString>) -> Self {
        Self {
            icon,
            title: title.into(),

            children: Vec::new(),
        }
    }
}

impl ParentElement for EmptyState {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for EmptyState {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .items_center()
            .justify_center()
            .p_6()
            .child(
                div()
                    .column()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .min_w_0()
                    .max_w(px(280.))
                    .text_sm()
                    .text_center()
                    .child(
                        self.icon
                            .size_6()
                            .mb_2()
                            .text_color(cx.theme().colors.text_muted),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_color(cx.theme().colors.text)
                            .font_weight(FontWeight::MEDIUM)
                            .child(self.title),
                    )
                    .children(self.children),
            )
    }
}
