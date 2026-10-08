use std::{cell::Cell, rc::Rc};

use gpui::{
    App, Bounds, Hsla, InteractiveElement, IntoElement, KeyContext, MouseButton, ParentElement,
    PathBuilder, Pixels, RenderOnce, StatefulInteractiveElement, Styled, Window, canvas, div,
    point, prelude::FluentBuilder, px,
};

use gpui_kit::foundation::FocusRing as _;
use gpui_kit_theme::ActiveTheme;

use crate::{
    components::{
        Draggable,
        ext::ElementExt,
        menu::{context_menu_contains_position, open_context_menu_at_bounds},
        transition::{self, WindowTransitionExt as _},
    },
    item_manager::{
        DRAFT_KEY_CONTEXT, DraftAsAction, DraftAsEvent, DraftType, ItemManager, OpenDraftTypeMenu,
    },
    selection::{ITEM_CARD_KEY_CONTEXT, OpenItemContextMenu, SelectionGesture, SelectionManager},
    settings::Settings,
};

use super::{
    DraggedItems, ItemCard, ToggleItemDetails, availability::projected_event_availability_target,
    card_background, content::CardContentGeometry, create_drag_data, draft_type::open_type_menu,
    menu::card_context_menu, neutral_card_colors, notes::item_content,
};

pub(super) const ITEM_CARD_BORDER: Pixels = px(2.);

const ITEM_SKIN_SPACING: Pixels = px(16.);
const ITEM_SKIN_DOT: Pixels = px(1.);
const ITEM_SKIN_ALPHA: f32 = 0.085;

fn item_card_key_context(navigation_context: Option<&'static str>, actionable: bool) -> KeyContext {
    let mut context = KeyContext::default();
    if let Some(navigation_context) = navigation_context {
        context.add(navigation_context);
    }
    if actionable {
        context.add(ITEM_CARD_KEY_CONTEXT);
    }
    context
}

fn item_card_skin(color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let mut dots = PathBuilder::stroke(ITEM_SKIN_DOT);
            let spacing = ITEM_SKIN_SPACING.as_f32();
            let columns = (bounds.size.width.as_f32() / spacing).ceil() as usize + 1;
            let rows = (bounds.size.height.as_f32() / spacing).ceil() as usize + 1;

            for row in 0..rows {
                let shift = if row % 2 == 0 { 0.0 } else { spacing / 2.0 };
                let y = bounds.top() + ITEM_SKIN_SPACING * (row as f32 + 0.5);
                for column in 0..columns {
                    let x = bounds.left() + ITEM_SKIN_SPACING * (column as f32 + 0.5) + px(shift);
                    dots.move_to(point(x, y));
                    dots.line_to(point(x + ITEM_SKIN_DOT, y));
                }
            }

            if let Ok(dots) = dots.build() {
                window.paint_path(dots, color);
            }
        },
    )
    .absolute()
    .size_full()
}

impl RenderOnce for ItemCard {
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = neutral_card_colors(cx.theme());

        let (is_editing, is_draft) = {
            let manager = ItemManager::global(cx);
            let manager = manager.read(cx);
            (
                manager.is_being_edited(self.item.id()),
                manager.is_draft(self.item.id()),
            )
        };
        self.actionable &= !is_draft;
        self.draggable &= !is_draft;
        let can_change_type = self.editable
            && ItemManager::global(cx)
                .read(cx)
                .draft_type(self.item.id())
                .is_some();
        let availability_menu_enabled = !is_draft
            && self
                .projected_availability_store
                .as_ref()
                .is_some_and(|store| {
                    projected_event_availability_target(&self.item, store.read(cx).events())
                        .is_some()
                });
        if self.projected_availability_store.is_some() {
            self.actionable = false;
            self.editable = false;
            self.draggable = false;
            self.selection = None;
            self.tab_stop = availability_menu_enabled;
        }
        let menu_enabled = self.actionable || availability_menu_enabled;
        let draft_opacity = if is_draft {
            let transition = window.keyed_transition(
                (self.element_id.clone(), "draft-entrance"),
                cx,
                transition::QUICK,
                || 0.0,
            );
            transition.set(1.0, cx);
            transition.animate(window, cx)
        } else {
            1.0
        };
        let has_focus = self.focus_handle.is_focused(window);
        let item = self.item.clone();
        let item_for_menu = self.item.clone();
        let item_for_keyboard_menu = self.item.clone();
        let schedule_navigation = self.schedule_navigation;
        let item_id = self.item.id();

