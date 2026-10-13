use std::{
    collections::{BTreeMap, BTreeSet, HashMap, hash_map::Entry},
    rc::Rc,
};

use chrono::{DateTime, Duration, Local, NaiveDate, NaiveTime, Utc};
use gpui::{Pixels, Size, px, size};
use subroutine_core::{AnyItem, Marker, Signal};
use uuid::Uuid;

use super::sections::UpcomingSection;

const FUTURE_HORIZON_DAYS: i64 = 366;

const MAX_DAYS: usize = 400;

const DAY_ROW_HEIGHT: Pixels = px(38.);

const ITEM_CARD_HEIGHT: Pixels = px(64.);
const TITLE_ONLY_ITEM_CARD_HEIGHT: Pixels = px(40.);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CreateKind {
    Slot,
    Append,
}

impl CreateKind {
    pub(super) fn height(self) -> Pixels {
        match self {
            Self::Slot => ITEM_CARD_HEIGHT,
            Self::Append => px(44.),
        }
    }
}

pub(super) fn row_top_inset(index: usize) -> Pixels {
    if index == 0 {
        crate::views::TOP_EDGE_INSET + super::headers::CONTROLS_HEIGHT
    } else {
        px(0.)
    }
}

#[derive(Clone)]
pub(super) struct AgendaMarker {
    pub marker: Marker,
    pub projected: bool,
}

