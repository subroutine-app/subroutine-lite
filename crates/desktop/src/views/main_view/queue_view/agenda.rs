use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    rc::Rc,
};

use chrono::{DateTime, Duration, Local, NaiveDate, NaiveTime, Utc};
use gpui::{Pixels, Size, px, size};
use subroutine_core::{AnyItem, Marker, Signal};
use uuid::Uuid;

use super::sections::QueueSection;

const FUTURE_HORIZON_DAYS: i64 = 366;

const MAX_DAYS: usize = 400;

const DAY_ROW_HEIGHT: Pixels = px(38.);
pub(super) const MARKER_ROW_HEIGHT: Pixels = px(20.);
const ITEM_CARD_HEIGHT: Pixels = px(64.);
const TITLE_ONLY_ITEM_CARD_HEIGHT: Pixels = px(40.);
pub(super) const ITEM_ROW_PADDING: Pixels = px(3.);
const CREATE_ROW_HEIGHT: Pixels = px(44.);

pub(super) fn row_top_inset(index: usize) -> Pixels {
    if index == 0 {
        crate::views::TOP_EDGE_INSET
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
pub(super) enum QueueRow {
    Missed,
    Day {
        date: NaiveDate,
        markers: Vec<AgendaMarker>,
        relative: Option<&'static str>,
    },
    Item {
        item: AnyItem,
        projected: bool,
        section: QueueSection,
    },
    Create {
        date: Option<NaiveDate>,
    },

    AnyTime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum QueueRowKey {
    Heading(QueueSection),
    Item(Uuid),
    Create(QueueSection),
}

impl QueueRow {
    pub(super) fn key(&self) -> QueueRowKey {
        match self {
            Self::Item { item, .. } => QueueRowKey::Item(item.id()),
            Self::Create { .. } => QueueRowKey::Create(self.section()),
            _ => QueueRowKey::Heading(self.section()),
        }
    }

    pub(super) fn section(&self) -> QueueSection {
        match self {
            Self::Missed => QueueSection::Missed,
            Self::Day { date, .. } => QueueSection::Day(*date),
            Self::Item { section, .. } => *section,
            Self::Create { date } => (*date).into(),
            Self::AnyTime => QueueSection::Unscheduled,
        }
    }

    pub(super) fn is_heading(&self) -> bool {
        matches!(self, Self::Missed | Self::Day { .. } | Self::AnyTime)
    }

    pub(super) fn height(&self, card_height: impl Fn(&AnyItem) -> Pixels) -> Pixels {
        match self {
            QueueRow::Day { markers, .. } if markers.is_empty() => DAY_ROW_HEIGHT,
            QueueRow::Day { .. } => DAY_ROW_HEIGHT + MARKER_ROW_HEIGHT,
            QueueRow::Item { item, .. } => card_height(item) + ITEM_ROW_PADDING * 2.,
            QueueRow::Create { .. } => CREATE_ROW_HEIGHT,
            QueueRow::Missed | QueueRow::AnyTime => DAY_ROW_HEIGHT,
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
    pub rows: Vec<QueueRow>,
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
        let mut any_time: Vec<AnyItem> = Vec::new();
        let mut missed = Vec::new();
        for item in items {
            if QueueSection::for_item(item, today) == QueueSection::Missed {
                missed.push(item.clone());
                continue;
            }
            match item.start_date() {
                Some(date) if (start..=end).contains(&date) || revealed_dates.contains(&date) => {
                    by_day.entry(date).or_default().push((item.clone(), false));
                }
                Some(_) => {}
                None => any_time.push(item.clone()),
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
        for entries in by_day.values_mut() {
            entries.sort_by_key(|(item, _)| day_order(item));
        }

        let mut days = concrete_days.clone();
        days.retain(|date| (start..=end).contains(date));
        days.insert(today);
        let mut days: BTreeSet<NaiveDate> = days.into_iter().take(MAX_DAYS).collect();
        days.extend(revealed_dates);

        let mut agenda = Agenda::default();
        if !missed.is_empty() {
            missed.sort_by_key(|item| (item.start_date(), day_order(item), item.id()));
            agenda.rows.push(QueueRow::Missed);
            agenda
                .rows
                .extend(missed.into_iter().map(|item| QueueRow::Item {
                    item,
                    projected: false,
                    section: QueueSection::Missed,
                }));
        }

        agenda.rows.push(QueueRow::AnyTime);
        agenda
            .rows
            .extend(any_time.into_iter().map(|item| QueueRow::Item {
                item,
                projected: false,
                section: QueueSection::Unscheduled,
            }));
        agenda.rows.push(QueueRow::Create { date: None });

        for date in days {
            agenda.rows.push(QueueRow::Day {
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
            agenda
                .rows
                .extend(entries.into_iter().map(|(item, projected)| QueueRow::Item {
                    item,
                    projected,
                    section: QueueSection::Day(date),
                }));
            agenda.rows.push(QueueRow::Create { date: Some(date) });
        }

        agenda
    }

    pub(super) fn layout(
        &self,
        section_state: impl Fn(QueueSection) -> (bool, f32),
        card_height: impl Fn(&AnyItem) -> Pixels,
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
                .map(|row| row.height(&card_height))
                .sum::<Pixels>()
                * fraction;
            for (index, row) in group.iter().enumerate() {
                let full_height = row.height(&card_height);
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
                if expanded
                    && height == full_height
                    && let QueueRow::Item {
                        item,
                        projected: false,
                        ..
                    } = row
                {
                    layout.order.push(item.id());
                    layout.positions.insert(item.id(), layout.rows.len());
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
