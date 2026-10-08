use gpui::{
    AnyElement, App, Bounds, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels,
    Point, Size, Styled, Window, div, prelude::FluentBuilder, px, size,
};
use gpui_kit::foundation::{Sizable, StyledExt as _};
use gpui_kit_theme::ActiveTheme;
use subroutine_core::AnyItem;

use crate::{
    AppIcon,
    components::{Button, ButtonVariants, Checkbox, Label},
    icons::Icon,
    item_manager::ItemManager,
    selection::{FocusHandoff, SelectionManager, bulk},
    stores::AppDatabaseStore,
    utils::ButtonColors,
};

use super::{
    CardMeta, ItemCard, ItemCardTitleScale,
    availability::event_availability_indicator,
    item_icon,
    menu::{RESTORE_ACTION_LABEL, card_context_menu, restore_action_label},
    notes::{item_content, render_item_card_notes},
    render::ITEM_CARD_BORDER,
    render_dynamic_title,
};

const ITEM_CARD_END_INSET: Pixels = px(6.);
const ITEM_HEADER_CONTENT_GAP: Pixels = px(3.);

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) struct CardContentGeometry {
    size: Size<Pixels>,
}

impl CardContentGeometry {
    pub(super) fn measured(bounds: Bounds<Pixels>) -> Self {
        Self { size: bounds.size }
    }

    fn notes_size(self, header_height: Pixels, content_offset: Point<Pixels>) -> Size<Pixels> {
        size(
            (self.size.width - content_offset.x - ITEM_CARD_END_INSET).max(px(0.)),
            (self.size.height
                - content_offset.y
                - header_height
                - ITEM_HEADER_CONTENT_GAP
                - ITEM_CARD_END_INSET)
                .max(px(0.)),
        )
    }
}

impl ItemCard {
    fn completion_checkbox(&self, cx: &mut App) -> Option<AnyElement> {
        let AnyItem::Action(action) = &self.item else {
            return None;
        };
        if !self.actionable {
            return None;
        }

        let action = action.clone();
        let action_id = action.id;
        let handoff = FocusHandoff::new(
            self.selection.as_ref().map(|order| order.scope),
            action_id,
            self.next_focus.clone(),
        );
        let completed = action.is_completed();
        let is_completing = ItemManager::global(cx).read(cx).is_completing(action_id);

        Some(
            Checkbox::new((self.element_id.clone(), "complete"))
                .large()
                .tab_stop(false)
                .cursor_default()
                .checked(completed || is_completing)
                .on_click(move |checked, window, cx| {
                    cx.stop_propagation();
                    if !*checked {
                        handoff.take(window, cx);
                        AppDatabaseStore::global(cx).update(cx, |store, cx| {
                            store.uncomplete_action(action_id, cx);
                        });
                        return;
                    }
                    ItemManager::global(cx).update(cx, |handler, cx| {
                        if handler.is_completing(action_id) {
                            return;
                        }
                        handler.begin_complete_action(
                            action.clone(),
                            Some(handoff.clone()),
                            window,
                            cx,
                        );
                    })
                })
                .into_any_element(),
        )
    }

    fn render_more_action(&self, menu_enabled: bool, cx: &App) -> Option<AnyElement> {
        menu_enabled.then(|| {
            let item = self.item.clone();
            let item_id = item.id();
            let selection_scope = self.selection.as_ref().map(|order| order.scope);
            let schedule_navigation = self.schedule_navigation;
            let availability_store = self.projected_availability_store.clone();
            Button::new((self.element_id.clone(), "more-actions"))
                .tab_stop(self.tab_stop)
                .ghost()
                .xsmall()
                .compact()
                .w_6()
                .icon(AppIcon::Ellipsis)
                .tooltip("More actions")
                .text_color(cx.theme().colors.text_muted)
                .block_mouse_except_scroll()
                .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                .on_click(move |event, window, cx| {
                    cx.stop_propagation();
                    if let Some(scope) = selection_scope {
                        SelectionManager::prepare_context_menu(scope, item_id, cx);
                    }
                    let menu = card_context_menu(
                        &item,
                        selection_scope,
                        schedule_navigation,
                        availability_store.as_ref(),
                        cx,
                    );
                    crate::components::menu::open_context_menu(menu, event.position(), window, cx);
                })
                .into_any_element()
        })
    }