#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
pub(super) enum UpcomingRow {
    Missed,
    Day {
        date: NaiveDate,
        markers: Vec<AgendaMarker>,
        relative: Option<&'static str>,
    },
    Item {
        item: AnyItem,
        projected: bool,
        section: UpcomingSection,
    },
    Create {
        date: NaiveDate,
        kind: CreateKind,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UpcomingRowKey {
    Heading(UpcomingSection),
    Item(Uuid),
    Create(UpcomingSection),
}

impl UpcomingRow {
    pub(super) fn key(&self) -> UpcomingRowKey {
        match self {
            Self::Item { item, .. } => UpcomingRowKey::Item(item.id()),
            Self::Create { .. } => UpcomingRowKey::Create(self.section()),
            _ => UpcomingRowKey::Heading(self.section()),
        }
    }

    pub(super) fn section(&self) -> UpcomingSection {
        match self {
            Self::Missed => UpcomingSection::Missed,
            Self::Day { date, .. } | Self::Create { date, .. } => UpcomingSection::Day(*date),
            Self::Item { section, .. } => *section,
        }
    }

    pub(super) fn is_heading(&self) -> bool {
        matches!(self, Self::Missed | Self::Day { .. })
    }

    pub(super) fn selectable_ids(&self) -> impl Iterator<Item = Uuid> + '_ {
        let item = match self {
            Self::Item {
                item,
                projected: false,
                ..
            } => Some(item.id()),
            _ => None,
        };
        let markers = match self {
            Self::Day { markers, .. } => markers.as_slice(),
            _ => &[],
        };
        item.into_iter().chain(
            markers
                .iter()
                .filter(|entry| !entry.projected)
                .map(|entry| entry.marker.id),
        )
    }

    pub(super) fn height(
        &self,
        card_height: impl Fn(&AnyItem) -> Pixels,
        day_height: impl Fn(NaiveDate) -> Pixels,
    ) -> Pixels {
        match self {
            UpcomingRow::Day { date, .. } => day_height(*date),
            UpcomingRow::Item { item, .. } => card_height(item),
            UpcomingRow::Create { kind, .. } => kind.height(),
            UpcomingRow::Missed => DAY_ROW_HEIGHT,
        }
    }
}

pub(super) fn item_card_height(item: &AnyItem) -> Pixels {
    if item.recurrence().is_some() || super::super::format_item_meta(item).is_some() {
        ITEM_CARD_HEIGHT
    } else {
        TITLE_ONLY_ITEM_CARD_HEIGHT
    }
}

#[derive(Default)]
pub(super) struct Agenda {
    pub rows: Vec<UpcomingRow>,
}

#[derive(Default)]
pub(super) struct AgendaLayout {
    pub rows: Vec<usize>,
    pub sizes: Rc<Vec<Size<Pixels>>>,
    pub order: Vec<Uuid>,
    pub positions: HashMap<Uuid, usize>,
}

impl Agenda {
    pub(super) fn build_revealing(
        items: &[AnyItem],
        drafts: &[AnyItem],
        markers: &[Marker],
        signals: &[Signal],
        today: NaiveDate,
        revealed_dates: &BTreeSet<NaiveDate>,
    ) -> Self {
        let (start, end) = Self::window(items, markers, signals, today);
        let visible_markers = project_markers(markers, start, end, revealed_dates);
        let visible_signals = project_signals(signals, start, end, revealed_dates);
        let projected_items = subroutine_core::projected_items_between(items, start, end);

        let mut by_day: BTreeMap<NaiveDate, Vec<(AnyItem, bool)>> = BTreeMap::new();
        let mut missed = Vec::new();
        for item in items {
            match UpcomingSection::for_item(item, today) {
                Some(UpcomingSection::Missed) => missed.push(item.clone()),
                Some(UpcomingSection::Day(date))
                    if (start..=end).contains(&date) || revealed_dates.contains(&date) =>
                {
                    by_day.entry(date).or_default().push((item.clone(), false));
                }
                Some(UpcomingSection::Day(_)) | None => {}
            }
        }
        for (signal, projected) in &visible_signals {
            if *projected {
                continue;
            }
            let date = signal.datetime.with_timezone(&Local).date_naive();
            by_day
                .entry(date)
                .or_default()
                .push((AnyItem::Signal(signal.clone()), false));
        }

        let mut concrete_days: BTreeSet<NaiveDate> = by_day.keys().copied().collect();
        for entry in &visible_markers {
            if entry.projected {
                continue;
            }
            let first = entry.marker.date.max(start);
            let last = entry.marker.end_date.unwrap_or(entry.marker.date).min(end);
            let mut day = first;
            while day <= last {
                concrete_days.insert(day);
                day += Duration::days(1);
            }
        }

        for item in projected_items {
            if let Some(date) = item.start_date()
                && concrete_days.contains(&date)
            {
                by_day.entry(date).or_default().push((item, true));
            }
        }
        for (signal, projected) in visible_signals {
            if !projected {
                continue;
            }
            let date = signal.datetime.with_timezone(&Local).date_naive();
            if concrete_days.contains(&date) {
                by_day
                    .entry(date)
                    .or_default()
                    .push((AnyItem::Signal(signal), true));
            }
        }
        // Provisional dates must not introduce extra recurrence projections around a draft.
        let mut draft_dates = BTreeSet::new();
        for draft in drafts {
            if let Some(date) = draft.start_date() {
                draft_dates.insert(date);
                by_day.entry(date).or_default().push((draft.clone(), false));
            }
        }
        for entries in by_day.values_mut() {
            entries.sort_by_key(|(item, _)| day_order(item));
        }

        let mut days = concrete_days.clone();
        days.retain(|date| (start..=end).contains(date));
        days.insert(today);
        let mut days: BTreeSet<NaiveDate> = days.into_iter().take(MAX_DAYS).collect();
        days.extend(revealed_dates.iter().copied().filter(|date| {
            concrete_days.contains(date)
                || visible_markers
                    .iter()
                    .any(|entry| !entry.projected && entry.marker.covers(*date))
        }));
        days.extend(&draft_dates);

        let has_actions_or_events = !missed.is_empty()
            || by_day
                .values()
                .flatten()
                .any(|(item, _)| matches!(item, AnyItem::Action(_) | AnyItem::Event(_)));
        let mut agenda = Agenda::default();
        if !missed.is_empty() {
            missed.sort_by_key(|item| (item.start_date(), day_order(item), item.id()));
            agenda.rows.push(UpcomingRow::Missed);
            agenda
                .rows
                .extend(missed.into_iter().map(|item| UpcomingRow::Item {
                    item,
                    projected: false,
                    section: UpcomingSection::Missed,
                }));
        }

        for date in days {
            agenda.rows.push(UpcomingRow::Day {
                date,
                markers: visible_markers
                    .iter()
                    .filter(|entry| {
                        entry.marker.covers(date)
                            && (!entry.projected || concrete_days.contains(&date))
                    })
                    .cloned()
                    .collect(),
                relative: relative_day(date, today),
            });

            let entries = by_day.remove(&date).unwrap_or_default();
            let create_kind = if date == today && !has_actions_or_events {
                CreateKind::Slot
            } else {
                CreateKind::Append
            };
            let needs_create_row = !entries.is_empty() || create_kind == CreateKind::Slot;
            agenda.rows.extend(
                entries
                    .into_iter()
                    .map(|(item, projected)| UpcomingRow::Item {
                        item,
                        projected,
                        section: UpcomingSection::Day(date),
                    }),
            );
            if needs_create_row && !draft_dates.contains(&date) {
                agenda.rows.push(UpcomingRow::Create {
                    date,
                    kind: create_kind,
                });
            }
        }

        agenda
    }

