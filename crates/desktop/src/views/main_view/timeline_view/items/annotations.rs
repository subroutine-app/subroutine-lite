use std::{cell::Cell, collections::HashSet, rc::Rc};

use chrono::{DateTime, Duration as ChronoDuration, Local, NaiveDate, Utc};
use gpui::{
    AnyElement, App, Bounds, Context, InteractiveElement, IntoElement, MouseButton, ParentElement,
    Pixels, StatefulInteractiveElement as _, Styled, Window, canvas, div, point,
    prelude::FluentBuilder as _, px, size,
};
use gpui_kit::foundation::{Selectable as _, StyledExt as _};
use gpui_kit::layout::ScrollArea;
use gpui_kit::overlay::{
    Hang, Overlay, OverlaySurface, Placement, popover::anchored_slot, surface,
};
use gpui_kit_theme::ActiveTheme;
use subroutine_core::{AnyItem, ItemType, Marker, SchedulePoint, Signal};
use uuid::Uuid;

use crate::AppIcon;
use crate::components::{Button, ButtonVariants, Divider, DragData, DraggedItems, ItemCard, Label};
use crate::item_manager::ItemManager;
use crate::selection::{
    CompleteSelected, CopySelected, CutSelected, DeleteSelected, DuplicateSelected, PasteItems,
    SelectAllItems, SelectionManager, SelectionScope, TogglePinnedSelected, ToggleQueuedSelected,
};
use crate::views::TOP_EDGE_INSET;

use super::super::{
    CloseMarkerPanel, EDGE_HORIZON, TimelineView, toolbar::toolbar_right_clearance,
};
use super::{
    ANNOTATION_CHIP_GAP, ANNOTATION_CHIP_HEIGHT, ANNOTATION_INSET, ANNOTATION_RIGHT_PADDING,
    ATTACHED_ITEM_LEFT, MIN_ANNOTATION_CHIP_WIDTH, MIN_ANNOTATION_GUTTER,
    STICKY_OUTER_LABEL_HEIGHT, STICKY_OUTER_LABEL_INSET, annotation_gutter_for,
    sticky_outer_label_clearance,
};

const GHOST_OPACITY: f32 = 0.45;
const GHOST_LINE_OPACITY: f32 = 0.7;

const LINE_STROKE: Pixels = px(1.);
const MARKER_BUTTON_WIDTH: Pixels = px(44.);
const MARKER_PANEL_WIDTH: Pixels = px(340.);
const MARKER_PANEL_MAX_LIST_HEIGHT: Pixels = px(280.);
const MARKER_ROW_HEIGHT: Pixels = px(42.);
const MARKER_ROW_GAP: Pixels = px(4.);

const MAX_ANNOTATION_RANGE: ChronoDuration = ChronoDuration::days(45);
const MAX_ANNOTATIONS: usize = 128;

const DETAIL_START_DAY_HEIGHT: Pixels = px(28.);
const DETAIL_FULL_DAY_HEIGHT: Pixels = px(72.);
pub(super) const SIGNAL_CARD_HEIGHT: Pixels = px(42.);
const COMPACT_SIGNAL_WIDTH: Pixels = px(8.);
const COMPACT_ANNOTATION_HEIGHT: Pixels = px(4.);
const COMPACT_LINE_OPACITY: f32 = 0.24;
const COMPACT_GLYPH_OPACITY: f32 = 0.55;

fn annotation_detail(day_height: Pixels) -> f32 {
    ((day_height - DETAIL_START_DAY_HEIGHT) / (DETAIL_FULL_DAY_HEIGHT - DETAIL_START_DAY_HEIGHT))
        .clamp(0.0, 1.0)
}

fn lerp_pixels(from: Pixels, to: Pixels, amount: f32) -> Pixels {
    from + (to - from) * amount
}

fn lerp(from: f32, to: f32, amount: f32) -> f32 {
    from + (to - from) * amount
}

