use std::{cell::Cell, rc::Rc};

use gpui::{
    App, Bounds, Corners, DispatchPhase, Edges, EntityId, FocusHandle, HitboxBehavior, Hsla,
    InteractiveElement, IntoElement, KeyContext, MouseButton, MouseExitEvent, MouseMoveEvent,
    ParentElement, PathBuilder, Pixels, RenderOnce, StatefulInteractiveElement, Styled, Window,
    canvas, div, point, prelude::FluentBuilder, px,
};

use gpui_kit::foundation::FocusRing as _;
use gpui_kit_theme::ActiveTheme;
use subroutine_core::{AnyItem, ItemType};

use crate::{
    components::{
        Draggable,
        ext::{ElementExt, InteractiveElementExt},
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
    CARD_HOVER_GROUP, CardMeta, CardPresentation, DraggedItems, ItemCard, ItemCardPosition,
    ItemCardTitleScale, ToggleItemDetails,
    availability::projected_event_availability_target,
    card_background,
    content::CardContentGeometry,
    create_drag_data,
    draft_type::open_type_menu,
    menu::card_context_menu,
    neutral_card_colors,
    notes::{NotesFocus, NotesSurface, item_content},
};

pub(super) const ITEM_CARD_BORDER: Pixels = px(2.);

const ITEM_SKIN_SPACING: Pixels = px(16.);
const ITEM_SKIN_DOT: Pixels = px(1.);
const ITEM_SKIN_ALPHA: f32 = 0.085;

fn card_border_widths(
    position: ItemCardPosition,
    marked: bool,
    [previous_marked, next_marked]: [bool; 2],
) -> Edges<Pixels> {
    let top = match position {
        ItemCardPosition::Standalone | ItemCardPosition::First => ITEM_CARD_BORDER,
        _ if marked && !previous_marked => ITEM_CARD_BORDER,
        _ => px(0.),
    };
    let bottom = match position {
        ItemCardPosition::Standalone | ItemCardPosition::Last => ITEM_CARD_BORDER,
        _ if !marked && next_marked => px(0.),
        _ if marked && !next_marked => ITEM_CARD_BORDER,
        _ => px(1.),
    };
    Edges {
        top,
        bottom,
        left: ITEM_CARD_BORDER,
        right: ITEM_CARD_BORDER,
    }
}

fn horizontal_card_border_widths(position: ItemCardPosition) -> Edges<Pixels> {
    let (left, right) = match position {
        ItemCardPosition::Standalone => (ITEM_CARD_BORDER, ITEM_CARD_BORDER),
        ItemCardPosition::First => (ITEM_CARD_BORDER, px(0.)),
        ItemCardPosition::Middle => (px(0.), px(0.)),
        ItemCardPosition::Last => (px(0.), ITEM_CARD_BORDER),
    };
    Edges {
        top: ITEM_CARD_BORDER,
        bottom: ITEM_CARD_BORDER,
        left,
        right,
    }
}

fn inset_card_focus_ring(
    focus: FocusHandle,
    borders: Edges<Pixels>,
    corners: Corners<Pixels>,
    background: Hsla,
    cx: &App,
) -> impl IntoElement {
    let rings = cx
        .theme()
        .focus_ring_on(background)
        .into_iter()
        .map(gpui::BoxShadow::inset)
        .collect::<Vec<_>>();
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            if focus.is_focused(window) && window.focus_is_visible() {
                window.paint_inset_shadows(
                    bounds,
                    corners.clamp_radii_for_quad_size(bounds.size),
                    &rings,
                );
            }
        },
    )
    .absolute()
    // Keep focus inside the same content box even when selection borders are shared.
    .top(ITEM_CARD_BORDER - borders.top)
    .bottom(ITEM_CARD_BORDER - borders.bottom)
    .left(ITEM_CARD_BORDER - borders.left)
    .right(ITEM_CARD_BORDER - borders.right)
}

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

