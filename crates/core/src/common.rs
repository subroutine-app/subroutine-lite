use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use chronoutil::RelativeDuration;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    Action, ActionTemplate, Event, EventTemplate, Marker, MarkerTemplate, Recurrence, Routine,
    SchedulePoint, Signal, SignalTemplate, StartPrecision, recurrence::occurrence_id,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ItemType {
    Action,
    Event,
    Routine,
    Marker,
    Signal,
    ActionTemplate,
    EventTemplate,
}

impl ItemType {
    pub fn occupies_time(&self) -> bool {
        matches!(self, ItemType::Action | ItemType::Event | ItemType::Routine)
    }

    pub fn is_template(&self) -> bool {
        matches!(self, ItemType::ActionTemplate | ItemType::EventTemplate)
    }
}

pub trait CoreItem {
    fn id(&self) -> Uuid;

    fn id_u64(&self) -> u64 {
        self.id().as_u64_pair().1
    }

    fn lineage_id(&self) -> Uuid {
        self.id()
    }

    fn item_type(&self) -> ItemType;

    fn title(&self) -> &str;

    fn content(&self) -> Option<String> {
        None
    }

    fn start(&self) -> Option<SchedulePoint>;

    fn start_precision(&self) -> StartPrecision {
        self.start().into()
    }

    fn start_date(&self) -> Option<NaiveDate> {
        self.start().map(|t| t.into())
    }

    fn start_datetime(&self) -> Option<DateTime<Local>> {
        self.start().map(|t| t.into())
    }

    fn duration(&self) -> Option<RelativeDuration> {
        None
    }

    fn end(&self) -> Option<SchedulePoint> {
        crate::checked_duration_end(self.start()?, self.duration()?).ok()
    }