fn annotation_opacities(detail: f32, projection: bool) -> (f32, f32, f32) {
    if projection {
        (
            detail * GHOST_LINE_OPACITY,
            detail * GHOST_OPACITY,
            detail * (1.0 - detail) * COMPACT_GLYPH_OPACITY,
        )
    } else {
        (
            lerp(COMPACT_LINE_OPACITY, 1.0, detail),
            detail,
            (1.0 - detail) * COMPACT_GLYPH_OPACITY,
        )
    }
}

fn signal_card_detail(detail: f32, held: bool) -> f32 {
    if held { 1.0 } else { detail }
}

fn signal_is_on_screen(top: Pixels, height: Pixels, viewport_height: Pixels) -> bool {
    top + height >= px(0.) && top <= viewport_height
}

pub(super) struct SignalLayout {
    pub signal: Signal,
    pub projection: bool,
    pub top: Pixels,
    pub height: Pixels,
    anchor: Pixels,
    width: Pixels,
    detail: f32,
}

fn pack_signal_cards(cards: &mut [SignalLayout]) {
    let mut occupied = Vec::new();
    for card in cards.iter_mut().filter(|card| !card.projection) {
        if let Some((_, bottom)) = occupied.last() {
            card.top = card.top.max(*bottom + ANNOTATION_CHIP_GAP);
        }
        occupied.push((card.top, card.top + card.height));
    }
    for card in cards.iter_mut().filter(|card| card.projection) {
        for (top, bottom) in &occupied {
            if card.top + card.height + ANNOTATION_CHIP_GAP <= *top {
                break;
            }
            if card.top < *bottom + ANNOTATION_CHIP_GAP {
                card.top = *bottom + ANNOTATION_CHIP_GAP;
            }
        }
        let index = occupied.partition_point(|(top, _)| *top <= card.top);
        occupied.insert(index, (card.top, card.top + card.height));
    }
}

pub(super) fn day_start(date: NaiveDate) -> DateTime<Local> {
    SchedulePoint::Date(date).into()
}

fn marker_is_active_on(marker: &Marker, date: NaiveDate) -> bool {
    marker.date <= date && marker.end_date.unwrap_or(marker.date) >= date
}

fn marker_button_bounds() -> Bounds<Pixels> {
    Bounds::new(
        point(
            STICKY_OUTER_LABEL_INSET,
            TOP_EDGE_INSET + (STICKY_OUTER_LABEL_HEIGHT - ANNOTATION_CHIP_HEIGHT) / 2.,
        ),
        size(MARKER_BUTTON_WIDTH, ANNOTATION_CHIP_HEIGHT),
    )
}

fn marker_command_boundary<T: InteractiveElement>(element: T) -> T {
    element
        .on_action(|_: &CompleteSelected, _, cx| cx.stop_propagation())
        .on_action(|_: &CopySelected, _, cx| cx.stop_propagation())
        .on_action(|_: &CutSelected, _, cx| cx.stop_propagation())
        .on_action(|_: &DeleteSelected, _, cx| cx.stop_propagation())
        .on_action(|_: &DuplicateSelected, _, cx| cx.stop_propagation())
        .on_action(|_: &PasteItems, _, cx| cx.stop_propagation())
        .on_action(|_: &SelectAllItems, _, cx| cx.stop_propagation())
        .on_action(|_: &TogglePinnedSelected, _, cx| cx.stop_propagation())
        .on_action(|_: &ToggleQueuedSelected, _, cx| cx.stop_propagation())
}

fn sort_marker_rows(markers: &mut [(Marker, bool)], draft: Option<Uuid>) {
    markers.sort_by_key(|(marker, projection)| {
        (
            Some(marker.id) != draft,
            *projection,
            marker.date,
            marker.lineage_id,
        )
    });
}

fn marker_list_height(
    heights: impl IntoIterator<Item = Pixels>,
    viewport_height: Pixels,
) -> Pixels {
    let rows = heights
        .into_iter()
        .enumerate()
        .map(|(index, height)| height + if index == 0 { px(0.) } else { MARKER_ROW_GAP })
        .sum::<Pixels>()
        .max(MARKER_ROW_HEIGHT);
    rows.min(MARKER_PANEL_MAX_LIST_HEIGHT)
        .min((viewport_height - marker_button_bounds().bottom() - px(64.)).max(px(0.)))
}