fn pointer_presence(inside: Rc<Cell<bool>>, owner: EntityId) -> impl IntoElement {
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |_, hitbox, window, _| {
            let update = {
                let inside = Rc::clone(&inside);
                move |now: bool, cx: &mut App| {
                    if inside.replace(now) != now {
                        cx.notify(owner);
                    }
                }
            };
            let now = hitbox.should_handle_scroll(window);
            if inside.get() != now {
                let update = update.clone();
                window.on_next_frame(move |_, cx| update(now, cx));
            }
            let moved = update.clone();
            window.on_mouse_event(move |_: &MouseMoveEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble {
                    moved(hitbox.should_handle_scroll(window), cx);
                }
            });
            window.on_mouse_event(move |_: &MouseExitEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    update(false, cx);
                }
            });
        },
    )
    .absolute()
    .size_full()
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
        let preview = self.presentation == CardPresentation::Preview;
        let subtle = self.title_scale == ItemCardTitleScale::Small;
        let mut colors = neutral_card_colors(cx.theme());
        let bg = if preview {
            gpui::transparent_black()
        } else if subtle {
            card_background(cx).alpha(0.45)
        } else {
            card_background(cx)
        };
        if subtle {
            colors.fg = cx.theme().colors.text_muted;
            colors.hover = bg.blend(cx.theme().colors.hover);
        }
        if preview {
            colors.hover = cx.theme().colors.hover;
        }

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
        if is_draft && matches!(self.item, AnyItem::Action(_)) {
            let manager = ItemManager::global(cx);
            if let Some(editing) = manager.read(cx).editing_item.as_ref()
                && editing.id() == self.item.id()
                && editing.item_type() == ItemType::Event
            {
                self.meta = editing
                    .draft_needs_time()
                    .then(|| CardMeta::new("Time required"))
                    .into_iter()
                    .collect();
            }
        }
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
        let has_notes = item_content(&self.item).is_some();
        let notes_focus = has_notes.then(|| {
            let expanded = self
                .details
                .as_ref()
                .is_some_and(|details| details.expanded());
            window
                .use_keyed_state((self.element_id.clone(), "notes-focus"), cx, |_, cx| {
                    NotesFocus::new(cx)
                })
                .update(cx, |state, cx| {
                    state.sync(expanded, &self.focus_handle, window, cx)
                })
        });
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
        let inspector_action = self.inspector_action();
        let is_multi_selected = selection_scope.is_some_and(|scope| {
            SelectionManager::global(cx)
                .read(cx)
                .is_selected_in(scope, item_id)
        });
        let is_marked = is_editing || (self.list_position.is_none() && has_focus);
        let neighbor_marks = self.neighbors.map(|neighbor| {
            neighbor.is_some_and(|id| ItemManager::global(cx).read(cx).is_being_edited(id))
        });
        let position = self.list_position.unwrap_or_default();
        let horizontal = self.horizontal_position.is_some();
        let inset_focus = self.list_position.is_some() || horizontal;
        let borders = self.horizontal_position.map_or_else(
            || card_border_widths(position, is_marked, neighbor_marks),
            horizontal_card_border_widths,
        );
        let joined_above = matches!(position, ItemCardPosition::Middle | ItemCardPosition::Last);
        let joined_below = matches!(position, ItemCardPosition::First | ItemCardPosition::Middle);
        let standalone = position == ItemCardPosition::Standalone;

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
        let hold_to_drag = self.hold_to_drag;
        let dragged_since_press = Rc::new(Cell::new(false));
        let dragged_for_start = Rc::clone(&dragged_since_press);
        let dragged_for_press = Rc::clone(&dragged_since_press);
        let dragged_for_click = Rc::clone(&dragged_since_press);
        let measured_bounds = Rc::new(Cell::new(None::<Bounds<Pixels>>));
        let bounds_for_prepaint = Rc::clone(&measured_bounds);
        let bounds_for_keyboard_menu = Rc::clone(&measured_bounds);

        let compact_title = self.compact;
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
        let skin = (!preview && !subtle && !self.compact && !has_content)
            .then(|| item_card_skin(skin_color));
        let measured_geometry = has_notes.then(|| {
            window.use_keyed_state((self.element_id.clone(), "notes-geometry"), cx, |_, _| {
                CardContentGeometry::default()
            })
        });
        let geometry = measured_geometry
            .as_ref()
            .map_or_default(|geometry| *geometry.read(cx));
        let owner = window.current_view();
        let pointer_inside = window
            .use_keyed_state((self.element_id.clone(), "pointer-inside"), cx, |_, _| {
                Rc::new(Cell::new(false))
            })
            .read(cx)
            .clone();
        let buttons_revealed = pointer_inside.get()
            || (window.focus_is_visible() && self.focus_handle.contains_focused(window, cx));
        let selection_tint = cx.theme().colors.focus.alpha(0.10);
        let hover_bg = if is_multi_selected {
            colors.hover.blend(selection_tint)
        } else {
            colors.hover
        };
        let rest_bg = if is_multi_selected {
            bg.blend(selection_tint)
        } else if preview || !subtle || is_marked {
            bg
        } else {
            gpui::transparent_black()
        };
        let notes_surface = NotesSurface::opaque(rest_bg, hover_bg);
        let content = self.render_content(
            colors,
            geometry,
            menu_enabled,
            buttons_revealed,
            compact_title,
            notes_focus.as_ref(),
            notes_surface,
            window,
            cx,
        );
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

        let row_focus = self.focus_handle.clone();
        let mut card = self
            .base
            .group(CARD_HOVER_GROUP)
            .border(ITEM_CARD_BORDER)
            .when(preview, |card| {
                card.border_color(gpui::transparent_black()).shadow_none()
            })
            .when(self.border && !preview, |this| {
                this.when_some(colors.border, |this, color| this.border_color(color))
            })
            .when(preview || !subtle || is_marked, |card| card.bg(bg))
            .opacity(draft_opacity)
            .hover(|style| style.bg(hover_bg))
            .when(is_marked, |this| this.border_color(cx.theme().colors.focus))
            .when(is_multi_selected, |this| this.bg(rest_bg))
            .when_else(
                self.compact,
                |this| this.rounded_lg(),
                |this| this.rounded_xl(),
            )
            .when(joined_above, |card| card.rounded_t(px(0.)))
            .when(joined_below, |card| card.rounded_b(px(0.)))
            // Reserve the same space whether an edge is a border, a divider, or shared.
            .when(self.list_position.is_some(), |card| {
                card.border_t(borders.top)
                    .border_b(borders.bottom)
                    .pt(ITEM_CARD_BORDER - borders.top)
                    .pb(ITEM_CARD_BORDER - borders.bottom)
            })
            .when(horizontal, |card| {
                card.border_l(borders.left)
                    .border_r(borders.right)
                    .pl(ITEM_CARD_BORDER - borders.left)
                    .pr(ITEM_CARD_BORDER - borders.right)
                    .when(borders.left == px(0.), |card| card.rounded_l(px(0.)))
                    .when(borders.right == px(0.), |card| card.rounded_r(px(0.)))
                    .shadow_none()
            })
            .overflow_hidden()
            .when(!preview && !subtle && standalone && !horizontal, |this| {
                this.shadow_sm()
            })
            .track_focus(&self.focus_handle.clone().tab_stop(self.tab_stop))
            .when(!inset_focus, |card| card.focus_ring(cx.theme()))
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
                card.on_action(move |_: &ToggleItemDetails, window, cx| {
                    details.toggle(&row_focus, notes_focus.as_ref(), window, cx);
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
            .child(pointer_presence(pointer_inside, owner))
            .when_some(inspector_action, |card, action| {
                card.on_double_click(move |_, window, cx| {
                    if dragged_since_press.get()
                        || Settings::global(cx)
                            .selection
                            .is_modified(&window.modifiers())
                    {
                        return;
                    }
                    cx.stop_propagation();
                    window.dispatch_action(action.boxed_clone(), cx);
                })
            })
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
                        .hold_to_drag(hold_to_drag)
                        .on_drag_start(move |_, _| dragged_for_start.set(true))
                        .absolute()
                        .size_full(),
                )
            })
            .child(content);

        if inset_focus {
            let radii = &card.style().corner_radii;
            let [top_left, top_right, bottom_right, bottom_left] = [
                radii.top_left,
                radii.top_right,
                radii.bottom_right,
                radii.bottom_left,
            ]
            .map(|radius| {
                (radius.map_or(px(0.), |radius| radius.to_pixels(window.rem_size()))
                    - ITEM_CARD_BORDER)
                    .max(px(0.))
            });
            card = card.child(inset_card_focus_ring(
                self.focus_handle,
                borders,
                Corners {
                    top_left,
                    top_right,
                    bottom_right,
                    bottom_left,
                },
                bg,
                cx,
            ));
        }
        card
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_spans_keep_only_terminal_side_borders() {
        for (position, left, right) in [
            (
                ItemCardPosition::Standalone,
                ITEM_CARD_BORDER,
                ITEM_CARD_BORDER,
            ),
            (ItemCardPosition::First, ITEM_CARD_BORDER, px(0.)),
            (ItemCardPosition::Middle, px(0.), px(0.)),
            (ItemCardPosition::Last, px(0.), ITEM_CARD_BORDER),
        ] {
            let borders = horizontal_card_border_widths(position);
            assert_eq!(borders.top, ITEM_CARD_BORDER);
            assert_eq!(borders.bottom, ITEM_CARD_BORDER);
            assert_eq!(borders.left, left);
            assert_eq!(borders.right, right);
        }
    }

    #[test]
    fn adjacent_rows_draw_each_boundary_once() {
        for (upper_selected, lower_selected, seam) in [
            (false, false, px(1.)),
            (false, true, ITEM_CARD_BORDER),
            (true, false, ITEM_CARD_BORDER),
            (true, true, px(1.)),
        ] {
            let upper = card_border_widths(
                ItemCardPosition::First,
                upper_selected,
                [false, lower_selected],
            );
            let lower = card_border_widths(
                ItemCardPosition::Last,
                lower_selected,
                [upper_selected, false],
            );
            assert_eq!(upper.bottom + lower.top, seam);
            assert_eq!(upper.top, ITEM_CARD_BORDER);
            assert_eq!(lower.bottom, ITEM_CARD_BORDER);
            assert_eq!(upper.left, lower.left);
            assert_eq!(upper.right, lower.right);
        }
    }

    #[test]
    fn selection_runs_close_at_unselected_rows_and_group_boundaries() {
        let middle = card_border_widths(ItemCardPosition::Middle, true, [true, true]);
        assert_eq!(middle.top, px(0.));
        assert_eq!(middle.bottom, px(1.));

        let isolated = card_border_widths(ItemCardPosition::Middle, true, [false, false]);
        assert_eq!(isolated.top, ITEM_CARD_BORDER);
        assert_eq!(isolated.bottom, ITEM_CARD_BORDER);

        let first = card_border_widths(ItemCardPosition::First, true, [true, true]);
        let last = card_border_widths(ItemCardPosition::Last, true, [true, true]);
        let single = card_border_widths(ItemCardPosition::Standalone, true, [true, true]);
        assert_eq!(first.top, ITEM_CARD_BORDER);
        assert_eq!(last.bottom, ITEM_CARD_BORDER);
        assert_eq!(single.top, ITEM_CARD_BORDER);
        assert_eq!(single.bottom, ITEM_CARD_BORDER);
    }
}
