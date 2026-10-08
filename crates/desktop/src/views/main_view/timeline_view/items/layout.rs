use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;

use uuid::Uuid;

use chrono::{DateTime, Duration as ChronoDuration, Local};
use gpui::{App, Pixels};

use super::super::TimelineView;
use super::{ActiveResizeState, MAX_LANES, MIN_ITEM_HEIGHT, MIN_LANE_WIDTH, SLOT_GAP};
use super::{
    TimelineItem, TransitionState, item_layout_duration, item_min_height, item_timeline_span,
    visual_duration_at,
};

const BIN_GROUPING_PROXIMITY: Pixels = gpui::px(24.);
const TARGET_BIN_MEMBERS: usize = 4;
type BinMergeScore = (bool, ChronoDuration, usize, usize, usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Lane {
    pub index: usize,
    pub count: usize,
}

impl Lane {
    pub(crate) const FULL: Lane = Lane { index: 0, count: 1 };
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TimelineBin {
    pub key: u64,
    pub lane: Lane,
    pub start: DateTime<Local>,
    pub end: DateTime<Local>,
    pub members: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TimelineSlot {
    Item { index: usize, lane: Lane },
    Bin(TimelineBin),
    Drop { lane: Option<Lane> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpanOf {
    Item(usize),
    Drop,
}

impl SpanOf {
    fn order(self) -> usize {
        match self {
            SpanOf::Item(index) => index,
            SpanOf::Drop => usize::MAX,
        }
    }
}

struct Span {
    of: SpanOf,
    start: DateTime<Local>,
    end: DateTime<Local>,
    visual_end: DateTime<Local>,
    held: bool,
}

fn span_during_resize(
    item_id: Uuid,
    start: DateTime<Local>,
    duration: ChronoDuration,
    resize: Option<&ActiveResizeState>,
) -> (DateTime<Local>, ChronoDuration) {
    resize
        .filter(|resize| resize.item_id == item_id)
        .map(|resize| (resize.new_time, resize.new_end - resize.new_time))
        .unwrap_or((start, duration))
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Packing {
    Lanes {
        lanes: usize,
        assignment: Vec<usize>,
    },
    Bin,
    BinBeside {
        lanes: usize,
        held: Vec<(usize, usize)>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MosaicGroup {
    positions: Vec<usize>,
    start: DateTime<Local>,
    end: DateTime<Local>,
    visual_end: DateTime<Local>,
    held: bool,
}

impl MosaicGroup {
    fn single(position: usize, span: &Span) -> Self {
        Self {
            positions: vec![position],
            start: span.start,
            end: span.end,
            visual_end: span.visual_end,
            held: span.held,
        }
    }

    fn member_count(&self) -> usize {
        self.positions.len()
    }

    fn can_bin(&self, spans: &[Span]) -> bool {
        !self.held
            && self
                .positions
                .iter()
                .all(|position| matches!(spans[*position].of, SpanOf::Item(_)))
    }

    fn order(&self, spans: &[Span]) -> usize {
        self.positions
            .iter()
            .map(|position| spans[*position].of.order())
            .min()
            .unwrap_or(usize::MAX)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MosaicPacking {
    groups: Vec<MosaicGroup>,
    lanes: usize,
    assignment: Vec<usize>,
}

fn min_visual_span_at(pixel_duration: ChronoDuration, min_height: Pixels) -> ChronoDuration {
    visual_duration_at(pixel_duration, min_height + SLOT_GAP)
}

fn pack_mosaic(
    spans: &[Span],
    max_lanes: usize,
    bin_visual_span: ChronoDuration,
    max_bin_proximity: ChronoDuration,
) -> MosaicPacking {
    let mut groups = spans
        .iter()
        .enumerate()
        .map(|(position, span)| MosaicGroup::single(position, span))
        .collect::<Vec<_>>();

    loop {
        let (lanes, assignment) = assign_mosaic_lanes(&groups);
        if lanes <= max_lanes {
            return MosaicPacking {
                groups,
                lanes,
                assignment,
            };
        }

        let overflow = first_overflow(&groups, max_lanes)
            .expect("a layout using too many lanes must contain an overflowing moment");
        let mut best: Option<(BinMergeScore, usize, usize)> = None;
        for (left_offset, left) in overflow.iter().copied().enumerate() {
            if !groups[left].can_bin(spans) {
                continue;
            }
            for right in overflow.iter().copied().skip(left_offset + 1) {
                if !groups[right].can_bin(spans) {
                    continue;
                }

                let distance = groups[right].start - groups[left].start;
                if distance > max_bin_proximity {
                    continue;
                }
                let members = groups[left].member_count() + groups[right].member_count();
                let score = (
                    members > TARGET_BIN_MEMBERS,
                    distance,
                    members,
                    groups[left].order(spans),
                    groups[right].order(spans),
                );
                if best
                    .as_ref()
                    .is_none_or(|(best_score, _, _)| score < *best_score)
                {
                    best = Some((score, left, right));
                }
            }
        }

        let Some((_, left, right)) = best else {
            return MosaicPacking {
                groups,
                lanes,
                assignment,
            };
        };

        let right = groups.remove(right);
        let left_group = groups.remove(left);
        let start = left_group.start.min(right.start);
        let mut positions = left_group.positions;
        positions.extend(right.positions);
        positions.sort_by_key(|position| spans[*position].of.order());
        groups.push(MosaicGroup {
            positions,
            start,
            end: left_group.end.max(right.end),
            visual_end: start + bin_visual_span,
            held: false,
        });
        groups.sort_by_key(|group| (group.start, group.order(spans)));
    }
}

fn first_overflow(groups: &[MosaicGroup], max_lanes: usize) -> Option<Vec<usize>> {
    for (current, group) in groups.iter().enumerate() {
        let active = groups[..=current]
            .iter()
            .enumerate()
            .filter(|(_, candidate)| candidate.visual_end > group.start)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if active.len() > max_lanes {
            return Some(active);
        }
    }
    None
}

fn assign_mosaic_lanes(groups: &[MosaicGroup]) -> (usize, Vec<usize>) {
    let mut lane_free_from: Vec<DateTime<Local>> = Vec::new();
    let mut assignment = Vec::with_capacity(groups.len());
    for group in groups {
        match lane_free_from
            .iter()
            .position(|free_from| *free_from <= group.start)
        {
            Some(lane) => {
                lane_free_from[lane] = group.visual_end;
                assignment.push(lane);
            }
            None => {
                lane_free_from.push(group.visual_end);
                assignment.push(lane_free_from.len() - 1);
            }
        }
    }
    (lane_free_from.len().max(1), assignment)
}

impl TimelineView {
    pub(super) fn min_visual_span(&self) -> ChronoDuration {
        min_visual_span_at(self.pixel_duration, MIN_ITEM_HEIGHT)
    }

    pub(super) fn max_lanes(&self) -> usize {
        let fits = (self.item_area_width() / MIN_LANE_WIDTH).floor() as usize;
        fits.clamp(1, MAX_LANES)
    }

    pub(super) fn layout_slots(&self, cx: &App) -> Vec<TimelineSlot> {
        let carried: &[Uuid] = self
            .active_drop
            .as_ref()
            .map(|drop| drop.dragged.as_slice())
            .unwrap_or_default();

        let mut spans: Vec<Span> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                matches!(
                    item.transition_state,
                    TransitionState::Attached | TransitionState::Completing
                )
            })
            .filter(|(_, item)| !carried.contains(&item.item.id()))
            .filter_map(|(index, item)| {
                let (start, duration) = item_timeline_span(&item.item)?;
                let (start, duration) = span_during_resize(
                    item.item.id(),
                    start,
                    duration,
                    self.active_resize.as_ref(),
                );
                let min_height = self
                    .active_resize
                    .as_ref()
                    .filter(|resize| resize.item_id == item.item.id())
                    .map(|resize| resize.min_height())
                    .unwrap_or_else(|| item_min_height(&item.item));
                let min_span = min_visual_span_at(self.pixel_duration, min_height);
                let layout_duration =
                    item_layout_duration(&item.item, duration, self.pixel_duration);
                Some(Span {
                    of: SpanOf::Item(index),
                    start,
                    end: start + duration,
                    visual_end: start + layout_duration.max(min_span),
                    held: self.is_held(item, cx),
                })
            })
            .collect();

        let mut loose_drop = false;
        if let Some(drop) = self.active_drop.as_ref() {
            match drop.span() {
                Some((start, duration)) => spans.push(Span {
                    of: SpanOf::Drop,
                    start,
                    end: start + duration,
                    visual_end: start
                        + duration.max(min_visual_span_at(
                            self.pixel_duration,
                            self.drop_min_height(drop),
                        )),
                    held: true,
                }),
                None => loose_drop = true,
            }
        }

        spans.sort_by_key(|span| (span.start, span.of.order()));

        let max_lanes = self.max_lanes();
        let mut slots = Vec::with_capacity(spans.len() + 1);
        for (range, packing) in pack(&spans, max_lanes) {
            let cluster = &spans[range];
            match packing {
                Packing::Lanes { lanes, assignment } => {
                    slots.extend(cluster.iter().zip(assignment).map(|(span, index)| {
                        slot_for(
                            span,
                            Lane {
                                index,
                                count: lanes,
                            },
                        )
                    }));
                }
                Packing::Bin | Packing::BinBeside { .. } => {
                    let mosaic = pack_mosaic(
                        cluster,
                        max_lanes,
                        min_visual_span_at(self.pixel_duration, MIN_ITEM_HEIGHT),
                        visual_duration_at(self.pixel_duration, BIN_GROUPING_PROXIMITY),
                    );
                    for (group, index) in mosaic.groups.iter().zip(mosaic.assignment) {
                        let lane = Lane {
                            index,
                            count: mosaic.lanes,
                        };
                        if let [position] = group.positions.as_slice() {
                            slots.push(slot_for(&cluster[*position], lane));
                        } else {
                            let members = group
                                .positions
                                .iter()
                                .map(|position| &cluster[*position])
                                .collect::<Vec<_>>();
                            slots.extend(self.bin(&members, lane).map(TimelineSlot::Bin));
                        }
                    }
                }
            }
        }

        if loose_drop {
            slots.push(TimelineSlot::Drop { lane: None });
        }
        slots
    }

    fn is_held(&self, item: &TimelineItem, cx: &App) -> bool {
        let id = item.item.id();
        if self.expanded_bin_contains(id) {
            return false;
        }
        self.pending_item_focus == Some(id)
            || self.is_being_edited(id, cx)
            || self
                .active_resize
                .as_ref()
                .is_some_and(|resize| resize.item_id == id)
    }

    fn bin(&self, members: &[&Span], lane: Lane) -> Option<TimelineBin> {
        let mut hasher = DefaultHasher::new();
        let mut indices = Vec::with_capacity(members.len());
        for span in members {
            if let SpanOf::Item(index) = span.of {
                indices.push(index);
                if let Some(item) = self.items.get(index) {
                    item.item.id_u64().hash(&mut hasher);
                }
            }
        }
        if indices.is_empty() {
            return None;
        }
        Some(TimelineBin {
            key: hasher.finish(),
            lane,
            start: members.iter().map(|span| span.start).min()?,
            end: members.iter().map(|span| span.end).max()?,
            members: indices,
        })
    }
}

fn slot_for(span: &Span, lane: Lane) -> TimelineSlot {
    match span.of {
        SpanOf::Item(index) => TimelineSlot::Item { index, lane },
        SpanOf::Drop => TimelineSlot::Drop { lane: Some(lane) },
    }
}

fn pack(spans: &[Span], max_lanes: usize) -> Vec<(Range<usize>, Packing)> {
    let mut packings = Vec::new();
    let mut cursor = 0;
    while cursor < spans.len() {
        let end = next_cluster(spans, cursor);
        let cluster = &spans[cursor..end];
        let (lanes, assignment) = assign_lanes(cluster);

        let packing = if lanes <= max_lanes {
            Packing::Lanes { lanes, assignment }
        } else {
            let held: Vec<usize> = cluster
                .iter()
                .enumerate()
                .filter(|(_, span)| span.held)
                .map(|(position, _)| position)
                .collect();
            if held.is_empty() {
                Packing::Bin
            } else if held.len() == cluster.len() {
                Packing::Lanes { lanes, assignment }
            } else {
                let held_spans: Vec<&Span> =
                    held.iter().map(|position| &cluster[*position]).collect();
                let (held_lanes, held_assignment) = assign_lanes(held_spans.iter().copied());
                Packing::BinBeside {
                    lanes: held_lanes + 1,
                    held: held
                        .into_iter()
                        .zip(held_assignment.into_iter().map(|lane| lane + 1))
                        .collect(),
                }
            }
        };
        packings.push((cursor..end, packing));
        cursor = end;
    }
    packings
}

fn next_cluster(spans: &[Span], from: usize) -> usize {
    let mut reach = spans[from].visual_end;
    let mut end = from + 1;
    while end < spans.len() && spans[end].start < reach {
        reach = reach.max(spans[end].visual_end);
        end += 1;
    }
    end
}

fn assign_lanes<'a>(spans: impl IntoIterator<Item = &'a Span>) -> (usize, Vec<usize>) {
    let mut lane_free_from: Vec<DateTime<Local>> = Vec::new();
    let mut assignment = Vec::new();
    for span in spans {
        match lane_free_from
            .iter()
            .position(|free_from| *free_from <= span.start)
        {
            Some(lane) => {
                lane_free_from[lane] = span.visual_end;
                assignment.push(lane);
            }
            None => {
                lane_free_from.push(span.visual_end);
                assignment.push(lane_free_from.len() - 1);
            }
        }
    }
    (lane_free_from.len().max(1), assignment)
}
