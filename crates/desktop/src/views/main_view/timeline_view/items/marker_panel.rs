use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use gpui::{
    AnyElement, Bounds, Context, Edges, FocusHandle, FontWeight, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, ParentElement, Pixels, ScrollHandle, Size,
    StatefulInteractiveElement as _, Styled, Window, div, ease_in_out, point,
    prelude::FluentBuilder as _, px, size,
};
use gpui_kit::{
    foundation::{Sizable as _, StyledExt as _},
    overlay::GlassExt as _,
};
use gpui_kit_theme::{ActiveTheme, Radius, Surface};
use subroutine_core::{AnyItem, Marker};
use uuid::Uuid;

use crate::{
    AppIcon,
    components::{
        Button, ButtonVariants as _, CloseOverlay, DragData, DraggedItems, ItemCard, Label,
        SIDEBAR_ITEM_HEIGHT,
        ext::ElementExt as _,
        menu::{context_menu_contains_position, context_menu_has_focus},
        scrollbar::Scrollbar,
        transition::{self, WindowTransitionExt as _},
    },
    item_manager::ItemManager,
    selection::{
        SelectAllItems, SelectionManager, SelectionOrder, SelectionScope, focus_item,
        focus_item_extending,
    },
    views::{
        FocusNext, FocusPrevious, OVERLAY_BORDER_WIDTH, TOP_EDGE_INSET, format_item_meta,
        frosted_overlay_tint,
    },
};

use super::super::{self as timeline, TimelineView};

const HEADER_HEIGHT: Pixels = px(48.);
const HEADER_GAP: Pixels = px(8.);
const PADDING: Pixels = px(12.);
const MAX_HEIGHT: Pixels = px(520.);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum MarkerEntry {
    Concrete(Uuid),
    Projected { lineage: Uuid, date: NaiveDate },
}

impl MarkerEntry {
    fn resolve(&mut self, known: &[&Marker]) -> Option<(Marker, bool)> {
        match *self {
            Self::Concrete(id) => known
                .iter()
                .copied()
                .find(|marker| marker.id == id)
                .map(|marker| (marker.clone(), false)),
            Self::Projected { lineage, date } => {
                if let Some(marker) = known
                    .iter()
                    .copied()
                    .filter(|marker| marker.lineage_id == lineage && marker.date == date)
                    .min_by_key(|marker| marker.id)
                {
                    // Once materialized, follow its identity even if its date changes.
                    *self = Self::Concrete(marker.id);
                    return Some((marker.clone(), false));
                }
                known
                    .iter()
                    .copied()
                    .filter(|marker| marker.lineage_id == lineage && marker.date < date)
                    .max_by_key(|marker| (marker.date, marker.id))?
                    .projections_between(date, date)
                    .into_iter()
                    .find(|marker| marker.date == date)
                    .map(|marker| (marker, true))
            }
        }
    }
}

pub(in crate::views::main_view::timeline_view) struct MarkerPanel {
    pub(in crate::views::main_view::timeline_view) draft: Option<Uuid>,
    // This is a timeline-local opening snapshot, never a time/scroll coordinate.
    anchor: Bounds<Pixels>,
    scope: Option<(NaiveDate, NaiveDate)>,
    entries: Vec<MarkerEntry>,
    scroll: ScrollHandle,
    focus_handles: HashMap<Uuid, FocusHandle>,
    order: SelectionOrder,
}

impl MarkerPanel {
    pub(in crate::views::main_view::timeline_view) fn new(
        mut markers: Vec<(Marker, bool)>,
        anchor: Bounds<Pixels>,
    ) -> Self {
        let scope = markers
            .iter()
            .map(|(marker, _)| (marker.date, marker.end_date.unwrap_or(marker.date)))
            .reduce(|(start, end), (next_start, next_end)| {
                (start.min(next_start), end.max(next_end))
            });
        markers.sort_by_key(|(marker, projected)| {
            (*projected, marker.date, marker.lineage_id, marker.id)
        });
        let mut seen = HashSet::new();
        let entries = markers
            .into_iter()
            .map(|(marker, projected)| {
                if projected {
                    MarkerEntry::Projected {
                        lineage: marker.lineage_id,
                        date: marker.date,
                    }
                } else {
                    MarkerEntry::Concrete(marker.id)
                }
            })
            .filter(|entry| seen.insert(*entry))
            .collect();
        Self {
            draft: None,
            anchor,
            scope,
            entries,
            scroll: ScrollHandle::new(),
            focus_handles: HashMap::new(),
            order: SelectionOrder::new(SelectionScope::Timeline, []),
        }
    }