fn sticky_annotation_top(top: Pixels, bottom: Pixels, height: Pixels) -> Pixels {
    let ceiling = TOP_EDGE_INSET + sticky_outer_label_clearance();
    let floor = (bottom - height - px(4.)).max(top);
    top.max(ceiling).min(floor)
}

pub(super) fn sticky_chip_top(top: Pixels, bottom: Pixels) -> Pixels {
    sticky_annotation_top(top, bottom, ANNOTATION_CHIP_HEIGHT)
}

#[derive(Default)]
pub(crate) struct AnnotationCache {
    key: Option<(NaiveDate, NaiveDate, bool, u64)>,
    markers: Vec<(Marker, bool)>,
    signals: Vec<(Signal, bool)>,
}

impl TimelineView {
    pub fn refresh_markers(&mut self, markers: Vec<Marker>, cx: &mut Context<Self>) {
        self.markers = markers;
        self.draft_markers
            .retain(|draft| !self.markers.iter().any(|marker| marker.id == draft.id));
        self.invalidate_annotations();
        cx.notify();
    }

    pub fn refresh_signals(&mut self, signals: Vec<Signal>, cx: &mut Context<Self>) {
        self.signals = signals;
        self.draft_signals
            .retain(|draft| !self.signals.iter().any(|signal| signal.id == draft.id));
        let ids: HashSet<_> = self.known_signals().map(|signal| signal.id).collect();
        self.signal_focus_handles.retain(|id, _| ids.contains(id));
        for id in &ids {
            self.signal_focus_handles
                .entry(*id)
                .or_insert_with(|| cx.focus_handle());
        }
        if self.pending_item_focus.is_some_and(|id| {
            !ids.contains(&id) && !self.items.iter().any(|entry| entry.item.id() == id)
        }) {
            self.pending_item_focus = None;
        }
        self.invalidate_annotations();
        cx.notify();
    }

    pub(crate) fn invalidate_annotations(&mut self) {
        self.annotations_revision = self.annotations_revision.wrapping_add(1);
    }

    fn known_markers(&self) -> impl Iterator<Item = &Marker> {
        self.markers.iter().chain(self.draft_markers.iter())
    }

    pub(in crate::views::main_view::timeline_view) fn known_signals(
        &self,
    ) -> impl Iterator<Item = &Signal> {
        self.signals.iter().chain(self.draft_signals.iter())
    }

    pub(in crate::views::main_view::timeline_view) fn drawn_range(
        &self,
    ) -> (DateTime<Local>, DateTime<Local>) {
        let height = self.bounds.map(|b| b.size.height).unwrap_or(px(800.));
        let horizon_seconds =
            (height + EDGE_HORIZON * 2.).as_f32() / 2. * self.pixel_duration.as_seconds_f32();
        let horizon = ChronoDuration::seconds(horizon_seconds.round() as i64);
        let center = self.scroll_position();
        (center - horizon, center + horizon)
    }

    pub(super) fn annotation_gutter_width(&self) -> Pixels {
        let clearance = toolbar_right_clearance(self.effective_toolbar_position());
        let Some(bounds) = self.bounds else {
            return MIN_ANNOTATION_GUTTER + clearance;
        };
        let flexible = (bounds.size.width - self.item_area_left()).max(px(0.));
        annotation_gutter_for(flexible, clearance)
    }

    pub(super) fn annotation_left(&self) -> Pixels {
        self.item_area_left() + self.item_area_width() + ANNOTATION_INSET
    }

    pub(super) fn chip_left(&self) -> Pixels {
        self.annotation_left() + ANNOTATION_CHIP_GAP
    }