        let availability_for_menu = self.projected_availability_store.clone();
        let availability_for_keyboard_menu = self.projected_availability_store.clone();
        let selection_order = self.selection.clone();
        let on_click = self.on_click.clone();
        let selection_scope = selection_order.as_ref().map(|order| order.scope);
        let is_multi_selected = selection_scope.is_some_and(|scope| {
            SelectionManager::global(cx)
                .read(cx)
                .is_selected_in(scope, item_id)
        });
        let is_marked = is_editing || is_multi_selected || has_focus;

        let drag_payload = self.drag_payload.clone();
        let dragged = self.draggable.then(|| {
            if let Some(payload) = drag_payload {
                return payload;
            }
            let group = if is_multi_selected {
                SelectionManager::selected_items(cx)
            } else {
                Vec::new()
            };
            if group.len() > 1 && group.iter().any(|other| other.id() == item_id) {
                DraggedItems {
                    primary: self.item.clone(),
                    source_anchor: None,
                    items: group,
                    saved_item_ids: None,
                    materialized_item_ids: Vec::new(),
                }
            } else {
                DraggedItems::single(self.item.clone())
            }
        });
        let drag_size = self.drag_size;
        let dragged_since_press = Rc::new(Cell::new(false));
        let dragged_for_start = Rc::clone(&dragged_since_press);
        let dragged_for_press = Rc::clone(&dragged_since_press);
        let dragged_for_click = Rc::clone(&dragged_since_press);
        let measured_bounds = Rc::new(Cell::new(None::<Bounds<Pixels>>));
        let bounds_for_prepaint = Rc::clone(&measured_bounds);
        let bounds_for_keyboard_menu = Rc::clone(&measured_bounds);

        let has_notes = item_content(&self.item).is_some();
        if let Some(details) = &self.details {
            if !has_notes && details.reset() {
                let owner = window.current_view();
                window.on_next_frame(move |_, cx| cx.notify(owner));
            }
            if details.is_open() {
                self.compact = false;
                self.title_only = false;
                self.content_offset.x = px(8.0);
                if !self.positioned_content {
                    self.content_offset.y = px(6.0);
                }
            }
        }
        let keyboard_details = self
            .details
            .clone()
            .filter(|_| has_notes && (self.actionable || self.tab_stop) && !is_editing);
        let mut key_context = item_card_key_context(self.navigation_context, menu_enabled);
        if can_change_type {
            key_context.add(DRAFT_KEY_CONTEXT);
        }
        if keyboard_details.is_some() {
            key_context.add("ItemCardDetails");
        }
        let has_content = !self.title_only && has_notes;
        let skin_color = colors.fg.alpha(ITEM_SKIN_ALPHA);
        let skin = (!self.compact && !has_content).then(|| item_card_skin(skin_color));
        let measured_geometry = has_notes.then(|| {
            window.use_keyed_state((self.element_id.clone(), "notes-geometry"), cx, |_, _| {
                CardContentGeometry::default()
            })
        });
        let geometry = measured_geometry
            .as_ref()
            .map_or_default(|geometry| *geometry.read(cx));
        let owner = window.current_view();
        let bg = card_background(cx);
        let content = self.render_content(colors, geometry, menu_enabled, window, cx);
        let content = if let Some(geometry) = measured_geometry {
            div()
                .w_full()
                .h_full()
                .on_children_prepainted(move |children, window, cx| {
                    let Some(bounds) = children.first().copied() else {
                        return;
                    };
                    let measured = CardContentGeometry::measured(bounds);
                    if *geometry.read(cx) != measured {
                        geometry.update(cx, |current, _| *current = measured);
                        window.on_next_frame(move |_, cx| cx.notify(owner));
                    }
                })
                .child(content)
                .into_any_element()
        } else {
            content
        };