    fn rows<'a>(&mut self, known: impl Iterator<Item = &'a Marker>) -> Vec<(Marker, bool)> {
        let known: Vec<_> = known.collect();
        if let Some(id) = self.draft
            && !self.entries.contains(&MarkerEntry::Concrete(id))
        {
            self.entries.insert(0, MarkerEntry::Concrete(id));
        }
        let mut seen = HashSet::new();
        self.entries
            .iter_mut()
            .filter_map(|entry| entry.resolve(&known))
            .filter(|(marker, _)| seen.insert(marker.id))
            .collect()
    }

    pub(in crate::views::main_view::timeline_view) fn set_scope(
        &mut self,
        start: NaiveDate,
        end: NaiveDate,
    ) {
        self.scope = Some((start, end));
    }

    fn title(&self) -> String {
        match self.scope {
            None => "Markers".into(),
            Some((start, end)) if start == end => {
                format!("Markers · {}", start.format("%a, %b %-d"))
            }
            Some((start, end)) => format!(
                "Markers · {} – {}",
                start.format("%b %-d, %Y"),
                end.format("%b %-d, %Y")
            ),
        }
    }
}

fn panel_bounds(
    anchor: Bounds<Pixels>,
    viewport: Size<Pixels>,
    content_height: Pixels,
    reveal: f32,
) -> Bounds<Pixels> {
    let horizontal_inset = PADDING.min(viewport.width / 2.).max(px(0.));
    let visible_top = TOP_EDGE_INSET.min(viewport.height).max(px(0.));
    let vertical_inset = PADDING
        .min((viewport.height - visible_top) / 2.)
        .max(px(0.));
    let reveal = reveal.clamp(0., 1.);
    let target_width = anchor.size.width.clamp(px(400.), px(520.));
    let target_height =
        (HEADER_HEIGHT + HEADER_GAP + PADDING * 2. + OVERLAY_BORDER_WIDTH * 2. + content_height)
            .min(MAX_HEIGHT);
    let width = (anchor.size.width + (target_width - anchor.size.width) * reveal)
        .min((viewport.width - horizontal_inset * 2.).max(px(0.)));
    let height = (anchor.size.height + (target_height - anchor.size.height) * reveal)
        .min((viewport.height - visible_top - vertical_inset * 2.).max(px(0.)));
    let left = if anchor.center().x > viewport.width / 2. {
        anchor.right() - width
    } else {
        anchor.left()
    }
    .clamp(
        horizontal_inset,
        (viewport.width - width - horizontal_inset).max(horizontal_inset),
    );
    let top = if anchor.center().y > (visible_top + viewport.height) / 2. {
        anchor.bottom() - height
    } else {
        anchor.top()
    }
    .clamp(
        visible_top + vertical_inset,
        (viewport.height - height - vertical_inset).max(visible_top + vertical_inset),
    );
    Bounds::new(point(left, top), size(width, height))
}

fn timeline_input_boundary<E: InteractiveElement>(element: E) -> E {
    // These MainView bindings also apply inside Overlay.
    element
        .on_action(|_: &timeline::ZoomIn, _, cx| cx.stop_propagation())
        .on_action(|_: &timeline::ZoomOut, _, cx| cx.stop_propagation())
        .on_action(|_: &timeline::ZoomReset, _, cx| cx.stop_propagation())
        .on_action(|_: &timeline::FitSelection, _, cx| cx.stop_propagation())
        .on_action(|_: &timeline::NextDivision, _, cx| cx.stop_propagation())
        .on_action(|_: &timeline::PreviousDivision, _, cx| cx.stop_propagation())
        .on_action(|_: &timeline::NextOuterDivision, _, cx| cx.stop_propagation())
        .on_action(|_: &timeline::PreviousOuterDivision, _, cx| cx.stop_propagation())
}