    pub(super) fn item_neighbors(&self, index: usize) -> (Option<Uuid>, Option<Uuid>) {
        let Some(UpcomingRow::Item { section, .. }) = self.rows.get(index) else {
            return (None, None);
        };
        let neighbor_id = |row: &UpcomingRow| match row {
            UpcomingRow::Item {
                item,
                section: neighbor,
                ..
            } if neighbor == section => Some(item.id()),
            _ => None,
        };
        // Reveal clipping and virtualization must not create new compound-card endpoints.
        (
            index
                .checked_sub(1)
                .and_then(|index| self.rows.get(index))
                .and_then(neighbor_id),
            self.rows.get(index + 1).and_then(neighbor_id),
        )
    }

    pub(super) fn layout(
        &self,
        section_state: impl Fn(UpcomingSection) -> (bool, f32),
        card_height: impl Fn(&AnyItem) -> Pixels,
        day_height: impl Fn(NaiveDate) -> Pixels,
    ) -> AgendaLayout {
        let mut layout = AgendaLayout::default();
        let mut sizes = Vec::new();
        let mut offset = 0;
        for group in self.rows.chunk_by(|a, b| a.section() == b.section()) {
            let (expanded, fraction) = section_state(group[0].section());
            let fraction = fraction.clamp(0., 1.);
            let mut remaining = group
                .iter()
                .filter(|row| !row.is_heading())
                .map(|row| row.height(&card_height, &day_height))
                .sum::<Pixels>()
                * fraction;
            for (index, row) in group.iter().enumerate() {
                let full_height = row.height(&card_height, &day_height);
                let height = if row.is_heading() || fraction == 1. {
                    full_height
                } else {
                    let height = full_height.min(remaining);
                    remaining -= height;
                    height
                };
                if height <= px(0.) {
                    continue;
                }
                if row.is_heading() || (expanded && height == full_height) {
                    for id in row.selectable_ids() {
                        if let Entry::Vacant(entry) = layout.positions.entry(id) {
                            entry.insert(layout.rows.len());
                            layout.order.push(id);
                        }
                    }
                }
                layout.rows.push(offset + index);
                sizes.push(size(px(0.), height + row_top_inset(offset + index)));
            }
            offset += group.len();
        }
        layout.sizes = Rc::new(sizes);
        layout
    }

    fn window(
        items: &[AnyItem],
        markers: &[Marker],
        signals: &[Signal],
        today: NaiveDate,
    ) -> (NaiveDate, NaiveDate) {
        let ceiling = today + Duration::days(FUTURE_HORIZON_DAYS);

        let dates = items
            .iter()
            .filter_map(|item| item.start_date())
            .chain(
                signals
                    .iter()
                    .map(|signal| signal.datetime.with_timezone(&Local).date_naive()),
            )
            .chain(
                markers
                    .iter()
                    .flat_map(|marker| [marker.date, marker.end_date.unwrap_or(marker.date)]),
            );

        let mut end = today;
        for date in dates {
            if date <= ceiling {
                end = end.max(date);
            }
        }
        (today, end)
    }
}

fn day_order(item: &AnyItem) -> (bool, Option<DateTime<Utc>>) {
    let start = item.start();
    (
        start.is_none_or(|start| start.time().is_none()),
        start.map(DateTime::<Utc>::from),
    )
}

fn relative_day(date: NaiveDate, today: NaiveDate) -> Option<&'static str> {
    match (date - today).num_days() {
        0 => Some("Today"),
        1 => Some("Tomorrow"),
        _ => None,
    }
}

