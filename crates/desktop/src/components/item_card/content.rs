use gpui::{
    AnyElement, App, Bounds, FocusHandle, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Pixels, Size, Styled, Window, div, prelude::FluentBuilder, px, size,
};
use gpui_kit::foundation::{Sizable, StyledExt as _};
use gpui_kit_theme::ActiveTheme;
use subroutine_core::AnyItem;

use crate::{
    AppIcon,
    components::{
        Button, ButtonVariants, Checkbox, Label,
        transition::{self, WindowTransitionExt as _},
    },
    icons::Icon,
    item_manager::ItemManager,
    selection::{FocusHandoff, SelectionManager, bulk},
    stores::AppDatabaseStore,
    utils::ButtonColors,
};

use super::{
    CardMeta, CardPresentation, DETAILS_FOOTER_EXTENT, DETAILS_FOOTER_HEIGHT, DETAILS_FOOTER_INSET,
    ItemCard, ItemCardDetails, ItemCardTitleScale, PREVIEW_HOVER_GROUP,
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

    fn notes_size(
        self,
        top: Pixels,
        header_height: Pixels,
        bottom_inset: Pixels,
        left_inset: Pixels,
    ) -> Size<Pixels> {
        size(
            (self.size.width - left_inset - ITEM_CARD_END_INSET).max(px(0.)),
            (self.size.height - top - header_height - ITEM_HEADER_CONTENT_GAP - bottom_inset)
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
        let scope = self.selection.as_ref().map(|order| order.scope);
        let handoff = FocusHandoff::new(scope, action_id, self.next_focus.clone());
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
                        let saved = AppDatabaseStore::global(cx).update(cx, |store, cx| {
                            store.uncomplete_action(action_id, cx).is_ok()
                        });
                        if saved {
                            handoff.take(window, cx);
                        }
                        return;
                    }
                    ItemManager::global(cx).update(cx, |handler, cx| {
                        if handler.is_completing(action_id) {
                            return;
                        }
                        handler.begin_complete_action(action.clone(), scope, window, cx);
                    })
                })
                .into_any_element(),
        )
    }

    fn render_more_action(
        &self,
        menu_enabled: bool,
        pointer_inside: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        menu_enabled.then(|| {
            let item = self.item.clone();
            let item_id = item.id();
            let selection_scope = self.selection.as_ref().map(|order| order.scope);
            let schedule_navigation = self.schedule_navigation;
            let availability_store = self.projected_availability_store.clone();
            let reveal = window.keyed_transition(
                (self.element_id.clone(), "more-actions-reveal"),
                cx,
                transition::QUICK,
                || 0.0,
            );
            reveal.set(if pointer_inside { 1.0 } else { 0.0 }, cx);
            let opacity = reveal.animate(window, cx);
            Button::new((self.element_id.clone(), "more-actions"))
                .tab_stop(self.tab_stop)
                .ghost()
                .xsmall()
                .compact()
                .w_6()
                .icon(AppIcon::Ellipsis)
                .tooltip("More actions")
                .text_color(cx.theme().colors.text_muted)
                .opacity(opacity)
                .focus(|style| style.opacity(1.0))
                .when(self.presentation != CardPresentation::Preview, |button| {
                    button.block_mouse_except_scroll()
                })
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
        in_footer: bool,
        labelled: bool,
        notes_focus: Option<&FocusHandle>,
        window: &Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let details = self.details.as_ref()?.clone();
        item_content(&self.item)?;
        let expanded = details.expanded();
        let row_focus = self.focus_handle.clone();
        let notes_focus = notes_focus.cloned();
        let preview = self.presentation == CardPresentation::Preview;
        let owner = window.current_view();
        Some(
            Button::new((self.element_id.clone(), "details-disclosure"))
                .tab_stop(self.tab_stop)
                .ghost()
                .xsmall()
                .when_else(
                    in_footer,
                    |button| button.h(DETAILS_FOOTER_HEIGHT),
                    |button| button.compact(),
                )
                .icon(if expanded {
                    AppIcon::ChevronUp
                } else {
                    AppIcon::ChevronDown
                })
                .when(labelled, |button| {
                    button.label(if expanded { "Show less" } else { "Show more" })
                })
                .tooltip(if expanded {
                    "Collapse details"
                } else {
                    "Expand details"
                })
                .text_color(cx.theme().colors.text_muted)
                .when_else(
                    preview,
                    |button| {
                        button.on_any_mouse_down(|event, _, cx| {
                            if event.button != MouseButton::Left {
                                cx.stop_propagation();
                            }
                        })
                    },
                    |button| button.block_mouse_except_scroll(),
                )
                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                    SelectionManager::claim_press(cx);
                    cx.stop_propagation();
                })
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    details.toggle(&row_focus, notes_focus.as_ref(), window, cx);
                    cx.notify(owner);
                })
                .into_any_element(),
        )
    }

    fn preview_controls(
        &self,
        controls: impl IntoElement,
        window: &Window,
        cx: &App,
    ) -> AnyElement {
        let controls = controls.into_any_element();
        if self.presentation != CardPresentation::Preview {
            return controls;
        }

        // Visibility::Hidden removes tab stops; keep them mounted and reveal on descendant focus.
        div()
            .flex_none()
            .opacity(if self.focus_handle.contains_focused(window, cx) {
                1.0
            } else {
                0.0
            })
            .group_hover(PREVIEW_HOVER_GROUP, |style| style.opacity(1.0))
            .child(controls)
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_content(
        &mut self,
        colors: ButtonColors,
        geometry: CardContentGeometry,
        menu_enabled: bool,
        pointer_inside: bool,
        compact_title: bool,
        notes_focus: Option<&FocusHandle>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let preview = self.presentation == CardPresentation::Preview;
        let collapsed_preview = preview
            && !self
                .details
                .as_ref()
                .is_some_and(|details| details.is_open());
        let narrow_details = self.details.is_some()
            && item_content(&self.item).is_some()
            && geometry.size.width > px(0.0)
            && geometry.size.width < px(192.0);
        if narrow_details {
            self.content_offset.x = px(2.0);
        }
        let draft_picker = self.draft_type_picker(cx);
        let has_draft_picker = draft_picker.is_some();
        let editor_width = self.editor_width.filter(|_| {
            self.editable
                && ItemManager::global(cx)
                    .read(cx)
                    .is_being_edited(self.item.id())
        });
        let title = render_dynamic_title(
            &self.item,
            compact_title,
            self.editable,
            self.title_scale,
            window,
            cx,
        )
        .into_any_element();

        let more_action =
            self.render_more_action(menu_enabled && !narrow_details, pointer_inside, window, cx);

        if self.compact {
            let details_toggle = self.render_details_toggle(false, false, notes_focus, window, cx);
            let completion = draft_picker.or_else(|| {
                (!narrow_details || preview)
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
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .when_some(editor_width, |title, width| title.max_w(width))
                        .child(title),
                )
                .when(!narrow_details, |row| {
                    let availability =
                        event_availability_indicator(&self.item, &self.element_id, cx);
                    row.children(
                        availability.map(|control| self.preview_controls(control, window, cx)),
                    )
                })
                .children(details_toggle.map(|control| self.preview_controls(control, window, cx)))
                .when(self.projected_availability_store.is_some(), |row| {
                    row.children(
                        more_action.map(|control| self.preview_controls(control, window, cx)),
                    )
                });
            return div().relative().size_full().child(row).into_any_element();
        }

        let leading = draft_picker
            .or_else(|| self.completion_checkbox(cx))
            .unwrap_or_else(|| {
                let manager = ItemManager::global(cx);
                let item_type = manager
                    .read(cx)
                    .editing_item
                    .as_ref()
                    .filter(|editing| editing.id() == self.item.id())
                    .map_or_else(|| self.item.item_type(), |editing| editing.item_type());
                let icon = item_icon(item_type).text_color(colors.fg);
                match self.title_scale {
                    ItemCardTitleScale::Small | ItemCardTitleScale::Standard => icon.size_4(),
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
                                .when(!preview, |button| button.block_mouse_except_scroll())
                                .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    let saved = AppDatabaseStore::global(cx)
                                        .update(cx, |store, cx| {
                                            store.uncomplete_action(action_id, cx).is_ok()
                                        });
                                    if saved {
                                        handoff.take(window, cx);
                                    }
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
                                .when(!preview, |button| button.block_mouse_except_scroll())
                                .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                                .on_click(move |_, _window, cx| {
                                    cx.stop_propagation();
                                    AppDatabaseStore::global(cx).update(cx, |store, cx| {
                                        let _ = store.auto_queue_action(action_id, cx);
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
                                .when(!preview, |button| button.block_mouse_except_scroll())
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
        if !collapsed_preview && let Some(rule) = self.item.recurrence() {
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

        let content_source = if self.title_only || collapsed_preview {
            None
        } else {
            item_content(&self.item)
        };
        let open = self
            .details
            .as_ref()
            .is_some_and(|details| details.is_open());
        let expandable = self.details.is_some() && item_content(&self.item).is_some();
        let top_aligned = !preview && self.list_position.is_none() && self.border;
        let header_height = (content_source.is_some() || expandable).then(|| {
            window.use_keyed_state(
                (self.element_id.clone(), "notes-header-height"),
                cx,
                |_, _| px(0.),
            )
        });
        let measured_header = header_height
            .as_ref()
            .map_or(px(0.), |height| *height.read(cx));

        let box_height = geometry.size.height;
        let in_footer = expandable
            && (open
                || box_height <= px(0.)
                || box_height >= self.content_offset.y + measured_header + DETAILS_FOOTER_EXTENT);
        let bottom_inset = if in_footer {
            DETAILS_FOOTER_EXTENT
        } else {
            ITEM_CARD_END_INSET
        };
        let held_top = self
            .details
            .as_ref()
            .filter(|_| expandable && !top_aligned)
            .and_then(ItemCardDetails::collapsed_top);
        let open_top = held_top.unwrap_or(self.content_offset.y);
        let top = if open {
            open_top
        } else {
            self.content_offset.y
        };
        let labelled = if in_footer {
            !narrow_details
        } else {
            geometry.size.width >= px(360.0)
        };
        let details_toggle =
            self.render_details_toggle(in_footer, labelled, notes_focus, window, cx);

        let content_panel = content_source.and_then(|source| {
            render_item_card_notes(
                self.element_id.clone(),
                source,
                geometry.notes_size(top, measured_header, bottom_inset, self.content_offset.x),
                self.details.clone(),
                self.tab_stop,
                self.inspector_action(),
                open_top
                    + measured_header
                    + ITEM_HEADER_CONTENT_GAP
                    + if expandable {
                        DETAILS_FOOTER_EXTENT
                    } else {
                        ITEM_CARD_END_INSET
                    }
                    + ITEM_CARD_BORDER * 2.0,
                window,
                cx,
            )
            .map(|(notes, viewport_height)| {
                let notes = match notes_focus {
                    Some(focus) => div()
                        .id((self.element_id.clone(), "notes-focus-scope"))
                        .w_full()
                        .flex_none()
                        .track_focus(focus)
                        .child(notes)
                        .into_any_element(),
                    None => notes,
                };
                (notes, viewport_height)
            })
        });
        if let Some(details) = self.details.as_ref()
            && expandable
            && !top_aligned
            && !open
            && box_height > px(0.)
        {
            let panel = content_panel
                .as_ref()
                .map_or(px(0.), |(_, height)| ITEM_HEADER_CONTENT_GAP + *height);
            let slack = box_height - top - bottom_inset - measured_header - panel;
            details.record_collapsed_top(top + (slack / 2.).max(px(0.)));
        }
        let content_panel = content_panel.map(|(notes, _)| notes);
        let (inline_toggle, footer_toggle) = if in_footer {
            (None, details_toggle)
        } else {
            (details_toggle, None)
        };

        trailing.extend(inline_toggle);
        trailing.extend(more_action);
        let trailing_controls = (!trailing.is_empty()).then(|| {
            self.preview_controls(
                div().row().flex_none().gap(px(2.)).children(
                    trailing
                        .into_iter()
                        .map(|control| div().flex_none().child(control)),
                ),
                window,
                cx,
            )
        });
        let footer = footer_toggle.map(|toggle| {
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(DETAILS_FOOTER_INSET)
                .h(DETAILS_FOOTER_HEIGHT)
                .row()
                .items_center()
                .justify_center()
                .child(self.preview_controls(toggle, window, cx))
        });
        let header = div()
            .row()
            .w_full()
            .flex_none()
            .when_else(
                top_aligned,
                |this| this.items_start(),
                |this| this.items_center(),
            )
            .gap(if narrow_details {
                px(2.0)
            } else {
                self.title_scale.header_gap()
            })
            .when(!narrow_details || has_draft_picker || preview, |header| {
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
                    .child(
                        div()
                            .row()
                            .w_full()
                            .min_w(px(0.))
                            .when_some(editor_width, |title, width| title.max_w(width))
                            .child(title),
                    )
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
                top_aligned || open,
                |this| this.justify_start(),
                |this| this.justify_center(),
            )
            .gap(ITEM_HEADER_CONTENT_GAP)
            .pl(self.content_offset.x)
            .pr(ITEM_CARD_END_INSET)
            .pt(top)
            .pb(bottom_inset)
            .child(header)
            .children(content_panel)
            .children(footer)
            .into_any_element()
    }
}