impl TimelineView {
    pub(in crate::views::main_view::timeline_view) fn open_marker_panel(
        &mut self,
        markers: Vec<(Marker, bool)>,
        anchor: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let manager = ItemManager::global(cx);
        manager.update(cx, |manager, cx| manager.commit_open_edit(window, cx));
        if manager.read(cx).is_editing() {
            return false;
        }
        self.marquee.end();
        self.edge_scroll.clear();
        self.force_press = None;
        self.inspect_press_item = None;
        self.inspect_force_item = None;
        self.expanded_bin = None;
        self.pending_item_focus = None;
        SelectionManager::clear_global(cx);
        self.marker_panel = Some(MarkerPanel::new(markers, anchor));
        self.marker_panel_focus.focus(window, cx);
        cx.notify();
        true
    }

    pub(in crate::views::main_view::timeline_view) fn close_marker_panel(
        &mut self,
        restore_focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.marker_panel.is_none() {
            return;
        }
        let was_focused = self.marker_panel_focus.contains_focused(window, cx);
        let destination = window.focused(cx);
        let manager = ItemManager::global(cx);
        manager.update(cx, |manager, cx| manager.commit_open_edit(window, cx));
        if manager.read(cx).is_editing() {
            return;
        }
        if let Some(panel) = self.marker_panel.take() {
            SelectionManager::global(cx).update(cx, |selection, cx| {
                for id in panel.order.ids() {
                    selection.hand_off(SelectionScope::Timeline, *id, None, cx);
                }
            });
        }
        SelectionManager::report_order(&self.selection_order(), cx);
        if restore_focus || was_focused {
            self.focus_handle.focus(window, cx);
        } else if let Some(destination) = destination {
            destination.focus(window, cx);
        }
        cx.notify();
    }

    fn navigate_marker_panel(
        &mut self,
        delta: isize,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ItemManager::global(cx).read(cx).is_editing() {
            return;
        }
        let Some(panel) = &self.marker_panel else {
            return;
        };
        let ids = panel.order.ids();
        let current = ids.iter().position(|id| {
            panel
                .focus_handles
                .get(id)
                .is_some_and(|focus| focus.contains_focused(window, cx))
        });
        let target = match current {
            Some(current) => current
                .checked_add_signed(delta)
                .filter(|index| *index < ids.len()),
            None if delta < 0 => ids.len().checked_sub(1),
            None => (!ids.is_empty()).then_some(0),
        };
        let Some(id) = target.map(|index| ids[index]) else {
            return;
        };
        let Some(focus) = panel.focus_handles.get(&id) else {
            return;
        };
        if extend {
            let origin = current.map_or(id, |index| ids[index]);
            focus_item_extending(&panel.order, origin, id, focus, window, cx);
        } else {
            focus_item(SelectionScope::Timeline, id, focus, window, cx);
        }
        cx.notify();
    }