fn day_start(date: NaiveDate) -> DateTime<Utc> {
    date.and_time(NaiveTime::MIN)
        .and_local_timezone(Local)
        .earliest()
        .map(|start| start.with_timezone(&Utc))
        .unwrap_or_else(|| date.and_time(NaiveTime::MIN).and_utc())
}

fn project_markers(
    markers: &[Marker],
    start: NaiveDate,
    end: NaiveDate,
    revealed_dates: &BTreeSet<NaiveDate>,
) -> Vec<AgendaMarker> {
    let mut projected: Vec<Marker> = markers
        .iter()
        .flat_map(|marker| marker.projections_between(start, end))
        .filter(|ghost| {
            !markers
                .iter()
                .any(|stored| stored.lineage_id == ghost.lineage_id && stored.date == ghost.date)
        })
        .collect();
    projected.sort_by_key(|marker| (marker.date, marker.lineage_id));
    projected.dedup_by_key(|marker| (marker.date, marker.lineage_id));

    let mut visible: Vec<AgendaMarker> = markers
        .iter()
        .filter(|marker| {
            (marker.date <= end && marker.end_date.unwrap_or(marker.date) >= start)
                || revealed_dates.iter().any(|date| marker.covers(*date))
        })
        .cloned()
        .map(|marker| AgendaMarker {
            marker,
            projected: false,
        })
        .chain(projected.into_iter().map(|marker| AgendaMarker {
            marker,
            projected: true,
        }))
        .collect();
    visible.sort_by_key(|entry| (entry.marker.date, entry.marker.lineage_id));
    visible
}

fn project_signals(
    signals: &[Signal],
    start: NaiveDate,
    end: NaiveDate,
    revealed_dates: &BTreeSet<NaiveDate>,
) -> Vec<(Signal, bool)> {
    let from = day_start(start);
    let to = day_start(end + Duration::days(1)) - Duration::seconds(1);

    let mut visible: Vec<(Signal, bool)> = signals
        .iter()
        .filter(|signal| {
            let date = signal.datetime.with_timezone(&Local).date_naive();
            (start..=end).contains(&date) || revealed_dates.contains(&date)
        })
        .cloned()
        .map(|signal| (signal, false))
        .collect();
    for signal in signals {
        visible.extend(
            signal
                .projections_between(from, to)
                .into_iter()
                .filter(|ghost| {
                    !signals.iter().any(|stored| {
                        stored.lineage_id == ghost.lineage_id && stored.datetime == ghost.datetime
                    })
                })
                .map(|ghost| (ghost, true)),
        );
    }
    visible.sort_by_key(|(signal, _)| (signal.datetime, signal.lineage_id));
    visible.dedup_by_key(|(signal, _)| (signal.datetime, signal.lineage_id));
    visible
}

#[cfg(test)]
mod tests {
    use super::*;
    use subroutine_core::{Action, Event, SchedulePoint};

    #[test]
    fn empty_agenda_has_only_today_and_one_nonselectable_create_card() {
        let today = NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();
        let agenda = Agenda::build_revealing(&[], &[], &[], &[], today, &BTreeSet::new());
        let mut sections = super::super::sections::UpcomingSections::default();
        sections.sync(&agenda, today);
        assert!(!sections.add_in_header(&agenda, today));
        let dates: Vec<_> = agenda
            .rows
            .iter()
            .filter_map(|row| match row {
                UpcomingRow::Day { date, .. } => Some(*date),
                _ => None,
            })
            .collect();
        assert_eq!(dates, [today]);
        let layout = agenda.layout(
            |section| sections.layout_state(section),
            |_| panic!("empty agendas have no item cards"),
            |_| DAY_ROW_HEIGHT,
        );
        assert_eq!(layout.rows, [0, 1]);
        assert!(layout.order.is_empty());
        assert!(layout.positions.is_empty());
        for row in &agenda.rows {
            assert!(row.selectable_ids().next().is_none());
        }
        assert_eq!(
            agenda
                .rows
                .iter()
                .filter(|row| matches!(
                    row,
                    UpcomingRow::Create {
                        kind: CreateKind::Slot,
                        ..
                    }
                ))
                .count(),
            1
        );
    }

