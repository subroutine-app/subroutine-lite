use gpui::{
    AnyElement, App, InteractiveElement, IntoElement, ParentElement, Styled, Window, div, px,
};
use gpui_kit_semantics::{NodeSpec, Role, Semantic as _};
use uuid::Uuid;

use crate::{
    AppIcon,
    components::{
        Button, ButtonVariants,
        ext::ElementExt as _,
        menu::{MenuBuilder, open_context_menu_at_bounds},
    },
    icons::Icon,
    item_manager::{DraftAsAction, DraftAsEvent, DraftType, ItemManager},
};

use super::{ItemCard, ItemCardTitleScale, item_icon};

pub(super) fn open_type_menu(id: Uuid, window: &mut Window, cx: &mut App) {
    let manager = ItemManager::global(cx);
    let Some(current) = manager.read(cx).draft_type(id) else {
        return;
    };
    let Some(bounds) = manager
        .read(cx)
        .editing_item
        .as_ref()
        .and_then(|editing| editing.draft_type_bounds)
    else {
        return;
    };
    let menu = MenuBuilder::new()
        .label("Draft item type")
        .check_with_keybinding(
            "Action",
            current == DraftType::Action,
            DraftAsAction,
            move |window, cx| {
                ItemManager::global(cx).update(cx, |manager, cx| {
                    manager.set_draft_type(id, DraftType::Action, window, cx);
                });
            },
        )
        .check_with_keybinding(
            "Event",
            current == DraftType::Event,
            DraftAsEvent,
            move |window, cx| {
                ItemManager::global(cx).update(cx, |manager, cx| {
                    manager.set_draft_type(id, DraftType::Event, window, cx);
                });
            },
        );
    open_context_menu_at_bounds(menu, bounds, window, cx);
}

impl ItemCard {
    pub(super) fn draft_type_picker(&self, cx: &mut App) -> Option<AnyElement> {
        if !self.editable {
            return None;
        }
        let id = self.item.id();
        let kind = ItemManager::global(cx).read(cx).draft_type(id)?;
        let label = format!("{} draft — Change item type", kind.label());
        let icon = item_icon(kind.item_type());
        let icon = match self.title_scale {
            ItemCardTitleScale::Standard => icon.size_4(),
            ItemCardTitleScale::Large => icon.size_5(),
            ItemCardTitleScale::Display => icon.size_6(),
        };
        Some(
            div()
                .flex_none()
                .on_prepaint(move |measured, _, cx| {
                    ItemManager::global(cx).update(cx, |manager, _| {
                        if let Some(editing) = manager
                            .editing_item
                            .as_mut()
                            .filter(|editing| editing.id() == id)
                        {
                            editing.draft_type_bounds = Some(measured);
                        }
                    });
                })
                .child(
                    Button::new((self.element_id.clone(), "draft-type"))
                        .ghost()
                        .xsmall()
                        .compact()
                        .tab_stop(false)
                        .h(self.title_scale.leading_extent())
                        .w(self.title_scale.leading_extent() + px(10.))
                        .gap(px(2.))
                        .icon(icon)
                        .child(Icon::new(AppIcon::ChevronDown).size(px(8.)))
                        .tooltip(label.clone())
                        .block_mouse_except_scroll()
                        .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            open_type_menu(id, window, cx);
                        }),
                )
                .semantic_in(
                    cx,
                    NodeSpec::new(format!("item-draft.{id}.type"), Role::Button).text(label),
                )
                .into_any_element(),
        )
    }
}