    fn render_details_toggle(
        &self,
        wide: bool,
        window: &Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let details = self.details.as_ref()?.clone();
        item_content(&self.item)?;
        let expanded = details.expanded();
        let owner = window.current_view();
        Some(
            Button::new((self.element_id.clone(), "details-disclosure"))
                .tab_stop(self.tab_stop)
                .ghost()
                .xsmall()
                .compact()
                .icon(if expanded {
                    AppIcon::ChevronUp
                } else {
                    AppIcon::ChevronDown
                })
                .when(wide, |button| {
                    button.label(if expanded { "Show less" } else { "Show more" })
                })
                .tooltip(if expanded {
                    "Collapse details"
                } else {
                    "Expand details"
                })
                .text_color(cx.theme().colors.text_muted)
                .block_mouse_except_scroll()
                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                    SelectionManager::claim_press(cx);
                    cx.stop_propagation();
                })
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    details.toggle();
                    cx.notify(owner);
                })
                .into_any_element(),
        )
    }

    pub(super) fn render_content(
        &mut self,
        colors: ButtonColors,
        geometry: CardContentGeometry,
        menu_enabled: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let narrow_details = self.details.is_some()
            && item_content(&self.item).is_some()
            && geometry.size.width > px(0.0)
            && geometry.size.width < px(192.0);
        if narrow_details {
            self.content_offset.x = px(2.0);
        }
        let draft_picker = self.draft_type_picker(cx);
        let has_draft_picker = draft_picker.is_some();
        let title = render_dynamic_title(
            &self.item,
            self.compact,
            self.editable,
            self.title_scale,
            window,
            cx,
        )
        .into_any_element();

        let more_action = self.render_more_action(menu_enabled && !narrow_details, cx);
        let details_toggle = self.render_details_toggle(
            !self.compact && geometry.size.width >= px(360.0),
            window,
            cx,
        );

        if self.compact {
            let completion = draft_picker.or_else(|| {
                (!narrow_details)
                    .then(|| self.completion_checkbox(cx))
                    .flatten()
            });
            let row = div()
                .row()
                .items_center()
                .gap(px(4.))
                .when_else(
                    self.positioned_content,
                    |row| {
                        row.absolute()
                            .top(self.content_offset.y)
                            .left(self.content_offset.x)
                            .right_0()
                            .h(self.title_scale.line_height())
                    },
                    |row| {
                        row.w_full()
                            .h_full()
                            .pt(self.content_offset.y)
                            .pl(self.content_offset.x)
                    },
                )
                .children(completion.map(|checkbox| {
                    div()
                        .row()
                        .flex_none()
                        .h(px(20.))
                        .w(px(if has_draft_picker { 30. } else { 20. }))
                        .items_center()
                        .justify_center()
                        .child(checkbox)
                }))
                .child(div().min_w_0().flex_1().truncate().child(title))
                .when(!narrow_details, |row| {
                    row.children(event_availability_indicator(
                        &self.item,
                        &self.element_id,
                        cx,
                    ))
                })
                .children(details_toggle)
                .when(self.projected_availability_store.is_some(), |row| {
                    row.children(more_action)
                });
            return div().relative().size_full().child(row).into_any_element();
        }

        let leading = draft_picker
            .or_else(|| self.completion_checkbox(cx))
            .unwrap_or_else(|| {
                let icon = item_icon(self.item.item_type()).text_color(colors.fg);
                match self.title_scale {
                    ItemCardTitleScale::Standard => icon.size_4(),
                    ItemCardTitleScale::Large => icon.size_5(),
                    ItemCardTitleScale::Display => icon.size_6(),
                }
                .into_any_element()
            });

        let quick_action = (self.actionable && !narrow_details)
            .then(|| {
                self.trailing.take().or_else(|| match &self.item {
                    AnyItem::Action(action) if restore_action_label(action).is_some() => {
                        let action_id = action.id;
                        let handoff = FocusHandoff::new(
                            self.selection.as_ref().map(|order| order.scope),
                            action_id,
                            self.next_focus.clone(),
                        );
                        Some(
                            Button::new(("restore-btn", action_id.as_u64_pair().1))
                                .tab_stop(self.tab_stop)
                                .ghost()
                                .xsmall()
                                .icon(AppIcon::RotateCcw)
                                .label(RESTORE_ACTION_LABEL)
                                .tooltip("Restore this action to active work")
                                .text_color(cx.theme().colors.text_muted)
                                .block_mouse_except_scroll()
                                .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    handoff.take(window, cx);
                                    AppDatabaseStore::global(cx).update(cx, |store, cx| {
                                        store.uncomplete_action(action_id, cx);
                                    });
                                })
                                .into_any_element(),
                        )
                    }
                    AnyItem::Action(action)
                        if !action.is_completed() && !action.queued && action.start.is_none() =>
                    {
                        let action_id = action.id;
                        Some(
                            Button::new(("queue-btn", action_id.as_u64_pair().1))
                                .tab_stop(self.tab_stop)
                                .ghost()
                                .xsmall()
                                .icon(AppIcon::Play)
                                .tooltip("Add to the queue")
                                .text_color(cx.theme().colors.text_muted)
                                .block_mouse_except_scroll()
                                .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                                .on_click(move |_, _window, cx| {
                                    cx.stop_propagation();
                                    AppDatabaseStore::global(cx).update(cx, |store, cx| {
                                        store.auto_queue_action(action_id, cx);
                                    });
                                })
                                .into_any_element(),
                        )
                    }
                    AnyItem::Action(action) if !action.is_completed() && action.start.is_some() => {
                        let action = action.clone();
                        let pinned = action.pinned;
                        Some(
                            Button::new(("pin-btn", action.id.as_u64_pair().1))
                                .tab_stop(self.tab_stop)
                                .ghost()
                                .xsmall()
                                .icon(AppIcon::Pin)
                                .tooltip(if pinned {
                                    "Unpin — let the schedule move this"
                                } else {
                                    "Pin to this time"
                                })
                                .text_color(if pinned {
                                    cx.theme().colors.accent
                                } else {
                                    cx.theme().colors.text_muted
                                })
                                .block_mouse_except_scroll()
                                .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                                .on_click(move |_, _window, cx| {
                                    cx.stop_propagation();
                                    bulk::set_pinned(
                                        &[AnyItem::Action(action.clone())],
                                        !pinned,
                                        cx,
                                    );
                                })
                                .into_any_element(),
                        )
                    }
                    _ => None,
                })
            })
            .flatten();

        let mut trailing: Vec<AnyElement> =
            event_availability_indicator(&self.item, &self.element_id, cx)
                .filter(|_| !narrow_details)
                .into_iter()
                .chain(quick_action)
                .collect();

        let mut meta: Vec<CardMeta> = Vec::new();
        if let Some(rule) = self.item.recurrence() {
            meta.push(CardMeta::new(rule.describe()).icon(AppIcon::Repeat));
        }
        meta.extend(self.meta.iter().cloned());

        let muted = cx.theme().colors.text_muted;
        let meta_line = (!self.title_only && !meta.is_empty()).then(|| {
            div()
                .row()
                .w_full()
                .gap_2()
                .overflow_hidden()
                .children(meta.into_iter().map(move |fact| {
                    let color = fact.color.unwrap_or(muted);
                    div()
                        .row()
                        .flex_shrink_0()
                        .gap_1()
                        .when_some(fact.icon, |this, icon| {
                            this.child(Icon::new(icon).size_3().flex_shrink_0().text_color(color))
                        })
                        .child(Label::new(fact.text).text_xs().text_color(color))
                }))
        });

        let content_source = if self.title_only {
            None
        } else {
            item_content(&self.item)
        };
        let header_height = content_source.as_ref().map(|_| {
            window.use_keyed_state(
                (self.element_id.clone(), "notes-header-height"),
                cx,
                |_, _| px(0.),
            )
        });
        let content_panel = content_source.and_then(|source| {
            let header_height = header_height
                .as_ref()
                .map_or(px(0.), |height| *height.read(cx));
            render_item_card_notes(
                self.element_id.clone(),
                source,
                geometry.notes_size(header_height, self.content_offset),
                self.details.clone(),
                header_height
                    + px(6.0)
                    + ITEM_HEADER_CONTENT_GAP
                    + ITEM_CARD_END_INSET
                    + ITEM_CARD_BORDER * 2.0,
                window,
                cx,
            )
        });

        trailing.extend(details_toggle);
        trailing.extend(more_action);
        let trailing_controls = (!trailing.is_empty()).then(|| {
            div().row().flex_none().gap(px(2.)).children(
                trailing
                    .into_iter()
                    .map(|control| div().flex_none().child(control)),
            )
        });
        let header = div()
            .row()
            .w_full()
            .flex_none()
            .when_else(
                self.border,
                |this| this.items_start(),
                |this| this.items_center(),
            )
            .gap(if narrow_details {
                px(2.0)
            } else {
                self.title_scale.header_gap()
            })
            .when(!narrow_details || has_draft_picker, |header| {
                header.child(
                    div()
                        .row()
                        .flex_none()
                        .h(self.title_scale.leading_extent())
                        .w(self.title_scale.leading_extent()
                            + px(if has_draft_picker { 10. } else { 0. }))
                        .items_center()
                        .justify_center()
                        .child(leading),
                )
            })
            .child(
                div()
                    .column()
                    .flex_1()
                    .min_w(px(0.))
                    .gap(px(1.))
                    .overflow_hidden()
                    .child(div().row().w_full().min_w(px(0.)).child(title))
                    .children(meta_line),
            )
            .children(trailing_controls)
            .when_some(header_height, |header, height| {
                let owner = window.current_view();
                header.on_children_prepainted(move |children, window, cx| {
                    let measured = children
                        .iter()
                        .map(|bounds| bounds.size.height)
                        .fold(px(0.), |height, child| height.max(child));
                    if *height.read(cx) != measured {
                        height.update(cx, |height, _| *height = measured);
                        window.on_next_frame(move |_, cx| cx.notify(owner));
                    }
                })
            });

        div()
            .column()
            .relative()
            .w_full()
            .h_full()
            .when_else(
                self.border
                    || self
                        .details
                        .as_ref()
                        .is_some_and(|details| details.is_open()),
                |this| this.justify_start(),
                |this| this.justify_center(),
            )
            .gap(ITEM_HEADER_CONTENT_GAP)
            .pl(self.content_offset.x)
            .pr(ITEM_CARD_END_INSET)
            .pt(self.content_offset.y)
            .pb(ITEM_CARD_END_INSET)
            .child(header)
            .children(content_panel)
            .into_any_element()
    }
}