    #[test]
    fn actions_and_events_suppress_slots_and_preserve_content_dates() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 12).unwrap();
        let tomorrow = today.succ_opt().unwrap();
        let next = tomorrow.succ_opt().unwrap();
        let action = |date| {
            AnyItem::Action(
                Action::new("Scheduled")
                    .with_queued(true)
                    .with_start(Some(SchedulePoint::Date(date))),
            )
        };
        for (items, markers, signals, expected_dates) in [
            (
                vec![action(tomorrow)],
                vec![],
                vec![],
                vec![today, tomorrow],
            ),
            (
                vec![action(today.pred_opt().unwrap())],
                vec![],
                vec![],
                vec![today],
            ),
            (
                vec![AnyItem::Event(Event::new(
                    "Later",
                    day_start(next),
                    Duration::hours(1),
                ))],
                vec![Marker::new("Today", today)],
                vec![],
                vec![today, next],
            ),
        ] {
            let agenda =
                Agenda::build_revealing(&items, &[], &markers, &signals, today, &BTreeSet::new());
            let dates: Vec<_> = agenda
                .rows
                .iter()
                .filter_map(|row| match row {
                    UpcomingRow::Day { date, .. } => Some(*date),
                    _ => None,
                })
                .collect();
            assert_eq!(dates, expected_dates);
            let controls: Vec<_> = agenda
                .rows
                .iter()
                .filter_map(|row| match row {
                    UpcomingRow::Create { kind, .. } => Some(*kind),
                    _ => None,
                })
                .collect();
            assert!(controls.iter().all(|kind| *kind == CreateKind::Append));
        }
    }

    #[test]
    fn markers_keep_today_slot_without_expanding_other_dates() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 12).unwrap();
        let future = today + Duration::days(7);
        let markers = [
            Marker::new("Holiday", today),
            Marker::new("Another holiday", future),
        ];
        let agenda = Agenda::build_revealing(&[], &[], &markers, &[], today, &BTreeSet::new());
        let mut sections = super::super::sections::UpcomingSections::default();
        sections.sync(&agenda, today);
        assert_eq!(agenda.rows.len(), 3);
        assert!(
            matches!(agenda.rows[1], UpcomingRow::Create { date, kind: CreateKind::Slot } if date == today)
        );
        assert!(!sections.add_in_header(&agenda, today));
        assert!(sections.add_in_header(&agenda, future));
        let layout = agenda.layout(
            |section| sections.layout_state(section),
            |_| panic!("marker-only dates have no item cards"),
            |_| DAY_ROW_HEIGHT,
        );
        assert_eq!(layout.sizes[0].height, DAY_ROW_HEIGHT + row_top_inset(0));
        assert_eq!(layout.sizes[1].height, CreateKind::Slot.height());
        assert_eq!(layout.sizes[2].height, DAY_ROW_HEIGHT);

        let signals = [Signal::new("Later", day_start(future))];
        let annotated =
            Agenda::build_revealing(&[], &[], &markers, &signals, today, &BTreeSet::new());
        let slots: Vec<_> = annotated
            .rows
            .iter()
            .filter_map(|row| match row {
                UpcomingRow::Create {
                    date,
                    kind: CreateKind::Slot,
                } => Some(*date),
                _ => None,
            })
            .collect();
        assert_eq!(slots, [today]);
    }

    #[test]
    fn revealed_dates_only_survive_when_they_have_content() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 12).unwrap();
        let past = today - Duration::days(3);
        let future = today + Duration::days(FUTURE_HORIZON_DAYS + 1);
        let requested = BTreeSet::from([past, today.succ_opt().unwrap(), future]);
        let dates = |agenda: &Agenda| {
            agenda
                .rows
                .iter()
                .filter_map(|row| match row {
                    UpcomingRow::Day { date, .. } => Some(*date),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let empty = Agenda::build_revealing(&[], &[], &[], &[], today, &requested);
        assert_eq!(dates(&empty), [today]);

        let item = AnyItem::Action(
            Action::new("Far ahead")
                .with_queued(true)
                .with_start(Some(SchedulePoint::Date(future))),
        );
        let agenda = Agenda::build_revealing(
            &[item],
            &[],
            &[Marker::new("Earlier", past)],
            &[],
            today,
            &requested,
        );
        assert_eq!(dates(&agenda), [past, today, future]);
    }

    #[test]
    fn inline_draft_replaces_empty_agenda_slot_without_moving() {
        let today = NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();
        let requested = BTreeSet::from([today.succ_opt().unwrap()]);
        let before = Agenda::build_revealing(&[], &[], &[], &[], today, &requested);
        let mut sections = super::super::sections::UpcomingSections::default();
        sections.sync(&before, today);
        let before_layout = before.layout(
            |section| sections.layout_state(section),
            item_card_height,
            |_| DAY_ROW_HEIGHT,
        );
        let draft = AnyItem::Action(
            Action::new("")
                .with_queued(true)
                .with_start(Some(SchedulePoint::Date(today))),
        );
        sections.reveal(UpcomingSection::Day(today));
        let after = Agenda::build_revealing(
            &[],
            std::slice::from_ref(&draft),
            &[],
            &[],
            today,
            &requested,
        );
        sections.sync(&after, today);
        let after_layout = after.layout(
            |section| sections.layout_state(section),
            item_card_height,
            |_| DAY_ROW_HEIGHT,
        );
        assert_eq!(before_layout.rows, [0, 1]);
        assert_eq!(before_layout.rows, after_layout.rows);
        assert_eq!(before_layout.sizes, after_layout.sizes);
        assert_eq!(after_layout.positions[&draft.id()], 1);
        assert!(!sections.is_animating());
        assert!(
            after
                .rows
                .iter()
                .all(|row| !matches!(row, UpcomingRow::Create { .. }))
        );
    }

    #[test]
    fn item_neighbors_include_projections_and_stop_at_boundaries() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 9).unwrap();
        let day = UpcomingSection::Day(today);
        let item = |id, section, projected| UpcomingRow::Item {
            item: AnyItem::Action(Action {
                id: Uuid::from_u128(id),
                ..Action::new("Item")
            }),
            projected,
            section,
        };
        let agenda = Agenda {
            rows: vec![
                UpcomingRow::Missed,
                item(1, UpcomingSection::Missed, false),
                item(2, UpcomingSection::Missed, true),
                item(3, UpcomingSection::Missed, false),
                UpcomingRow::Day {
                    date: today,
                    markers: Vec::new(),
                    relative: None,
                },
                item(5, day, false),
                UpcomingRow::Create {
                    date: today,
                    kind: CreateKind::Append,
                },
                item(7, day, true),
                item(8, UpcomingSection::Day(today.succ_opt().unwrap()), false),
            ],
        };
        for (index, (previous, next)) in [
            (None, None),
            (None, Some(2)),
            (Some(1), Some(3)),
            (Some(2), None),
            (None, None),
            (None, None),
            (None, None),
            (None, None),
            (None, None),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(
                agenda.item_neighbors(index),
                (previous.map(Uuid::from_u128), next.map(Uuid::from_u128)),
                "row {index}",
            );
        }
        assert_eq!(agenda.item_neighbors(agenda.rows.len()), (None, None));
        assert_eq!(agenda.item_neighbors(usize::MAX), (None, None));
    }

    #[test]
    fn layout_selects_visible_concrete_items_and_unique_heading_markers() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 9).unwrap();
        let tomorrow = today + Duration::days(1);
        let mut spanning = Marker::new("Multi-day", today);
        spanning.end_date = Some(tomorrow);
        let marker = Marker::new("Tomorrow", tomorrow);
        let projected_marker = Marker::new("Projected", today);
        let first =
            AnyItem::Action(Action::new("First").with_start(Some(SchedulePoint::Date(today))));
        let second =
            AnyItem::Action(Action::new("Second").with_start(Some(SchedulePoint::Date(tomorrow))));
        let projected_item =
            AnyItem::Action(Action::new("Projected").with_start(Some(SchedulePoint::Date(today))));
        let projected_id = projected_item.id();
        let agenda = Agenda {
            rows: vec![
                UpcomingRow::Day {
                    date: today,
                    markers: vec![
                        AgendaMarker {
                            marker: spanning.clone(),
                            projected: false,
                        },
                        AgendaMarker {
                            marker: projected_marker,
                            projected: true,
                        },
                    ],
                    relative: None,
                },
                UpcomingRow::Item {
                    item: first.clone(),
                    projected: false,
                    section: UpcomingSection::Day(today),
                },
                UpcomingRow::Item {
                    item: projected_item,
                    projected: true,
                    section: UpcomingSection::Day(today),
                },
                UpcomingRow::Day {
                    date: tomorrow,
                    markers: vec![
                        AgendaMarker {
                            marker: spanning.clone(),
                            projected: false,
                        },
                        AgendaMarker {
                            marker: marker.clone(),
                            projected: false,
                        },
                    ],
                    relative: None,
                },
                UpcomingRow::Item {
                    item: second.clone(),
                    projected: false,
                    section: UpcomingSection::Day(tomorrow),
                },
            ],
        };
        let day_height = |date| if date == today { px(130.) } else { px(84.) };

        for (expanded, fraction, rows, heights, selected) in [
            (
                false,
                0.,
                vec![0, 3],
                vec![],
                vec![(spanning.id, 0), (marker.id, 1)],
            ),
            (
                false,
                1.,
                vec![0, 1, 2, 3, 4],
                vec![40., 40., 40.],
                vec![(spanning.id, 0), (marker.id, 3)],
            ),
            (
                true,
                0.,
                vec![0, 3],
                vec![],
                vec![(spanning.id, 0), (marker.id, 1)],
            ),
            (
                true,
                0.25,
                vec![0, 1, 3, 4],
                vec![20., 10.],
                vec![(spanning.id, 0), (marker.id, 2)],
            ),
            (
                true,
                0.5,
                vec![0, 1, 3, 4],
                vec![40., 20.],
                vec![(spanning.id, 0), (first.id(), 1), (marker.id, 2)],
            ),
            (
                true,
                1.,
                vec![0, 1, 2, 3, 4],
                vec![40., 40., 40.],
                vec![
                    (spanning.id, 0),
                    (first.id(), 1),
                    (marker.id, 3),
                    (second.id(), 4),
                ],
            ),
        ] {
            let layout = agenda.layout(|_| (expanded, fraction), |_| px(40.), day_height);
            assert_eq!(layout.rows, rows);
            assert_eq!(agenda.item_neighbors(1), (None, Some(projected_id)));
            assert_eq!(agenda.item_neighbors(2), (Some(first.id()), None));
            assert_eq!(agenda.item_neighbors(4), (None, None));
            let item_heights: Vec<_> = layout
                .rows
                .iter()
                .zip(layout.sizes.iter())
                .filter_map(|(source, size)| {
                    matches!(agenda.rows[*source], UpcomingRow::Item { .. }).then_some(size.height)
                })
                .collect();
            assert_eq!(
                item_heights,
                heights.into_iter().map(px).collect::<Vec<_>>()
            );
            assert_eq!(
                layout.order,
                selected.iter().map(|(id, _)| *id).collect::<Vec<_>>()
            );
            assert_eq!(layout.positions, selected.into_iter().collect());
            for (index, row) in layout.rows.iter().enumerate() {
                if let UpcomingRow::Day { date, .. } = &agenda.rows[*row] {
                    assert_eq!(
                        layout.sizes[index].height,
                        day_height(*date) + row_top_inset(*row)
                    );
                }
            }
        }
    }
}
