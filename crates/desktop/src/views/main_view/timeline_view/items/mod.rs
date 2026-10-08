use chrono::{DateTime, Duration as ChronoDuration, Local};
use chronoutil::RelativeDuration;
use gpui::{App, FocusHandle, Focusable, Pixels, SharedString, px};
use std::time::Duration;
use subroutine_core::{AnyItem, SchedulePoint, Signal, StartPrecision};

pub(super) use super::super::item_inspection_matches_press;

use crate::views::format_item_meta;

mod annotations;
mod attached;
mod bins;
mod details;
mod drop;
mod edit;
mod force_create;
mod layout;
mod resize;

pub(super) use annotations::*;
pub(super) use bins::ExpandedBin;
pub(super) use details::DetailsAnchor;
pub(super) use drop::*;
pub(super) use force_create::*;
pub(super) use layout::*;
pub(super) use resize::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ItemNavigation {
    Previous,
    Next,
}

pub(super) const ITEM_KEY_CONTEXT: &str = "TimelineItem";

pub(super) type NavigationHandoffs =
    std::collections::HashMap<uuid::Uuid, Option<(uuid::Uuid, FocusHandle)>>;

fn timeline_selection_ids<'a>(
    items: impl IntoIterator<Item = &'a AnyItem>,
    signals: impl IntoIterator<Item = &'a Signal>,
) -> Vec<uuid::Uuid> {
    let mut entries: Vec<_> = items
        .into_iter()
        .filter_map(|item| item.start_datetime().map(|start| (start, item.id())))
        .chain(
            signals
                .into_iter()
                .map(|signal| (signal.datetime.with_timezone(&Local), signal.id)),
        )
        .collect();
    entries.sort_unstable();
    entries.into_iter().map(|(_, id)| id).collect()
}

pub(super) const FALLBACK_ITEM_DURATION: ChronoDuration = ChronoDuration::minutes(5);

pub(super) const COMPLETE_CHECKBOX_DURATION: Duration = Duration::from_millis(200);
pub(super) const ATTACHED_ITEM_LEFT: Pixels = px(16. * 4.);
pub(super) const MIN_ITEM_WIDTH: Pixels = px(48. * 4.);
pub(super) const MIN_ITEM_HEIGHT: Pixels = px(16. * 4.);
pub(super) const DURATIONLESS_ACTION_HEIGHT: Pixels = px(42.);

pub(super) const DATE_ITEM_TRACK_MAX_HEIGHT: Pixels = px(240.);
pub(super) const SLOT_GAP: Pixels = px(6.);
pub(super) const RESIZE_HANDLE_HEIGHT: Pixels = px(6.);

pub(super) const STICKY_OUTER_LABEL_INSET: Pixels = px(12.);
pub(super) const STICKY_OUTER_LABEL_HEIGHT: Pixels = px(40.);
pub(super) const STICKY_OUTER_LABEL_GAP: Pixels = px(8.);

pub(super) fn sticky_outer_label_clearance() -> Pixels {
    STICKY_OUTER_LABEL_INSET + STICKY_OUTER_LABEL_HEIGHT + STICKY_OUTER_LABEL_GAP
}

pub(super) const MIN_LANE_WIDTH: Pixels = px(28. * 4.);
pub(super) const MAX_LANES: usize = 4;

pub(super) const SPAN_FIT_FILL: f32 = 0.8;

pub(super) const ANNOTATION_INSET: Pixels = px(8.);
pub(super) const ANNOTATION_CHIP_GAP: Pixels = px(4.);
pub(super) const ANNOTATION_CHIP_HEIGHT: Pixels = px(26.);
pub(super) const ANNOTATION_RIGHT_PADDING: Pixels = STICKY_OUTER_LABEL_INSET;
pub(super) const MIN_ANNOTATION_CHIP_WIDTH: Pixels = px(96.);

pub(super) const ANNOTATION_GUTTER_FRACTION: f32 = 0.22;
pub(super) const MIN_ANNOTATION_GUTTER: Pixels = px(180.);
pub(super) const MAX_ANNOTATION_GUTTER: Pixels = px(320.);