    fn recurrence(&self) -> Option<Recurrence> {
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnyItem {
    Action(Action),
    Event(Event),
    Routine(Routine),
    Marker(Marker),
    Signal(Signal),
    ActionTemplate(ActionTemplate),
    EventTemplate(EventTemplate),
}

impl AnyItem {
    pub fn id(&self) -> Uuid {
        match self {
            AnyItem::Action(a) => a.id(),
            AnyItem::Event(e) => e.id(),
            AnyItem::Routine(r) => r.id(),
            AnyItem::Marker(m) => m.id(),
            AnyItem::Signal(s) => s.id(),
            AnyItem::ActionTemplate(t) => t.id(),
            AnyItem::EventTemplate(t) => t.id(),
        }
    }

    pub fn title(&self) -> &str {
        match self {
            AnyItem::Action(a) => a.title(),
            AnyItem::Event(e) => e.title(),
            AnyItem::Routine(r) => r.title(),
            AnyItem::Marker(m) => m.title(),
            AnyItem::Signal(s) => s.title(),
            AnyItem::ActionTemplate(t) => t.title(),
            AnyItem::EventTemplate(t) => t.title(),
        }
    }

    pub fn id_u64(&self) -> u64 {
        self.id().as_u64_pair().1
    }

    pub fn start_date(&self) -> Option<NaiveDate> {
        self.start().map(|start| start.into())
    }

    pub fn start_datetime(&self) -> Option<DateTime<Local>> {
        self.start().map(|start| start.into())
    }

    pub fn item_type(&self) -> ItemType {
        match self {
            AnyItem::Action(a) => a.item_type(),
            AnyItem::Event(e) => e.item_type(),
            AnyItem::Routine(r) => r.item_type(),
            AnyItem::Marker(m) => m.item_type(),
            AnyItem::Signal(s) => s.item_type(),
            AnyItem::ActionTemplate(t) => t.item_type(),
            AnyItem::EventTemplate(t) => t.item_type(),
        }
    }

    pub fn lineage_id(&self) -> Uuid {
        match self {
            AnyItem::Action(a) => a.lineage_id(),
            AnyItem::Event(e) => e.lineage_id(),
            AnyItem::Routine(r) => r.lineage_id(),
            AnyItem::Marker(m) => m.lineage_id(),
            AnyItem::Signal(s) => s.lineage_id(),
            AnyItem::ActionTemplate(t) => t.lineage_id(),
            AnyItem::EventTemplate(t) => t.lineage_id(),
        }
    }

    pub fn content(&self) -> Option<String> {
        match self {
            AnyItem::Action(a) => a.content(),
            AnyItem::Event(e) => e.content(),
            AnyItem::Routine(r) => r.content(),
            AnyItem::Marker(m) => m.content(),
            AnyItem::Signal(s) => s.content(),
            AnyItem::ActionTemplate(t) => t.content(),
            AnyItem::EventTemplate(t) => t.content(),
        }
    }

    pub fn start(&self) -> Option<SchedulePoint> {
        match self {
            AnyItem::Action(a) => a.start(),
            AnyItem::Event(e) => e.start(),
            AnyItem::Routine(r) => r.start(),
            AnyItem::Marker(m) => m.start(),
            AnyItem::Signal(s) => s.start(),
            AnyItem::ActionTemplate(t) => t.start(),
            AnyItem::EventTemplate(t) => t.start(),
        }
    }

    pub fn duration(&self) -> Option<RelativeDuration> {
        match self {
            AnyItem::Action(a) => a.duration(),
            AnyItem::Event(e) => e.duration(),
            AnyItem::Routine(r) => r.duration(),
            AnyItem::Marker(m) => CoreItem::duration(m),
            AnyItem::Signal(s) => s.duration(),
            AnyItem::ActionTemplate(t) => t.duration(),
            AnyItem::EventTemplate(t) => t.duration(),
        }
    }

    pub fn end(&self) -> Option<SchedulePoint> {
        match self {
            AnyItem::Action(a) => a.end(),
            AnyItem::Event(e) => e.end(),
            AnyItem::Routine(r) => r.end(),
            AnyItem::Marker(m) => m.end(),
            AnyItem::Signal(s) => s.end(),
            AnyItem::ActionTemplate(t) => t.end(),
            AnyItem::EventTemplate(t) => t.end(),
        }
    }

    pub fn recurrence(&self) -> Option<Recurrence> {
        match self {
            AnyItem::Action(a) => a.recurrence(),
            AnyItem::Event(e) => e.recurrence(),
            AnyItem::Routine(r) => r.recurrence(),
            AnyItem::Marker(m) => m.recurrence(),
            AnyItem::Signal(s) => s.recurrence(),
            AnyItem::ActionTemplate(t) => t.recurrence(),
            AnyItem::EventTemplate(t) => t.recurrence(),
        }
    }

    pub fn is_template(&self) -> bool {
        self.item_type().is_template()
    }

    pub fn is_completed(&self) -> bool {
        matches!(self, AnyItem::Action(a) if a.is_completed())
    }

    pub fn occupies_time(&self) -> bool {
        self.item_type().occupies_time()
    }

    pub fn start_precision(&self) -> StartPrecision {
        self.start().into()
    }

    pub fn occurrence_at(&self, start: SchedulePoint) -> Option<Self> {
        if let Some(duration) = self.duration() {
            crate::checked_duration_end(start, duration).ok()?;
        }
        let lineage_id = self.lineage_id();
        Some(match self {
            AnyItem::Action(source) => {
                let mut occurrence = source.clone();
                occurrence.id = occurrence_id(lineage_id, "action", start);
                occurrence.recurrence_id = lineage_id;
                occurrence.start = Some(start);
                occurrence.completion = None;
                occurrence.pinned = false;
                AnyItem::Action(occurrence)
            }
            AnyItem::Event(source) => {
                let mut occurrence = source.clone();
                occurrence.id = occurrence_id(lineage_id, "event", start);
                occurrence.lineage_id = lineage_id;
                occurrence.start = DateTime::<Utc>::from(start);
                AnyItem::Event(occurrence)
            }
            AnyItem::Routine(source) => {
                let mut occurrence = source.clone();
                occurrence.id = occurrence_id(lineage_id, "routine", start);
                occurrence.recurrence_id = lineage_id;
                occurrence.target = Some(start);
                AnyItem::Routine(occurrence)
            }
            AnyItem::Marker(source) => AnyItem::Marker(source.occurrence_on(start.date_naive())?),
            AnyItem::Signal(source) => {
                AnyItem::Signal(source.occurrence_at(DateTime::<Utc>::from(start)))
            }
            AnyItem::ActionTemplate(source) => AnyItem::ActionTemplate(source.clone()),
            AnyItem::EventTemplate(source) => AnyItem::EventTemplate(source.clone()),
        })
    }

    fn is_local_materialized_copy(&self) -> bool {
        match self {
            AnyItem::Event(event) => {
                event.source_provider.is_none()
                    && event.id != event.lineage_id
                    && event.recurrence.is_some()
            }
            AnyItem::Marker(marker) => {
                marker.source_provider.is_none()
                    && marker.id != marker.lineage_id
                    && marker.recurrence.is_some()
            }
            AnyItem::Signal(signal) => {
                signal.id != signal.lineage_id && signal.recurrence.is_some()
            }
            AnyItem::Action(_)
            | AnyItem::Routine(_)
            | AnyItem::ActionTemplate(_)
            | AnyItem::EventTemplate(_) => false,
        }
    }

    fn set_recurrence(&mut self, recurrence: Recurrence) {
        match self {
            AnyItem::Action(item) => item.recurrence = Some(recurrence),
            AnyItem::Event(item) => item.recurrence = Some(recurrence),
            AnyItem::Routine(item) => item.recurrence = Some(recurrence),
            AnyItem::Marker(item) => item.recurrence = Some(recurrence),
            AnyItem::Signal(item) => item.recurrence = Some(recurrence),
            AnyItem::ActionTemplate(item) => item.recurrence = Some(recurrence),
            AnyItem::EventTemplate(item) => item.recurrence = Some(recurrence),
        }
    }

    pub fn projections_between(&self, start: NaiveDate, end: NaiveDate) -> Vec<Self> {
        const MAX_PROJECTION_STEPS: usize = 4096;

        let (Some(mut recurrence), Some(mut cursor)) = (self.recurrence(), self.start()) else {
            return Vec::new();
        };
        let source_start = self.start_date().unwrap_or(start);
        let source_end = match self {
            AnyItem::Marker(marker) => marker.end_date.unwrap_or(marker.date),
            _ => self.end().map(NaiveDate::from).unwrap_or(source_start),
        };
        let lookback_days = self
            .duration()
            .and_then(|duration| crate::duration_lookback(duration).ok())
            .map(|duration| {
                duration.num_days() + i64::from(duration > Duration::days(duration.num_days()))
            })
            .unwrap_or_else(|| (source_end - source_start).num_days().max(0));
        let target = start
            .checked_sub_signed(Duration::days(lookback_days))
            .unwrap_or(start);
        (cursor, recurrence) = recurrence.fast_forward_before(cursor, target);

        let mut occurrences = Vec::new();

        for _ in 0..MAX_PROJECTION_STEPS {
            let Some((next, next_recurrence)) = recurrence.advance(cursor) else {
                break;
            };
            if next.timestamp() <= cursor.timestamp() {
                break;
            }
            cursor = next;
            recurrence = next_recurrence;

            let Some(mut occurrence) = self.occurrence_at(next) else {
                break;
            };
            occurrence.set_recurrence(next_recurrence);
            let Some(occurrence_start) = occurrence.start_date() else {
                break;
            };
            if occurrence_start > end {
                break;
            }
            let occurrence_end = match &occurrence {
                AnyItem::Marker(marker) => marker.end_date.unwrap_or(marker.date),
                _ => occurrence
                    .end()
                    .map(NaiveDate::from)
                    .unwrap_or(occurrence_start),
            };
            if occurrence_end >= start {
                occurrences.push(occurrence);
            }
        }

        occurrences
    }
}

pub fn projected_items_between(
    items: &[AnyItem],
    start: NaiveDate,
    end: NaiveDate,
) -> Vec<AnyItem> {
    let stored: HashSet<(ItemType, Uuid, SchedulePoint)> = items
        .iter()
        .filter(|item| !item.is_local_materialized_copy())
        .filter_map(|item| {
            item.start()
                .map(|start| (item.item_type(), item.lineage_id(), start))
        })
        .collect();
    let mut sources: HashMap<(ItemType, Uuid), &AnyItem> = HashMap::new();
    for item in items {
        if item.recurrence().is_none() || item.is_local_materialized_copy() {
            continue;
        }
        let key = (item.item_type(), item.lineage_id());
        let replace = sources
            .get(&key)
            .and_then(|current| current.start())
            .is_none_or(|current| {
                item.start()
                    .is_some_and(|candidate| candidate.timestamp() > current.timestamp())
            });
        if replace {
            sources.insert(key, item);
        }
    }

    let mut projected: Vec<AnyItem> = sources
        .into_values()
        .flat_map(|item| item.projections_between(start, end))
        .filter(|occurrence| {
            let Some(start) = occurrence.start() else {
                return false;
            };
            let key = (occurrence.item_type(), occurrence.lineage_id(), start);
            !stored.contains(&key)
        })
        .collect();
    projected.sort_by_key(|item| (item.start().map(|start| start.timestamp()), item.id()));
    projected
}

impl CoreItem for AnyItem {
    fn id(&self) -> Uuid {
        match self {
            AnyItem::Action(a) => a.id(),
            AnyItem::Event(e) => e.id(),
            AnyItem::Routine(r) => r.id(),
            AnyItem::Marker(m) => m.id(),
            AnyItem::Signal(s) => s.id(),
            AnyItem::ActionTemplate(t) => t.id(),
            AnyItem::EventTemplate(t) => t.id(),
        }
    }

    fn lineage_id(&self) -> Uuid {
        match self {
            AnyItem::Action(a) => a.lineage_id(),
            AnyItem::Event(e) => e.lineage_id(),
            AnyItem::Routine(r) => r.lineage_id(),
            AnyItem::Marker(m) => m.lineage_id(),
            AnyItem::Signal(s) => s.lineage_id(),
            AnyItem::ActionTemplate(t) => t.lineage_id(),
            AnyItem::EventTemplate(t) => t.lineage_id(),
        }
    }

    fn item_type(&self) -> ItemType {
        match self {
            AnyItem::Action(a) => a.item_type(),
            AnyItem::Event(e) => e.item_type(),
            AnyItem::Routine(r) => r.item_type(),
            AnyItem::Marker(m) => m.item_type(),
            AnyItem::Signal(s) => s.item_type(),
            AnyItem::ActionTemplate(t) => t.item_type(),
            AnyItem::EventTemplate(t) => t.item_type(),
        }
    }

    fn title(&self) -> &str {
        match self {
            AnyItem::Action(a) => a.title(),
            AnyItem::Event(e) => e.title(),
            AnyItem::Routine(r) => r.title(),
            AnyItem::Marker(m) => m.title(),
            AnyItem::Signal(s) => s.title(),
            AnyItem::ActionTemplate(t) => t.title(),
            AnyItem::EventTemplate(t) => t.title(),
        }
    }

    fn content(&self) -> Option<String> {
        match self {
            AnyItem::Action(a) => a.content(),
            AnyItem::Event(e) => e.content(),
            AnyItem::Routine(r) => r.content(),
            AnyItem::Marker(m) => m.content(),
            AnyItem::Signal(s) => s.content(),
            AnyItem::ActionTemplate(t) => t.content(),
            AnyItem::EventTemplate(t) => t.content(),
        }
    }

    fn start(&self) -> Option<SchedulePoint> {
        match self {
            AnyItem::Action(a) => a.start(),
            AnyItem::Event(e) => e.start(),
            AnyItem::Routine(r) => r.start(),
            AnyItem::Marker(m) => m.start(),
            AnyItem::Signal(s) => s.start(),
            AnyItem::ActionTemplate(t) => t.start(),
            AnyItem::EventTemplate(t) => t.start(),
        }
    }

    fn duration(&self) -> Option<RelativeDuration> {
        AnyItem::duration(self)
    }

    fn recurrence(&self) -> Option<Recurrence> {
        AnyItem::recurrence(self)
    }

    fn end(&self) -> Option<SchedulePoint> {
        match self {
            AnyItem::Action(a) => a.end(),
            AnyItem::Event(e) => e.end(),
            AnyItem::Routine(r) => r.end(),
            AnyItem::Marker(m) => m.end(),
            AnyItem::Signal(s) => s.end(),
            AnyItem::ActionTemplate(t) => t.end(),
            AnyItem::EventTemplate(t) => t.end(),
        }
    }
}

pub enum AnyTemplate {
    ActionTemplate(ActionTemplate),
    EventTemplate(EventTemplate),
    MarkerTemplate(MarkerTemplate),
    SignalTemplate(SignalTemplate),
}

impl CoreItem for AnyTemplate {
    fn id(&self) -> Uuid {
        match self {
            AnyTemplate::ActionTemplate(a) => a.id(),
            AnyTemplate::EventTemplate(e) => e.id(),
            AnyTemplate::MarkerTemplate(m) => m.id(),
            AnyTemplate::SignalTemplate(s) => s.id(),
        }
    }

    fn lineage_id(&self) -> Uuid {
        match self {
            AnyTemplate::ActionTemplate(a) => a.lineage_id(),
            AnyTemplate::EventTemplate(e) => e.lineage_id(),
            AnyTemplate::MarkerTemplate(m) => m.lineage_id(),
            AnyTemplate::SignalTemplate(s) => s.lineage_id(),
        }
    }

    fn item_type(&self) -> ItemType {
        match self {
            AnyTemplate::ActionTemplate(a) => a.item_type(),
            AnyTemplate::EventTemplate(e) => e.item_type(),
            AnyTemplate::MarkerTemplate(m) => m.item_type(),
            AnyTemplate::SignalTemplate(s) => s.item_type(),
        }
    }

    fn title(&self) -> &str {
        match self {
            AnyTemplate::ActionTemplate(a) => a.title(),
            AnyTemplate::EventTemplate(e) => e.title(),
            AnyTemplate::MarkerTemplate(m) => m.title(),
            AnyTemplate::SignalTemplate(s) => s.title(),
        }
    }

    fn content(&self) -> Option<String> {
        match self {
            AnyTemplate::ActionTemplate(a) => a.content(),
            AnyTemplate::EventTemplate(e) => e.content(),
            AnyTemplate::MarkerTemplate(m) => m.content(),
            AnyTemplate::SignalTemplate(s) => s.content(),
        }
    }

    fn start(&self) -> Option<SchedulePoint> {
        match self {
            AnyTemplate::ActionTemplate(a) => a.start(),
            AnyTemplate::EventTemplate(e) => e.start(),
            AnyTemplate::MarkerTemplate(m) => m.start(),
            AnyTemplate::SignalTemplate(s) => s.start(),
        }
    }

    fn end(&self) -> Option<SchedulePoint> {
        match self {
            AnyTemplate::ActionTemplate(a) => a.end(),
            AnyTemplate::EventTemplate(e) => e.end(),
            AnyTemplate::MarkerTemplate(m) => m.end(),
            AnyTemplate::SignalTemplate(s) => s.end(),
        }
    }
}

pub fn weekday_name(weekday: chrono::Weekday) -> &'static str {
    match weekday {
        chrono::Weekday::Mon => "monday",
        chrono::Weekday::Tue => "tuesday",
        chrono::Weekday::Wed => "wednesday",
        chrono::Weekday::Thu => "thursday",
        chrono::Weekday::Fri => "friday",
        chrono::Weekday::Sat => "saturday",
        chrono::Weekday::Sun => "sunday",
    }
}

pub fn weekday_from_name(name: &str) -> Option<chrono::Weekday> {
    match name.to_ascii_lowercase().as_str() {
        "monday" => Some(chrono::Weekday::Mon),
        "tuesday" => Some(chrono::Weekday::Tue),
        "wednesday" => Some(chrono::Weekday::Wed),
        "thursday" => Some(chrono::Weekday::Thu),
        "friday" => Some(chrono::Weekday::Fri),
        "saturday" => Some(chrono::Weekday::Sat),
        "sunday" => Some(chrono::Weekday::Sun),
        _ => None,
    }
}