    pub(crate) fn render_marker_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let viewport = self.bounds?.size;
        let mut panel = self.marker_panel.take()?;
        let markers = panel.rows(self.known_markers());
        let manager = ItemManager::global(cx);
        let order = SelectionOrder::new(
            SelectionScope::Timeline,
            markers.iter().filter_map(|(marker, projected)| {
                (!projected && !manager.read(cx).is_draft(marker.id)).then_some(marker.id)
            }),
        );
        SelectionManager::global(cx).update(cx, |selection, cx| {
            for id in panel
                .order
                .ids()
                .iter()
                .filter(|id| !order.ids().contains(id))
            {
                selection.hand_off(SelectionScope::Timeline, *id, None, cx);
            }
        });
        let removed_focus = panel.focus_handles.iter().any(|(id, focus)| {
            !markers
                .iter()
                .any(|(marker, projected)| !projected && marker.id == *id)
                && focus.contains_focused(window, cx)
        });
        panel.focus_handles.retain(|id, _| {
            markers
                .iter()
                .any(|(marker, projected)| !projected && marker.id == *id)
        });
        for (marker, projected) in &markers {
            if !projected {
                panel
                    .focus_handles
                    .entry(marker.id)
                    .or_insert_with(|| cx.focus_handle());
            }
        }
        if removed_focus && !manager.read(cx).is_editing() {
            self.marker_panel_focus.focus(window, cx);
        }
        panel.order = order.clone();
        SelectionManager::report_order(&order, cx);
        let title = panel.title();
        let heights: Vec<_> = markers
            .iter()
            .map(|(marker, projected)| {
                if *projected {
                    SIDEBAR_ITEM_HEIGHT
                } else {
                    self.marker_details.height(marker.id, SIDEBAR_ITEM_HEIGHT)
                }
            })
            .collect();
        let transition = window.keyed_transition(
            "timeline-marker-panel-expand",
            cx,
            transition::QUICK,
            || 0.0,
        );
        transition.set(1.0, cx);
        let reveal = ease_in_out(transition.animate(window, cx));
        let bounds = panel_bounds(
            panel.anchor,
            viewport,
            heights
                .iter()
                .copied()
                .sum::<Pixels>()
                .max(SIDEBAR_ITEM_HEIGHT),
            reveal,
        );
        let scroll = panel.scroll.clone();
        let rows = markers
            .iter()
            .enumerate()
            .map(|(index, (marker, projected))| {
                let item = AnyItem::Marker(marker.clone());
                let meta = format_item_meta(&item);
                let meta = if *projected {
                    Some(meta.map_or_else(
                        || "Projected".into(),
                        |meta| format!("{meta} · Projected").into(),
                    ))
                } else {
                    meta
                };
                let mut card = ItemCard::new_with_id(
                    ("timeline-marker-panel-item", item.id_u64()),
                    &item,
                    meta,
                    window,
                    cx,
                )
                .neighbors(
                    index.checked_sub(1).map(|index| markers[index].0.id),
                    markers.get(index + 1).map(|(marker, _)| marker.id),
                )
                .size_full()
                .actionable(!projected)
                .draggable(!projected, None);
                if let Some(focus) = panel.focus_handles.get(&item.id()) {
                    card = card
                        .with_focus_handle(focus.clone())
                        .details(self.marker_details.get(item.id()));
                    if order.ids().contains(&item.id()) {
                        card = card.selectable(order.clone());
                    }
                    card = self.wire_timeline_item_card(card, &item, cx);
                }
                let focused = window.use_keyed_state(
                    ("timeline-marker-panel-reveal", item.id_u64()),
                    cx,
                    |_, _| (false, px(0.)),
                );
                let row_scroll = scroll.clone();
                card = card.on_focus_resolved(move |bounds, focus, window, cx| {
                    let is_focused = focus.is_some_and(|focus| focus.contains_focused(window, cx));
                    let previous = focused.update(cx, |previous, _| {
                        std::mem::replace(previous, (is_focused, bounds.size.height))
                    });
                    let mut reveal = bounds;
                    reveal.size.height = reveal
                        .size
                        .height
                        .min((row_scroll.bounds().size.height - px(8.)).max(px(0.)));
                    if is_focused
                        && previous != (is_focused, bounds.size.height)
                        && row_scroll.reveal_bounds(reveal, Edges::all(px(4.)))
                    {
                        window.request_animation_frame();
                    }
                });
                div()
                    .w_full()
                    .h(heights[index])
                    .flex_none()
                    .opacity(if *projected { 0.55 } else { 1.0 })
                    .child(card)
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        self.marker_panel = Some(panel);

        let frame = div()
            .id("timeline-marker-panel")
            .key_context("Overlay")
            .track_focus(&self.marker_panel_focus)
            .tab_group()
            .tab_stop(true)
            .role(gpui::Role::Dialog)
            .aria_label(title.clone())
            .map(timeline_input_boundary)
            .on_action(cx.listener(|view, _: &CloseOverlay, window, cx| {
                if !context_menu_has_focus(window, cx) {
                    view.close_marker_panel(true, window, cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|view, _: &FocusNext, window, cx| {
                window.focus_next(cx);
                if !view.marker_panel_focus.contains_focused(window, cx) {
                    view.marker_panel_focus.focus(window, cx);
                }
            }))
            .on_action(cx.listener(|view, _: &FocusPrevious, window, cx| {
                window.focus_prev(cx);
                if !view.marker_panel_focus.contains_focused(window, cx) {
                    view.marker_panel_focus.focus(window, cx);
                }
            }))
            .on_action(move |_: &SelectAllItems, _, cx| {
                SelectionManager::global(cx).update(cx, |selection, cx| {
                    selection.select_many(SelectionScope::Timeline, order.ids().to_vec(), cx);
                });
            })
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                let modifiers = event.keystroke.modifiers;
                if event.is_held
                    || modifiers.control
                    || modifiers.platform
                    || modifiers.alt
                    || ItemManager::global(cx).read(cx).is_editing()
                    || context_menu_has_focus(window, cx)
                {
                    return;
                }
                let key = event.keystroke.key.as_str();
                let delta = match key {
                    "up" | "k" => -1,
                    "down" | "j" => 1,
                    _ => return,
                };
                view.navigate_marker_panel(
                    delta,
                    modifiers.shift && matches!(key, "up" | "down"),
                    window,
                    cx,
                );
                cx.stop_propagation();
            }))
            .on_mouse_down_out(cx.listener(|view, event: &MouseDownEvent, window, cx| {
                if !cx.has_active_drag() && !context_menu_contains_position(event.position, cx) {
                    view.close_marker_panel(false, window, cx);
                    if ItemManager::global(cx).read(cx).is_editing() {
                        SelectionManager::claim_press(cx);
                        cx.stop_propagation();
                    }
                }
            }))
            .column()
            .w(bounds.size.width)
            .h(bounds.size.height)
            .gap(HEADER_GAP)
            .p(PADDING)
            .overflow_hidden()
            .rounded(px(cx.theme().radius(Radius::Dialog)))
            .border(OVERLAY_BORDER_WIDTH)
            .border_color(cx.theme().colors.hairline)
            .text_color(cx.theme().colors.text)
            .block_mouse_except_scroll()
            .capture_any_mouse_down(cx.listener(|view, event: &MouseDownEvent, _, cx| {
                SelectionManager::claim_press(cx);
                view.force_press = None;
                view.inspect_force_item = None;
                view.inspect_press_item = if event.button == MouseButton::Left {
                    SelectionManager::global(cx)
                        .read(cx)
                        .card_at(SelectionScope::Timeline, event.position)
                } else {
                    None
                };
            }))
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .on_pinch(|_, _, cx| cx.stop_propagation())
            .on_drag_move::<DragData<DraggedItems>>(|event, window, cx| {
                if event.bounds.contains(&window.mouse_position()) {
                    cx.stop_propagation();
                }
            })
            .on_drop::<DragData<DraggedItems>>(|_, _, cx| cx.stop_propagation())
            .on_prepaint(|bounds, window, cx| {
                SelectionManager::occlude_cards(
                    SelectionScope::Timeline,
                    bounds.intersect(&window.content_mask().bounds),
                    cx,
                );
            })
            .child(
                div()
                    .row()
                    .flex_none()
                    .h(HEADER_HEIGHT)
                    .items_center()
                    .justify_between()
                    .gap_1()
                    .min_w_0()
                    .px_3()
                    .child(
                        Label::new(title)
                            .font_weight(FontWeight::SEMIBOLD)
                            .flex_1()
                            .min_w_0()
                            .truncate(),
                    )
                    .child(
                        Button::new("timeline-marker-panel-close")
                            .ghost()
                            .small()
                            .compact()
                            .px_2()
                            .icon(AppIcon::Close)
                            .role(gpui::Role::Button)
                            .aria_label("Close markers")
                            .tooltip("Close markers")
                            .on_click(cx.listener(|view, _, window, cx| {
                                cx.stop_propagation();
                                view.close_marker_panel(true, window, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .id("timeline-marker-panel-entries")
                            .column()
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            .children(rows)
                            .when(markers.is_empty(), |list| {
                                list.child(
                                    div()
                                        .h(SIDEBAR_ITEM_HEIGHT)
                                        .flex_none()
                                        .px_3()
                                        .row()
                                        .items_center()
                                        .child(
                                            Label::new("No markers")
                                                .text_sm()
                                                .text_color(cx.theme().colors.text_muted),
                                        ),
                                )
                            }),
                    )
                    .child(Scrollbar::new(
                        "timeline-marker-panel-scrollbar",
                        &scroll,
                        gpui::Axis::Vertical,
                    )),
            );
        Some(
            div()
                .absolute()
                .left(bounds.left())
                .top(bounds.top())
                .w(bounds.size.width)
                .h(bounds.size.height)
                .opacity(reveal)
                .child(
                    frame
                        .bg_frost()
                        .glass_surface(Surface::Overlay)
                        .glass_radius(Radius::Dialog)
                        .glass(|glass| glass.tint(frosted_overlay_tint(cx.theme()))),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subroutine_core::{Recurrence, RecurrenceRule};

    #[test]
    fn refresh_follows_concrete_identity_without_retaining_deleted_data() {
        let date = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let mut marker = Marker::new("Original", date);
        let anchor = Bounds::new(point(px(100.), px(200.)), size(px(44.), px(26.)));
        let mut panel = MarkerPanel::new(vec![(marker.clone(), false)], anchor);
        marker.date = date.succ_opt().unwrap();
        marker.title = "Edited".into();
        let unrelated = Marker::new("Same original date", date);

        let rows = panel.rows([&marker, &unrelated].into_iter());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0.id, marker.id);
        assert_eq!(rows[0].0.date, marker.date);
        assert_eq!(rows[0].0.title, "Edited");
        assert!(!rows[0].1);
        assert_eq!(panel.scope, Some((date, date)));
        assert_eq!(panel.anchor, anchor);
        assert!(panel.rows([&unrelated].into_iter()).is_empty());
    }

    #[test]
    fn materialized_projection_keeps_its_identity_after_moving_and_deletion() {
        let date = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let mut source =
            Marker::new("Original", date).with_recurrence(Recurrence::new(RecurrenceRule::days(1)));
        let mut occurrence = source.next_recurrence().unwrap();
        let mut panel = MarkerPanel::new(vec![(occurrence.clone(), true)], Bounds::default());
        source.title = "Updated source".into();

        let rows = panel.rows([&source].into_iter());
        assert_eq!(rows.len(), 1);
        assert!(rows[0].1);
        assert_eq!(rows[0].0.title, source.title);
        assert!(panel.rows(std::iter::empty()).is_empty());

        // Materialization can assign a different id from the deterministic ghost id.
        occurrence.id = Uuid::from_u128(42);
        let rows = panel.rows([&source, &occurrence].into_iter());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0.id, occurrence.id);
        assert!(!rows[0].1);

        occurrence.date = occurrence.date.succ_opt().unwrap();
        let rows = panel.rows([&source, &occurrence].into_iter());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0.id, occurrence.id);
        assert_eq!(rows[0].0.date, occurrence.date);
        assert!(!rows[0].1);
        assert!(panel.rows([&source].into_iter()).is_empty());
    }

    #[test]
    fn clamps_offscreen_anchors_to_the_local_viewport() {
        let viewport = size(px(640.), px(800.));
        for origin in [point(px(-1000.), px(-1000.)), point(px(2000.), px(2000.))] {
            let bounds = panel_bounds(
                Bounds::new(origin, size(px(100.), px(26.))),
                viewport,
                px(2000.),
                1.0,
            );
            assert!(bounds.left() >= px(0.) && bounds.right() <= viewport.width);
            assert!(bounds.top() >= TOP_EDGE_INSET && bounds.bottom() <= viewport.height);
            assert!(bounds.size.height <= MAX_HEIGHT);
        }
    }

    #[test]
    fn tiny_viewports_do_not_produce_negative_or_overflowing_bounds() {
        let anchor = Bounds::new(point(px(100.), px(100.)), size(px(44.), px(26.)));
        for viewport in [size(px(0.), px(0.)), size(px(10.), px(10.))] {
            let bounds = panel_bounds(anchor, viewport, SIDEBAR_ITEM_HEIGHT, 1.0);
            assert!(bounds.size.width >= px(0.) && bounds.size.height >= px(0.));
            assert!(bounds.left() >= px(0.) && bounds.right() <= viewport.width);
            assert!(bounds.top() >= px(0.) && bounds.bottom() <= viewport.height);
        }
    }
}