pub(super) fn annotation_gutter_for(flexible: Pixels, toolbar_clearance: Pixels) -> Pixels {
    let available = (flexible - toolbar_clearance).max(px(0.));
    let min_gutter = MIN_ANNOTATION_CHIP_WIDTH
        + ANNOTATION_INSET
        + ANNOTATION_CHIP_GAP
        + ANNOTATION_RIGHT_PADDING;
    (available * ANNOTATION_GUTTER_FRACTION)
        .max(MIN_ANNOTATION_GUTTER)
        .min(MAX_ANNOTATION_GUTTER)
        .min((available - MIN_ITEM_WIDTH).max(min_gutter))
        .min(available)
        + toolbar_clearance
}

pub(super) fn measure_from(start: DateTime<Local>, duration: RelativeDuration) -> ChronoDuration {
    (start + duration) - start
}

pub(super) fn is_durationless_action(item: &AnyItem) -> bool {
    matches!(item, AnyItem::Action(action) if action.duration.is_none())
}

fn action_has_notes(item: &AnyItem) -> bool {
    matches!(item, AnyItem::Action(action) if action.content.as_deref().is_some_and(|notes| !notes.trim().is_empty()))
}

pub(super) fn item_title_only(item: &AnyItem, card_height: Pixels) -> bool {
    item.start_precision() == StartPrecision::Date
        || (is_durationless_action(item)
            && (card_height < MIN_ITEM_HEIGHT - SLOT_GAP || !action_has_notes(item)))
}

pub(super) fn item_min_height(item: &AnyItem) -> Pixels {
    if item.start_precision() == StartPrecision::Date || is_durationless_action(item) {
        DURATIONLESS_ACTION_HEIGHT
    } else {
        MIN_ITEM_HEIGHT
    }
}

pub(super) fn visual_duration_at(pixel_duration: ChronoDuration, height: Pixels) -> ChronoDuration {
    let nanoseconds = pixel_duration.num_nanoseconds().unwrap_or_default() as f64;
    ChronoDuration::nanoseconds((nanoseconds * f64::from(height.as_f32())).round() as i64)
}

pub(super) fn layout_duration_for_precision(
    precision: StartPrecision,
    duration: ChronoDuration,
    pixel_duration: ChronoDuration,
) -> ChronoDuration {
    if precision == StartPrecision::Date {
        duration.min(visual_duration_at(
            pixel_duration,
            DATE_ITEM_TRACK_MAX_HEIGHT,
        ))
    } else {
        duration
    }
}

pub(super) fn item_layout_duration(
    item: &AnyItem,
    duration: ChronoDuration,
    pixel_duration: ChronoDuration,
) -> ChronoDuration {
    layout_duration_for_precision(item.start_precision(), duration, pixel_duration)
}

pub(super) fn item_timeline_span(item: &AnyItem) -> Option<(DateTime<Local>, ChronoDuration)> {
    let start = item.start_datetime()?;
    let duration = match item.start_precision() {
        StartPrecision::Date => {
            let next_date = item.start_date()?.succ_opt()?;
            let end: DateTime<Local> = SchedulePoint::Date(next_date).into();
            end - start
        }
        StartPrecision::DateTime => item
            .duration()
            .map(|duration| measure_from(start, duration))
            .filter(|duration| *duration > ChronoDuration::zero())
            .unwrap_or(FALLBACK_ITEM_DURATION),
        StartPrecision::Unscheduled => return None,
    };
    Some((start, duration))
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum TransitionState {
    Attached,
    Completing,
}

#[derive(Clone)]
pub(super) struct TimelineItem {
    pub focus_handle: FocusHandle,
    pub item: AnyItem,
    pub transition_state: TransitionState,
    pub cached_title: SharedString,
    pub cached_meta: Option<SharedString>,
}

impl PartialEq for TimelineItem {
    fn eq(&self, other: &Self) -> bool {
        self.item.id() == other.item.id()
    }
}

impl Focusable for TimelineItem {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TimelineItem {
    pub(super) fn new(item: AnyItem, cx: &App) -> Self {
        let focus_handle = cx.focus_handle();
        let cached_title = SharedString::from(item.title().to_string());
        let cached_meta = format_item_meta(&item);
        Self {
            focus_handle,
            item,
            transition_state: TransitionState::Attached,
            cached_title,
            cached_meta,
        }
    }

    pub(super) fn refresh(&mut self, item: AnyItem) {
        self.cached_title = SharedString::from(item.title().to_string());
        self.cached_meta = format_item_meta(&item);
        self.item = item;
    }
}
