use std::collections::HashSet;

use chrono::{DateTime, Duration as ChronoDuration, Local, NaiveDate, Utc};
use gpui::{
    AnyElement, App, Context, InteractiveElement, IntoElement, ParentElement, Pixels, Styled,
    Window, div, prelude::FluentBuilder as _, px,
};
use gpui_kit::foundation::StyledExt as _;
use gpui_kit_theme::ActiveTheme;
use subroutine_core::{AnyItem, Marker, SchedulePoint, Signal};
use uuid::Uuid;

use crate::components::{Divider, ItemCard};
use crate::selection::{SelectionManager, SelectionScope};
use crate::views::TOP_EDGE_INSET;

use super::super::{EDGE_HORIZON, TimelineView, toolbar::toolbar_right_clearance};
use super::{
    ANNOTATION_CHIP_GAP, ANNOTATION_CHIP_HEIGHT, ANNOTATION_INSET, ANNOTATION_RIGHT_PADDING,
    ATTACHED_ITEM_LEFT, MIN_ANNOTATION_CHIP_WIDTH, MIN_ANNOTATION_GUTTER, annotation_gutter_for,
    sticky_outer_label_clearance,
};

const GHOST_OPACITY: f32 = 0.45;
const GHOST_LINE_OPACITY: f32 = 0.7;

const LINE_STROKE: Pixels = px(1.);

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

    pub(in crate::views::main_view::timeline_view) fn known_markers(
        &self,
    ) -> impl Iterator<Item = &Marker> {
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

    pub(super) fn sync_annotations(&mut self) {
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

        let signals = if !project_recurrences {
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
            signals
        } else {
            self.project_signals(start.into(), end.into())
        };
        self.annotation_cache = AnnotationCache {
            key: Some(key),
            signals,
        };
    }

    pub(super) fn project_markers(&self, start: NaiveDate, end: NaiveDate) -> Vec<(Marker, bool)> {
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

    pub(crate) fn render_signal_ticks(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        self.sync_annotations();
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
            let card_center = self.geometry.y(top + chip_height / 2.);
            let connector_top = self.geometry.y(top + (chip_height - LINE_STROKE) / 2.);
            let tick_top = self.geometry.y(anchor - LINE_STROKE / 2.);
            let y = self.geometry.y(anchor);
            let top = self.geometry.y(top);
            let card_visible = signal_is_on_screen(top, chip_height, bounds.size.height);
            if !card_visible && !signal_is_on_screen(y, LINE_STROKE, bounds.size.height) {
                continue;
            }

            let item = AnyItem::Signal(signal.clone());
            let (line_opacity, card_opacity, compact_opacity) =
                annotation_opacities(detail, is_projection);

            elements.push(
                div()
                    .row()
                    .absolute()
                    .top(tick_top)
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
                    .top(connector_top)
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
                .hold_to_drag()
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