    pub(super) fn chip_width(&self) -> Pixels {
        let right_edge = self
            .bounds
            .map(|bounds| bounds.size.width)
            .unwrap_or(px(720.));
        (right_edge
            - ANNOTATION_RIGHT_PADDING
            - toolbar_right_clearance(self.effective_toolbar_position())
            - self.chip_left())
        .max(MIN_ANNOTATION_CHIP_WIDTH)
    }

    fn sync_annotations(&mut self) {
        let (start, end) = self.drawn_range();
        let project_recurrences = end - start <= MAX_ANNOTATION_RANGE;
        let key = (
            start.date_naive(),
            end.date_naive(),
            project_recurrences,
            self.annotations_revision,
        );
        if self.annotation_cache.key == Some(key) {
            return;
        }

        let (markers, signals) = if !project_recurrences {
            let mut markers: Vec<(Marker, bool)> = self
                .known_markers()
                .filter(|marker| {
                    marker.date <= end.date_naive()
                        && marker.end_date.unwrap_or(marker.date) >= start.date_naive()
                })
                .cloned()
                .map(|marker| (marker, false))
                .collect();
            markers.sort_by_key(|(marker, _)| (marker.date, marker.lineage_id));
            markers.truncate(MAX_ANNOTATIONS);

            let start_utc = start.with_timezone(&Utc);
            let end_utc = end.with_timezone(&Utc);
            let mut signals: Vec<(Signal, bool)> = self
                .known_signals()
                .filter(|signal| signal.datetime >= start_utc && signal.datetime <= end_utc)
                .cloned()
                .map(|signal| (signal, false))
                .collect();
            signals.sort_by_key(|(signal, _)| (signal.datetime, signal.lineage_id));
            signals.truncate(MAX_ANNOTATIONS);
            (markers, signals)
        } else {
            (
                self.project_markers(start.date_naive(), end.date_naive()),
                self.project_signals(start.into(), end.into()),
            )
        };
        self.annotation_cache = AnnotationCache {
            key: Some(key),
            markers,
            signals,
        };
    }

    fn project_markers(&self, start: NaiveDate, end: NaiveDate) -> Vec<(Marker, bool)> {
        let mut projected: Vec<Marker> = self
            .known_markers()
            .flat_map(|marker| marker.projections_between(start, end))
            .filter(|ghost| {
                !self.known_markers().any(|marker| {
                    marker.lineage_id == ghost.lineage_id && marker.date == ghost.date
                })
            })
            .collect();
        projected.sort_by_key(|marker| (marker.date, marker.lineage_id));
        projected.dedup_by_key(|marker| (marker.date, marker.lineage_id));

        let mut visible: Vec<(Marker, bool)> = self
            .known_markers()
            .filter(|marker| marker.date <= end && marker.end_date.unwrap_or(marker.date) >= start)
            .cloned()
            .map(|marker| (marker, false))
            .chain(projected.into_iter().map(|marker| (marker, true)))
            .collect();
        visible.sort_by_key(|(marker, _)| (marker.date, marker.lineage_id));
        visible.truncate(MAX_ANNOTATIONS);
        visible
    }