        self.base
            .border(ITEM_CARD_BORDER)
            .when(self.border, |this| {
                this.when_some(colors.border, |this, color| this.border_color(color))
            })
            .bg(bg)
            .opacity(draft_opacity)
            .hover(|s| s.bg(colors.hover))
            .when(is_marked, |this| this.border_color(cx.theme().colors.focus))
            .when(is_multi_selected, |this| {
                let tint = cx.theme().colors.focus.alpha(0.10);
                this.bg(bg.blend(tint))
                    .hover(|style| style.bg(colors.hover.blend(tint)))
            })
            .when_else(
                self.compact,
                |this| this.rounded_lg(),
                |this| this.rounded_xl(),
            )
            .overflow_hidden()
            .shadow_sm()
            .track_focus(&self.focus_handle.tab_stop(self.tab_stop))
            .focus_ring(cx.theme())
            .key_context(key_context)
            .when(can_change_type, |card| {
                card.on_mouse_down_out(move |event, window, cx| {
                    if !context_menu_contains_position(event.position, cx) {
                        ItemManager::global(cx).update(cx, |manager, cx| {
                            if manager.is_draft(item_id) {
                                manager.commit_open_edit(window, cx);
                            }
                        });
                    }
                })
            })
            .when(can_change_type, |card| {
                card.on_action(move |_: &DraftAsAction, window, cx| {
                    ItemManager::global(cx).update(cx, |manager, cx| {
                        manager.set_draft_type(item_id, DraftType::Action, window, cx);
                    });
                })
                .on_action(move |_: &DraftAsEvent, window, cx| {
                    ItemManager::global(cx).update(cx, |manager, cx| {
                        manager.set_draft_type(item_id, DraftType::Event, window, cx);
                    });
                })
                .on_action(move |_: &OpenDraftTypeMenu, window, cx| {
                    open_type_menu(item_id, window, cx);
                })
            })
            .when_some(keyboard_details, |card, details| {
                card.on_action(move |_: &ToggleItemDetails, _, cx| {
                    details.toggle();
                    cx.notify(owner);
                })
            })
            .on_prepaint(move |bounds, _, _| bounds_for_prepaint.set(Some(bounds)))
            .when(menu_enabled, |this| {
                this.on_action(move |_: &OpenItemContextMenu, window, cx| {
                    let Some(bounds) = bounds_for_keyboard_menu.get() else {
                        cx.propagate();
                        return;
                    };
                    if let Some(scope) = selection_scope {
                        SelectionManager::prepare_context_menu(scope, item_id, cx);
                    }
                    let builder = card_context_menu(
                        &item_for_keyboard_menu,
                        selection_scope,
                        schedule_navigation,
                        availability_for_keyboard_menu.as_ref(),
                        cx,
                    );
                    open_context_menu_at_bounds(builder, bounds, window, cx);
                })
            })
            .children(skin)
            .when_some(on_click, |this, handler| {
                this.on_click(move |event, window, cx| handler(event, window, cx))
            })
            .when_some(selection_order, |this, order| {
                let scope = order.scope;
                let range_order = order.clone();
                SelectionManager::report_order(&order, cx);
                this.on_prepaint(move |bounds, window, cx| {
                    let visible = bounds.intersect(&window.content_mask().bounds);
                    if !visible.is_empty() {
                        SelectionManager::report_card(scope, item_id, visible, cx);
                    }
                })
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    dragged_for_press.set(false);
                    SelectionManager::claim_press(cx);
                    let manager = SelectionManager::global(cx);
                    let order = range_order.clone();
                    match Settings::global(cx).selection.gesture(&window.modifiers()) {
                        SelectionGesture::Toggle => {
                            manager.update(cx, |selection, cx| selection.toggle(scope, item_id, cx))
                        }
                        SelectionGesture::Range => manager.update(cx, |selection, cx| {
                            selection.select_range(&order, item_id, false, cx)
                        }),
                        SelectionGesture::ExtendRange => manager.update(cx, |selection, cx| {
                            selection.select_range(&order, item_id, true, cx)
                        }),
                        SelectionGesture::Replace => {
                            let inside = manager.read(cx).is_selected_in(scope, item_id);
                            if !inside {
                                manager.update(cx, |selection, cx| {
                                    selection.select_only(scope, item_id, cx)
                                });
                            }
                        }
                    }
                })
                .on_click(move |_, window, cx| {
                    if dragged_for_click.replace(false) {
                        return;
                    }
                    if Settings::global(cx)
                        .selection
                        .is_modified(&window.modifiers())
                    {
                        return;
                    }
                    SelectionManager::global(cx).update(cx, |selection, cx| {
                        selection.select_only(scope, item_id, cx)
                    });
                })
            })
            .on_mouse_down(
                MouseButton::Right,
                move |event: &gpui::MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    if !menu_enabled {
                        return;
                    }
                    if let Some(scope) = selection_scope {
                        SelectionManager::claim_press(cx);
                        SelectionManager::prepare_context_menu(scope, item_id, cx);
                    }
                    let builder = card_context_menu(
                        &item_for_menu,
                        selection_scope,
                        schedule_navigation,
                        availability_for_menu.as_ref(),
                        cx,
                    );
                    crate::components::menu::open_context_menu(builder, event.position, window, cx);
                },
            )
            .when_some(dragged, |this, dragged| {
                let drag_data = create_drag_data(dragged, drag_size, cx);
                this.child(
                    Draggable::new(("item-card-draggable", item.id_u64()), drag_data)
                        .on_drag_start(move |_, _| dragged_for_start.set(true))
                        .absolute()
                        .size_full(),
                )
            })
            .child(content)
    }
}
