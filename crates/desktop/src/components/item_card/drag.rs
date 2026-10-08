use gpui::{
    App, IntoElement, ParentElement, Pixels, SharedString, Size, Styled, div,
    prelude::FluentBuilder, px, rems,
};
use gpui_kit::foundation::StyledExt as _;
use gpui_kit_theme::ActiveTheme;
use subroutine_core::{Action, AnyItem};
use uuid::Uuid;

use crate::{
    color::ColorExt,
    components::{DragData, Label},
    utils::ButtonColorizeExt,
};

use super::{item_icon, neutral_card_colors};

#[derive(Clone, Debug)]
pub struct DraggedItems {
    pub primary: AnyItem,
    pub source_anchor: Option<subroutine_core::SchedulePoint>,
    pub items: Vec<AnyItem>,
    pub saved_item_ids: Option<Vec<Uuid>>,
    pub materialized_item_ids: Vec<Uuid>,
}

impl DraggedItems {
    pub fn single(item: AnyItem) -> Self {
        Self {
            items: vec![item.clone()],
            primary: item,
            source_anchor: None,
            saved_item_ids: None,
            materialized_item_ids: Vec::new(),
        }
    }

    pub fn from_saved_items(primary: AnyItem, items: Vec<AnyItem>, ids: Vec<Uuid>) -> Self {
        Self {
            primary,
            source_anchor: None,
            materialized_item_ids: items.iter().map(AnyItem::id).collect(),
            items,
            saved_item_ids: Some(ids),
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn source_anchor(&self) -> Option<subroutine_core::SchedulePoint> {
        self.source_anchor.or_else(|| self.primary.start())
    }

    pub fn actions(&self) -> impl Iterator<Item = &Action> {
        self.items.iter().filter_map(|item| match item {
            AnyItem::Action(action) => Some(action),
            _ => None,
        })
    }
}

pub fn create_drag_data(
    dragged: DraggedItems,
    size: Size<Pixels>,
    cx: &App,
) -> DragData<DraggedItems> {
    let colors = neutral_card_colors(cx.theme());
    let preview_title: SharedString = dragged.primary.title().into();
    let fg = cx.theme().colors.text;
    let muted_fg = cx.theme().colors.text_muted;
    let badge_fill = cx.theme().colors.primary_fill;
    let badge_fg = cx.theme().colors.text_on_primary_fill;
    let item_type = dragged.primary.item_type();
    let count = dragged.len();
    let label = if count > 1 {
        format!("{} + {} more", dragged.primary.title(), count - 1)
    } else {
        dragged.primary.title().to_string()
    };
    DragData::new(dragged)
        .with_label(label)
        .with_preview(move || {
            let icon = item_icon(item_type).size_4().text_color(muted_fg);
            div()
                .row()
                .w(size.width)
                .h(size.height)
                .opacity(0.7)
                .button_colors(colors)
                .rounded_xl()
                .child(
                    div()
                        .row()
                        .size_full()
                        .py_2()
                        .px_4()
                        .items_start()
                        .gap_2()
                        .text_color(muted_fg)
                        .child(icon)
                        .child(
                            Label::new(preview_title.clone())
                                .text_sm()
                                .line_height(rems(1.25))
                                .text_color(muted_fg.mix(fg, 0.5))
                                .m_0()
                                .p_0()
                                .h_auto()
                                .cursor_default(),
                        ),
                )
                .when(count > 1, |this| {
                    this.child(
                        div()
                            .absolute()
                            .top(px(-6.))
                            .right(px(-6.))
                            .px_1p5()
                            .rounded_full()
                            .bg(badge_fill)
                            .text_color(badge_fg)
                            .child(Label::new(format!("{count}")).text_xs()),
                    )
                })
                .into_any_element()
        })
        .with_preview_size(size)
}