    fn project_signals(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Vec<(Signal, bool)> {
        let mut visible: Vec<(Signal, bool)> = self
            .known_signals()
            .filter(|signal| signal.datetime >= start && signal.datetime <= end)
            .cloned()
            .map(|signal| (signal, false))
            .collect();
        for signal in self.known_signals() {
            visible.extend(
                signal
                    .projections_between(start, end)
                    .into_iter()
                    .filter(|ghost| {
                        !self.known_signals().any(|stored| {
                            stored.lineage_id == ghost.lineage_id
                                && stored.datetime == ghost.datetime
                        })
                    })
                    .map(|ghost| (ghost, true)),
            );
        }
        visible.sort_by_key(|(signal, _)| (signal.datetime, signal.lineage_id));
        visible.dedup_by_key(|(signal, _)| (signal.datetime, signal.lineage_id));
        visible.truncate(MAX_ANNOTATIONS);
        visible
    }

    fn marker_header_date(&self) -> Option<NaiveDate> {
        let bounds = self.bounds?;
        let sample_y = TOP_EDGE_INSET + STICKY_OUTER_LABEL_HEIGHT / 2.;
        Some(
            self.position_to_time(sample_y - bounds.size.center().y)
                .date_naive(),
        )
    }

    fn active_header_markers(&self) -> impl Iterator<Item = &(Marker, bool)> {
        let date = self.marker_header_date();
        self.annotation_cache
            .markers
            .iter()
            .filter(move |(marker, _)| date.is_some_and(|date| marker_is_active_on(marker, date)))
    }

    pub(super) fn close_marker_panel(
        &mut self,
        restore_focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.marker_panel_date.take().is_none() {
            return;
        }
        self.marker_panel_draft = None;
        let manager = ItemManager::global(cx);
        if manager
            .read(cx)
            .editing_item
            .as_ref()
            .is_some_and(|editing| editing.item_type() == ItemType::Marker)
        {
            manager.update(cx, |manager, cx| {
                manager.commit_open_edit(window, cx);
            });
        }
        if restore_focus {
            self.focus_handle.focus(window, cx);
        }
        cx.notify();
    }

    pub(crate) fn render_marker_header(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.sync_annotations();
        let date = self
            .marker_panel_date
            .or_else(|| self.marker_header_date())?;
        let open = self.marker_panel_date.is_some();
        let mut markers = if open {
            self.project_markers(date, date)
        } else {
            self.active_header_markers().cloned().collect::<Vec<_>>()
        };

        if open {
            for marker in self.known_markers() {
                if (self.is_being_edited(marker.id, cx)
                    || (self.marker_panel_draft == Some(marker.id)
                        && marker_is_active_on(marker, date)))
                    && !markers.iter().any(|(shown, _)| shown.id == marker.id)
                {
                    markers.push((marker.clone(), false));
                }
            }
        }
        if markers.is_empty() && !open {
            return None;
        }
        sort_marker_rows(&mut markers, self.marker_panel_draft);
        let count = markers.len();
        if open
            && self
                .marker_panel_draft
                .is_some_and(|id| !self.is_being_edited(id, cx))
            && self.focus_handle.is_focused(window)
        {
            self.marker_trigger_focus.focus(window, cx);
        }
        let bounds = marker_button_bounds();
        let trigger_bounds = Rc::new(Cell::new(None::<Bounds<Pixels>>));
        let measured = trigger_bounds.clone();
        let trigger = Button::new("timeline-markers")
            .ghost()
            .xsmall()
            .compact()
            .w(bounds.size.width)
            .h(bounds.size.height)
            .rounded_lg()
            .icon(AppIcon::Calendar)
            .label(count.to_string())
            .tooltip(format!(
                "{count} {} · {}",
                if count == 1 { "marker" } else { "markers" },
                date.format("%a %b %-d")
            ))
            .text_color(cx.theme().colors.text_muted)
            .when(!open, |button| button.bg(cx.theme().colors.raised))
            .selected(open)
            .track_focus(&self.marker_trigger_focus)
            .key_context("TimelineMarkers")
            .map(marker_command_boundary)
            .block_mouse_except_scroll()
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                SelectionManager::claim_press(cx);
                cx.stop_propagation();
            })
            .on_action(cx.listener(|view, _: &CloseMarkerPanel, window, cx| {
                view.close_marker_panel(true, window, cx);
            }))
            .on_click(cx.listener(move |view, _, window, cx| {
                cx.stop_propagation();
                if view.marker_panel_date.is_some() {
                    view.close_marker_panel(true, window, cx);
                } else {
                    view.marker_panel_date = Some(date);
                    view.marker_panel_draft = None;
                    view.marker_panel_scroll.set_offset(point(px(0.), px(0.)));
                    view.marker_trigger_focus.focus(window, cx);
                    cx.notify();
                }
            }))
            .child(
                canvas(
                    move |bounds, _, _| measured.set(Some(bounds)),
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .into_any_element();

        let panel = open.then(|| {
            let list_height = marker_list_height(
                markers.iter().map(|(marker, projection)| {
                    if *projection {
                        MARKER_ROW_HEIGHT
                    } else {
                        self.marker_details.height(marker.id, MARKER_ROW_HEIGHT)
                    }
                }),
                window.viewport_size().height,
            );
            let rows = markers
                .into_iter()
                .map(|(marker, projection)| {
                    let item = AnyItem::Marker(marker);
                    let row_height = if projection {
                        MARKER_ROW_HEIGHT
                    } else {
                        self.marker_details.height(item.id(), MARKER_ROW_HEIGHT)
                    };
                    let focused = window.use_keyed_state(
                        ("timeline-marker-focus-reveal", item.id_u64()),
                        cx,
                        |_, _| (false, px(0.)),
                    );
                    let scroll = self.marker_panel_scroll.clone();
                    div()
                        .h(row_height)
                        .flex_none()
                        .w_full()
                        .opacity(if projection { GHOST_OPACITY } else { 1.0 })
                        .child(
                            ItemCard::new_with_id(
                                ("timeline-marker-panel", item.id_u64()),
                                &item,
                                None,
                                window,
                                cx,
                            )
                            .size_full()
                            .when(!projection, |card| {
                                card.details(self.marker_details.get(item.id()))
                            })
                            .title_only(true)
                            .border(false)
                            .actionable(!projection)
                            .draggable(!projection, None)
                            .on_focus_resolved(move |bounds, handle, window, cx| {
                                let is_focused = handle
                                    .is_some_and(|handle| handle.contains_focused(window, cx));
                                let (was_focused, previous_height) =
                                    focused.update(cx, |previous, _| {
                                        std::mem::replace(
                                            previous,
                                            (is_focused, bounds.size.height),
                                        )
                                    });
                                let mut reveal = bounds;
                                reveal.size.height = reveal
                                    .size
                                    .height
                                    .min((scroll.bounds().size.height - px(8.)).max(px(0.)));
                                if is_focused
                                    && (!was_focused || previous_height != bounds.size.height)
                                    && scroll.reveal_bounds(reveal, gpui::Edges::all(px(4.)))
                                {
                                    window.request_animation_frame();
                                }
                            })
                            .block_mouse_except_scroll(),
                        )
                        .into_any_element()
                })
                .collect::<Vec<_>>();
            let body = surface(
                "timeline-markers.surface",
                cx.theme(),
                OverlaySurface::FLOATING,
            )
            .w(MARKER_PANEL_WIDTH.min((window.viewport_size().width - px(24.)).max(px(0.))))
            .p_2()
            .column()
            .gap_1()
            .key_context("TimelineMarkers")
            .map(marker_command_boundary)
            .on_action(cx.listener(|view, _: &CloseMarkerPanel, window, cx| {
                view.close_marker_panel(true, window, cx);
            }))
            .on_mouse_down_out(cx.listener(
                move |view, event: &gpui::MouseDownEvent, window, cx| {
                    if trigger_bounds
                        .get()
                        .is_none_or(|bounds| !bounds.contains(&event.position))
                    {
                        view.close_marker_panel(false, window, cx);
                    }
                },
            ))
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                SelectionManager::claim_press(cx);
                cx.stop_propagation();
            })
            .on_drag_move::<DragData<DraggedItems>>(cx.listener(|view, _, _, cx| {
                view.marker_panel_date = None;
                view.marker_panel_draft = None;
                cx.notify();
            }))
            .child(
                div()
                    .row()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .child(Label::new(format!("Markers · {}", date.format("%a %b %-d"))).text_sm())
                    .child(
                        Button::new("timeline-markers.close")
                            .ghost()
                            .xsmall()
                            .compact()
                            .icon(AppIcon::Close)
                            .tooltip("Close markers")
                            .on_click(cx.listener(|view, _, window, cx| {
                                cx.stop_propagation();
                                view.close_marker_panel(true, window, cx);
                            })),
                    ),
            )
            .child(
                ScrollArea::new("timeline-markers.list")
                    .vertical()
                    .height(list_height.as_f32())
                    .bound_to(self.marker_panel_scroll.clone())
                    .child(
                        div()
                            .id("timeline-markers.entries")
                            .w_full()
                            .h(list_height)
                            .overflow_y_scroll()
                            .track_scroll(&self.marker_panel_scroll)
                            .child(
                                div()
                                    .column()
                                    .w_full()
                                    .gap(MARKER_ROW_GAP)
                                    .children(rows)
                                    .when(count == 0, |list| {
                                        list.child(
                                            Label::new("No markers for this day")
                                                .text_sm()
                                                .text_color(cx.theme().colors.text_muted),
                                        )
                                    }),
                            ),
                    ),
            );
            Overlay::new("timeline-markers.overlay")
                .placement(Placement::Below)
                .hang(Hang::Start)
                .window_snap_margin(px(8.))
                .child(div().pt_1().child(body))
                .into_any_element()
        });
        Some(
            div()
                .absolute()
                .left(bounds.left())
                .top(bounds.top())
                .child(anchored_slot(Placement::Below, Hang::Start, trigger, panel))
                .into_any_element(),
        )
    }

    pub(crate) fn render_signal_ticks(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        self.sync_annotations();
        SelectionManager::report_order(&self.selection_order(), cx);
        self.build_signal_ticks(window, cx)
    }

    fn signal_is_held(&self, id: Uuid, cx: &App) -> bool {
        self.pending_item_focus == Some(id)
            || self.signal_details.is_open(id)
            || self.is_being_edited(id, cx)
            || SelectionManager::global(cx)
                .read(cx)
                .is_selected_in(SelectionScope::Timeline, id)
    }

    fn signal_is_dragged(&self, id: Uuid) -> bool {
        self.active_drop
            .as_ref()
            .is_some_and(|drop| drop.dragged.contains(&id))
    }

    pub(super) fn signal_layout(&self, cx: &App) -> Vec<SignalLayout> {
        let detail = annotation_detail(self.duration_to_height(ChronoDuration::days(1)));
        let mut signals = self
            .known_signals()
            .cloned()
            .map(|signal| (signal, false))
            .chain(
                self.annotation_cache
                    .signals
                    .iter()
                    .filter(|(_, projection)| *projection)
                    .cloned(),
            )
            .filter(|(signal, _)| !self.signal_is_dragged(signal.id))
            .collect::<Vec<_>>();
        signals.sort_by_key(|(signal, projection)| (signal.datetime, *projection, signal.id));
        let mut cards = signals
            .into_iter()
            .filter_map(|(signal, projection)| {
                let detail =
                    signal_card_detail(detail, !projection && self.signal_is_held(signal.id, cx));
                if projection && detail == 0. {
                    return None;
                }
                let collapsed_height =
                    lerp_pixels(COMPACT_ANNOTATION_HEIGHT, SIGNAL_CARD_HEIGHT, detail);
                let height = if projection {
                    collapsed_height
                } else {
                    self.signal_details.height(signal.id, collapsed_height)
                };
                let anchor = self.time_to_offset(signal.datetime.with_timezone(&Local));
                Some(SignalLayout {
                    signal,
                    projection,
                    top: anchor - height / 2.,
                    anchor,
                    height,
                    width: lerp_pixels(COMPACT_SIGNAL_WIDTH, self.chip_width(), detail),
                    detail,
                })
            })
            .collect::<Vec<_>>();
        pack_signal_cards(&mut cards);
        cards
    }

    fn build_signal_ticks(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(bounds) = self.bounds else {
            return Vec::new();
        };
        let scroll_y = self.scroll_offset + self.center_relative().y;
        let color = cx.theme().colors.hairline_strong;
        let neutral = cx.theme().colors.text_muted;
        let tick_left = ATTACHED_ITEM_LEFT;
        let chip_left = self.chip_left();
        let connector_left = chip_left - ANNOTATION_CHIP_GAP;
        let order = self.selection_order();

        let mut elements = Vec::new();
        for layout in self.signal_layout(cx) {
            let SignalLayout {
                signal,
                projection: is_projection,
                top,
                height: chip_height,
                anchor,
                width: chip_width,
                detail,
            } = layout;
            let y = anchor + scroll_y;
            let top = top + scroll_y;
            let card_visible = signal_is_on_screen(top, chip_height, bounds.size.height);
            if !card_visible && !signal_is_on_screen(y, LINE_STROKE, bounds.size.height) {
                continue;
            }
            let card_center = top + chip_height / 2.;
            let item = AnyItem::Signal(signal.clone());
            let (line_opacity, card_opacity, compact_opacity) =
                annotation_opacities(detail, is_projection);

            elements.push(
                div()
                    .row()
                    .absolute()
                    .top(y - LINE_STROKE / 2.)
                    .left(tick_left)
                    .w((connector_left - tick_left).max(px(0.)))
                    .h(LINE_STROKE)
                    .gap_1()
                    .child(Divider::horizontal().w_2().color(color))
                    .child(Divider::horizontal().flex_1().dashed().color(color))
                    .opacity(line_opacity)
                    .into_any_element(),
            );
            elements.push(
                div()
                    .absolute()
                    .left(connector_left)
                    .top(y.min(card_center))
                    .w(LINE_STROKE)
                    .h((card_center - y).abs())
                    .bg(color)
                    .opacity(line_opacity)
                    .into_any_element(),
            );
            if !card_visible {
                continue;
            }
            elements.push(
                div()
                    .absolute()
                    .left(connector_left)
                    .top(card_center - LINE_STROKE / 2.)
                    .w(ANNOTATION_CHIP_GAP)
                    .h(LINE_STROKE)
                    .bg(color)
                    .opacity(line_opacity)
                    .into_any_element(),
            );
            let card = (!is_projection || detail > 0.0).then(|| {
                let meta = Some(
                    signal
                        .datetime
                        .with_timezone(&Local)
                        .format("%a %b %-d · %-I:%M %p")
                        .to_string()
                        .into(),
                );
                let mut card = ItemCard::new_with_id(
                    ("timeline-signal", item.id_u64()),
                    &item,
                    meta,
                    window,
                    cx,
                )
                .size_full()
                .title_only(true)
                .actionable(!is_projection)
                .draggable(!is_projection, None)
                .block_mouse_except_scroll();
                if !is_projection {
                    card = card.details(self.signal_details.get(signal.id));
                    if self.signal_details.is_open(signal.id) {
                        let offset = (TOP_EDGE_INSET - top + px(6.))
                            .max(px(6.))
                            .min((chip_height - SIGNAL_CARD_HEIGHT).max(px(0.)) + px(6.));
                        card = card.content_top(offset);
                    }
                    if let Some(handle) = self.signal_focus_handles.get(&signal.id) {
                        card = card.with_focus_handle(handle.clone());
                        if self.pending_item_focus == Some(signal.id) {
                            let handle = handle.clone();
                            self.pending_item_focus = None;
                            cx.on_next_frame(window, move |_, window, cx| {
                                handle.focus(window, cx);
                            });
                        }
                    }
                    card = self.wire_timeline_item_card(card.selectable(order.clone()), &item, cx);
                }
                div().size_full().opacity(card_opacity).child(card)
            });
            elements.push(
                div()
                    .absolute()
                    .top(top)
                    .left(chip_left)
                    .w(chip_width)
                    .h(chip_height)
                    .when(top + chip_height <= TOP_EDGE_INSET, |chip| chip.invisible())
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top((chip_height - COMPACT_ANNOTATION_HEIGHT) / 2.)
                            .w(COMPACT_SIGNAL_WIDTH.min(chip_width))
                            .h(COMPACT_ANNOTATION_HEIGHT)
                            .rounded_full()
                            .bg(neutral)
                            .opacity(compact_opacity),
                    )
                    .children(card)
                    .into_any_element(),
            );
        }
        elements
    }
}
