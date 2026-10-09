use chrono::{
    DateTime, Datelike as _, Duration, Local, Months, NaiveDate, NaiveTime, TimeZone, Timelike, Utc,
};
use chronoutil::RelativeDuration;
use parser::{HighlightKind, ParseDraft, ParserBuildError, WhenSpec, recurrence_to_rule};
use subroutine_core::{
    Action, AnyItem, Event, ItemType, Marker, Recurrence, RecurrenceRule, RecurrenceUnit, Routine,
    RoutineStep, SchedulePoint, Signal,
};

use crate::item_subject::{SavedItem, validate_item_timing, validate_saved_timing};
use uuid::Uuid;

const DAY: i64 = 1;
const STEP_MINUTES: i64 = 15;
const DEFAULT_EVENT_MINUTES: i64 = 60;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Schedule {
    pub date: Option<NaiveDate>,
    pub time: Option<NaiveTime>,
}

impl Schedule {
    pub fn is_set(&self) -> bool {
        self.date.is_some()
    }

    pub fn has_time(&self) -> bool {
        self.date.is_some() && self.time.is_some()
    }

    pub fn point(&self) -> Option<SchedulePoint> {
        let date = self.date?;
        match self.time {
            Some(time) => to_utc(date, time).map(SchedulePoint::DateTime),
            None => Some(SchedulePoint::Date(date)),
        }
    }

    pub fn datetime(&self) -> Option<DateTime<Utc>> {
        let date = self.date?;
        to_utc(date, self.time.unwrap_or(MIDNIGHT))
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.time.is_some() && self.date.is_none() {
            return Err("Choose a date for this time");
        }
        if self.has_time() && self.datetime().is_none() {
            return Err("This time does not exist in the local timezone. Choose another time.");
        }
        Ok(())
    }

    fn set_from_when(&mut self, when: Option<&WhenSpec>) {
        match when {
            Some(WhenSpec::DateTime(dt)) => {
                let local = dt.with_timezone(&Local);
                self.date = Some(local.date_naive());
                self.time = Some(local.time());
            }
            Some(WhenSpec::NaiveDate(date)) => {
                self.date = Some(*date);
                self.time = None;
            }
            None => {
                self.date = None;
                self.time = None;
            }
        }
    }

    pub fn shift_days(&mut self, days: i64) {
        let base = self.date.unwrap_or_else(today);
        self.date = base.checked_add_signed(Duration::days(days)).or(Some(base));
    }

    pub fn shift_minutes(&mut self, minutes: i64) {
        if self.date.is_none() {
            self.date = Some(today());
        }
        let base = self.time.unwrap_or_else(next_slot);
        let total = base.hour() as i64 * 60 + base.minute() as i64 + minutes;
        let day = 24 * 60;
        let rolled = total.rem_euclid(day);
        if total < 0 {
            self.shift_days(-1);
        } else if total >= day {
            self.shift_days(1);
        }
        self.time = NaiveTime::from_hms_opt((rolled / 60) as u32, (rolled % 60) as u32, 0);
    }

    pub fn toggle_date(&mut self) {
        if self.date.is_some() {
            self.date = None;
            self.time = None;
        } else {
            self.date = Some(today());
        }
    }

    pub fn toggle_time(&mut self) {
        if self.time.is_some() {
            self.time = None;
        } else {
            if self.date.is_none() {
                self.date = Some(today());
            }
            self.time = Some(next_slot());
        }
    }
}

const MIDNIGHT: NaiveTime = match NaiveTime::from_hms_opt(0, 0, 0) {
    Some(time) => time,
    None => unreachable!(),
};

fn today() -> NaiveDate {
    Local::now().date_naive()
}

fn next_slot() -> NaiveTime {
    let now = Local::now().time();
    let minutes = now.hour() as i64 * 60 + now.minute() as i64;
    let next = ((minutes / STEP_MINUTES) + 1) * STEP_MINUTES;
    let next = next.rem_euclid(24 * 60);
    NaiveTime::from_hms_opt((next / 60) as u32, (next % 60) as u32, 0).unwrap_or(MIDNIGHT)
}

fn to_utc(date: NaiveDate, time: NaiveTime) -> Option<DateTime<Utc>> {
    let naive = date.and_time(time);
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|dt| dt.to_utc())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Clause {
    When,
    Duration,
    Recurrence,
    RecurrenceEnd,
    RecurrenceCount,
}

impl Clause {
    pub fn highlight(self) -> HighlightKind {
        match self {
            Clause::When => HighlightKind::When,
            Clause::Duration => HighlightKind::Duration,
            Clause::Recurrence | Clause::RecurrenceEnd | Clause::RecurrenceCount => {
                HighlightKind::Recurrence
            }
        }
    }
}

#[derive(Clone, Default, PartialEq)]
struct Reading {
    when: Option<WhenSpec>,
    duration: Option<Duration>,
    recurrence: Option<Recurrence>,
    recurrence_end_date: Option<NaiveDate>,
    recurrence_remaining: Option<u32>,
    when_may_be_recurrence_end: bool,
}

impl Reading {
    fn of(draft: &ParseDraft) -> Result<Self, ParserBuildError> {
        Ok(Self {
            when: draft.when.clone(),
            duration: draft.duration,
            recurrence: recurrence_to_rule(draft.recurrence.as_ref())?,
            recurrence_end_date: draft.recurrence_end_date,
            recurrence_remaining: draft.recurrence_remaining,
            when_may_be_recurrence_end: draft.highlights.iter().any(|(range, kind)| {
                *kind == HighlightKind::When
                    && draft.raw[..range.start]
                        .trim_end()
                        .to_ascii_lowercase()
                        .ends_with("until")
            }),
        })
    }
}

#[derive(Clone, PartialEq)]
pub struct ItemDraft {
    pub schedule: Schedule,
    pub duration: Option<Duration>,
    pub recurrence: Option<Recurrence>,
    pub queued: bool,
    pub pinned: bool,
    pub span_days: u32,
    schedule_before_recurrence_bound: Option<(Schedule, bool)>,
    last_read: Reading,
}

impl Default for ItemDraft {
    fn default() -> Self {
        Self {
            schedule: Schedule::default(),
            duration: None,
            recurrence: None,
            queued: false,
            pinned: false,
            span_days: 1,
            schedule_before_recurrence_bound: None,
            last_read: Reading::default(),
        }
    }
}

impl ItemDraft {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn from_item(item: &AnyItem) -> Result<Self, &'static str> {
        validate_item_timing(item)?;
        let schedule = match item {
            AnyItem::ActionTemplate(template) => Schedule {
                date: None,
                time: template.naive_time,
            },
            _ => match item.start() {
                Some(SchedulePoint::DateTime(datetime)) => {
                    let local = datetime.with_timezone(&Local);
                    Schedule {
                        date: Some(local.date_naive()),
                        time: Some(local.time()),
                    }
                }
                Some(SchedulePoint::Date(date)) => Schedule {
                    date: Some(date),
                    time: None,
                },
                None => Schedule::default(),
            },
        };
        let duration = item
            .duration()
            .map(|duration| {
                let from = item.start_datetime().unwrap_or_else(Local::now).to_utc();
                subroutine_core::checked_duration_end(SchedulePoint::DateTime(from), duration)
                    .map(|end| DateTime::<Utc>::from(end) - from)
            })
            .transpose()?;
        let (queued, pinned, span_days) = match item {
            AnyItem::Action(action) => (action.queued, action.pinned, 1),
            AnyItem::Marker(marker) => (
                false,
                false,
                marker
                    .end_date
                    .map(|end| (end - marker.date).num_days().max(0) as u32 + 1)
                    .unwrap_or(1),
            ),
            _ => (false, false, 1),
        };

        Ok(Self {
            schedule,
            duration,
            recurrence: item.recurrence(),
            queued,
            pinned,
            span_days,
            schedule_before_recurrence_bound: None,
            last_read: Reading::default(),
        })
    }

    pub(crate) fn from_saved(item: &SavedItem) -> Result<Self, &'static str> {
        let now = Utc::now();
        let (time, duration) = match item {
            SavedItem::Action(template) => (template.naive_time, template.duration),
            SavedItem::Event(template) => (None, Some(template.duration)),
        };
        let duration = duration
            .map(|duration| {
                subroutine_core::checked_duration_end(SchedulePoint::DateTime(now), duration)
                    .map(|end| DateTime::<Utc>::from(end) - now)
            })
            .transpose()?;
        Ok(Self {
            schedule: Schedule { date: None, time },
            duration,
            recurrence: item.recurrence(),
            queued: false,
            pinned: false,
            span_days: 1,
            schedule_before_recurrence_bound: None,
            last_read: Reading::default(),
        })
    }

    pub(crate) fn apply_to_saved(
        &self,
        baseline: &Self,
        original: &SavedItem,
        title: &str,
        content: Option<String>,
    ) -> Result<SavedItem, &'static str> {
        let title = title.trim();
        if title.is_empty() {
            return Err("Name it first");
        }
        self.validate_timing(original.item_type())?;
        let updated = match original {
            SavedItem::Action(original) => {
                let mut template = original.clone();
                template.title = title.to_string();
                template.content = content;
                if self.schedule.time != baseline.schedule.time {
                    template.naive_time = self.schedule.time;
                }
                if self.duration != baseline.duration {
                    template.duration = self.duration.map(Into::into);
                }
                if self.recurrence != baseline.recurrence {
                    template.recurrence = self.recurrence;
                }
                SavedItem::Action(template)
            }
            SavedItem::Event(original) => {
                let mut template = original.clone();
                template.title = title.to_string();
                template.content = content;
                if self.duration != baseline.duration {
                    template.duration = self.duration.ok_or("An event needs a duration")?.into();
                }
                if self.recurrence != baseline.recurrence {
                    template.recurrence = self.recurrence;
                }
                SavedItem::Event(template)
            }
        };
        validate_saved_timing(&updated)?;
        Ok(updated)
    }

    pub(crate) fn apply_to(
        &self,
        baseline: &Self,
        original: &AnyItem,
        title: &str,
        content: Option<String>,
    ) -> Result<AnyItem, &'static str> {
        let title = title.trim();
        if title.is_empty() {
            return Err("Name it first");
        }
        self.validate_timing(original.item_type())?;
        let updated = match original {
            AnyItem::Action(original) => {
                let mut action = original.clone();
                action.title = title.to_string();
                action.content = content;
                if self.schedule != baseline.schedule {
                    action.start = self.schedule.point();
                }
                if self.duration != baseline.duration {
                    action.duration = self.duration.map(Into::into);
                }
                if self.recurrence != baseline.recurrence {
                    action.recurrence = self.recurrence;
                }
                if self.queued != baseline.queued {
                    action.queued = self.queued;
                }
                if self.pinned != baseline.pinned {
                    action.pinned =
                        self.pinned && matches!(action.start, Some(SchedulePoint::DateTime(_)));
                }
                AnyItem::Action(action)
            }
            AnyItem::Event(original) => {
                let mut event = original.clone();
                event.title = title.to_string();
                event.content = content;
                if self.schedule != baseline.schedule {
                    event.start = self
                        .schedule
                        .datetime()
                        .ok_or("An event needs a valid local time")?;
                }
                if self.duration != baseline.duration {
                    event.duration = self.duration.ok_or("An event needs a duration")?.into();
                }
                if self.recurrence != baseline.recurrence {
                    event.recurrence = self.recurrence;
                }
                AnyItem::Event(event)
            }
            AnyItem::Marker(original) => {
                let mut marker = original.clone();
                marker.title = title.to_string();
                marker.content = content;
                let date_changed = self.schedule != baseline.schedule;
                let span_changed = self.span_days != baseline.span_days;
                if date_changed || span_changed {
                    let date = if date_changed {
                        self.schedule.date.ok_or("A marker needs a date")?
                    } else {
                        marker.date
                    };
                    let current_span = marker
                        .end_date
                        .map(|end| (end - marker.date).num_days().max(0) as u32 + 1)
                        .unwrap_or(1);
                    let span = if span_changed {
                        self.span_days
                    } else {
                        current_span
                    };
                    marker.date = date;
                    marker.end_date = marker_end(date, span)?;
                }
                if self.recurrence != baseline.recurrence {
                    marker.recurrence = self.recurrence;
                }
                AnyItem::Marker(marker)
            }
            AnyItem::Signal(original) => {
                let mut signal = original.clone();
                signal.title = title.to_string();
                signal.content = content;
                if self.schedule != baseline.schedule {
                    signal.datetime = self
                        .schedule
                        .datetime()
                        .ok_or("A signal needs a valid local time")?;
                }
                if self.recurrence != baseline.recurrence {
                    signal.recurrence = self.recurrence;
                }
                AnyItem::Signal(signal)
            }
            AnyItem::Routine(original) => {
                let mut routine = original.clone();
                routine.title = title.to_string();
                routine.content = content;
                if self.schedule != baseline.schedule {
                    routine.target = self.schedule.point();
                }
                if self.recurrence != baseline.recurrence {
                    routine.recurrence = self.recurrence;
                }
                AnyItem::Routine(routine)
            }
            AnyItem::ActionTemplate(original) => {
                let mut template = original.clone();
                template.title = title.to_string();
                template.content = content;
                if self.schedule.time != baseline.schedule.time {
                    template.naive_time = self.schedule.time;
                }
                if self.duration != baseline.duration {
                    template.duration = self.duration.map(Into::into);
                }
                if self.recurrence != baseline.recurrence {
                    template.recurrence = self.recurrence;
                }
                AnyItem::ActionTemplate(template)
            }
            AnyItem::EventTemplate(original) => {
                let mut template = original.clone();
                template.title = title.to_string();
                template.content = content;
                if self.duration != baseline.duration {
                    template.duration = self.duration.ok_or("An event needs a duration")?.into();
                }
                if self.recurrence != baseline.recurrence {
                    template.recurrence = self.recurrence;
                }
                AnyItem::EventTemplate(template)
            }
        };
        validate_item_timing(&updated)?;
        Ok(updated)
    }

    pub fn sync_from_parse(&mut self, parsed: Option<&ParseDraft>) -> Result<(), ParserBuildError> {
        let read = parsed.map(Reading::of).transpose()?.unwrap_or_default();

        if read.when != self.last_read.when {
            let reclassified_as_bound = self.last_read.when_may_be_recurrence_end
                && read.when.is_none()
                && read.recurrence_end_date.is_some();
            if reclassified_as_bound {
                let (schedule, queued) = self
                    .schedule_before_recurrence_bound
                    .take()
                    .unwrap_or_default();
                self.schedule = schedule;
                self.queued = queued;
            } else {
                if read.when_may_be_recurrence_end && !self.last_read.when_may_be_recurrence_end {
                    self.schedule_before_recurrence_bound = Some((self.schedule, self.queued));
                }
                self.schedule.set_from_when(read.when.as_ref());
                self.queued = matches!(read.when, Some(WhenSpec::DateTime(_)));
            }
        }
        if !read.when_may_be_recurrence_end && read.recurrence_end_date.is_none() {
            self.schedule_before_recurrence_bound = None;
        }
        if read.duration != self.last_read.duration {
            self.duration = read.duration;
            self.span_days = read
                .duration
                .map(|duration| duration.num_days().max(1) as u32)
                .unwrap_or(1);
        }
        if read.recurrence != self.last_read.recurrence {
            let limits = self
                .recurrence
                .map(|recurrence| (recurrence.end_date, recurrence.remaining));
            self.recurrence = read.recurrence.map(|recurrence| match limits {
                Some((end_date, remaining)) => {
                    recurrence.with_end_date(end_date).with_remaining(remaining)
                }
                None => recurrence,
            });
        }
        if read.recurrence_end_date != self.last_read.recurrence_end_date
            && let Some(recurrence) = self.recurrence.as_mut()
        {
            recurrence.end_date = read.recurrence_end_date;
            if read.recurrence_end_date.is_some() {
                recurrence.remaining = None;
            }
        }
        if read.recurrence_remaining != self.last_read.recurrence_remaining
            && let Some(recurrence) = self.recurrence.as_mut()
        {
            recurrence.remaining = read.recurrence_remaining;
            if read.recurrence_remaining.is_some() {
                recurrence.end_date = None;
            }
        }

        self.last_read = read;
        Ok(())
    }

    pub fn forget_reading(&mut self) {
        self.last_read = Reading::default();
    }

    pub fn disown(&mut self, clause: Clause) {
        match clause {
            Clause::When => self.last_read.when = None,
            Clause::Duration => self.last_read.duration = None,
            Clause::Recurrence => self.last_read.recurrence = None,
            Clause::RecurrenceEnd => self.last_read.recurrence_end_date = None,
            Clause::RecurrenceCount => self.last_read.recurrence_remaining = None,
        }
    }

    pub fn apply_mode_defaults(&mut self, mode: ItemType) {
        match mode {
            ItemType::Action | ItemType::ActionTemplate | ItemType::Routine => {}
            ItemType::Event | ItemType::EventTemplate => {
                if self.schedule.date.is_none() {
                    self.schedule.date = Some(today());
                }
                if self.schedule.time.is_none() {
                    self.schedule.time = Some(next_slot());
                }
                if self.duration.is_none() {
                    self.duration = Some(Duration::minutes(DEFAULT_EVENT_MINUTES));
                }
            }
            ItemType::Signal => {
                if self.schedule.date.is_none() {
                    self.schedule.date = Some(today());
                }
                if self.schedule.time.is_none() {
                    self.schedule.time = Some(next_slot());
                }
            }
            ItemType::Marker => {
                if self.schedule.date.is_none() {
                    self.schedule.date = Some(today());
                }
                self.schedule.time = None;
            }
        }
    }

    pub fn step_days(&mut self, days: i64) {
        self.schedule.shift_days(days * DAY);
    }

    pub fn step_minutes(&mut self, steps: i64) {
        self.schedule.shift_minutes(steps * STEP_MINUTES);
    }

    pub fn toggle_date(&mut self) {
        self.schedule.toggle_date();
    }

    pub fn toggle_time(&mut self) {
        self.schedule.toggle_time();
    }

    pub fn step_duration(&mut self, steps: i64) -> Result<(), &'static str> {
        self.duration = stepped_duration(self.duration, steps, STEP_MINUTES)?;
        Ok(())
    }

    pub fn toggle_duration(&mut self) {
        self.duration = match self.duration {
            Some(_) => None,
            None => Some(Duration::minutes(DEFAULT_EVENT_MINUTES)),
        };
    }

    pub fn step_span(&mut self, days: i64) {
        let next = self.span_days as i64 + days;
        self.span_days = next.clamp(1, 366) as u32;
    }

    pub fn toggle_queued(&mut self) {
        self.queued = !self.queued;
    }

    pub fn toggle_pinned(&mut self) {
        self.pinned = !self.pinned;
    }

    pub fn cycle_recurrence(&mut self, forward: bool) -> Result<(), &'static str> {
        let presets = recurrence_presets();
        let current = self
            .recurrence
            .and_then(|rec| presets.iter().position(|rule| *rule == rec.rule))
            .map(|ix| ix as isize + 1)
            .unwrap_or(0);
        let limits = self
            .recurrence
            .map(|recurrence| (recurrence.end_date, recurrence.remaining));
        let len = presets.len() as isize + 1;
        let next = (current + if forward { 1 } else { -1 }).rem_euclid(len);
        self.recurrence = if next > 0 {
            let recurrence = Recurrence::in_local_timezone(presets[(next - 1) as usize])?;
            Some(match limits {
                Some((end_date, remaining)) => {
                    recurrence.with_end_date(end_date).with_remaining(remaining)
                }
                None => recurrence,
            })
        } else {
            None
        };
        Ok(())
    }

    pub fn step_recurrence_end_date(&mut self, days: i64) {
        let Some(recurrence) = self.recurrence.as_mut() else {
            return;
        };
        let default = self
            .schedule
            .date
            .unwrap_or_else(today)
            .checked_add_months(Months::new(1))
            .unwrap_or_else(|| self.schedule.date.unwrap_or_else(today));
        let base = recurrence.end_date.unwrap_or(default);
        recurrence.end_date = base.checked_add_signed(Duration::days(days)).or(Some(base));
        recurrence.remaining = None;
    }

    pub fn toggle_recurrence_end_date(&mut self) {
        let Some(recurrence) = self.recurrence.as_mut() else {
            return;
        };
        recurrence.end_date = match recurrence.end_date {
            Some(_) => None,
            None => {
                recurrence.remaining = None;
                let start = self.schedule.date.unwrap_or_else(today);
                start.checked_add_months(Months::new(1)).or(Some(start))
            }
        };
    }

    pub fn set_recurrence_end_date(&mut self, end_date: Option<NaiveDate>) {
        if let Some(recurrence) = self.recurrence.as_mut() {
            recurrence.end_date = end_date;
            if end_date.is_some() {
                recurrence.remaining = None;
            }
        }
    }

    pub fn toggle_recurrence_remaining(&mut self) {
        let Some(recurrence) = self.recurrence.as_mut() else {
            return;
        };
        recurrence.remaining = match recurrence.remaining {
            Some(_) => None,
            None => {
                recurrence.end_date = None;
                Some(5)
            }
        };
    }

    pub fn step_recurrence_remaining(&mut self, steps: i64) {
        let Some(recurrence) = self.recurrence.as_mut() else {
            return;
        };
        let current = i64::from(recurrence.remaining.unwrap_or(5));
        recurrence.remaining = Some((current + steps).clamp(0, i64::from(u32::MAX)) as u32);
        recurrence.end_date = None;
    }

    pub fn marker_end_date(&self) -> Result<Option<NaiveDate>, &'static str> {
        self.schedule
            .date
            .map(|start| marker_end(start, self.span_days))
            .transpose()
            .map(Option::flatten)
    }

    pub(crate) fn validate_timing(&self, mode: ItemType) -> Result<(), &'static str> {
        let saved = matches!(mode, ItemType::ActionTemplate | ItemType::EventTemplate);
        if !saved {
            self.schedule.validate()?;
        }
        if mode == ItemType::Marker {
            if self
                .duration
                .is_some_and(|duration| duration < Duration::days(1))
            {
                return Err("A marker needs at least one day");
            }
            self.marker_end_date()?;
        } else if let Some(duration) = self.duration {
            subroutine_core::checked_duration_end(
                if saved {
                    SchedulePoint::now()
                } else {
                    self.schedule.point().unwrap_or_else(SchedulePoint::now)
                },
                duration.into(),
            )?;
        }
        Ok(())
    }

    pub fn set_marker_range(&mut self, range: crate::dates::InclusiveDateRange) {
        self.schedule.date = Some(range.start());
        self.schedule.time = None;
        self.span_days = range.day_count();
    }

    pub fn blocker(&self, mode: ItemType, title: &str, step_count: usize) -> Option<&'static str> {
        if title.trim().is_empty() {
            return Some("Name it first");
        }
        if let Err(error) = self.validate_timing(mode) {
            return Some(error);
        }
        match mode {
            ItemType::Action | ItemType::ActionTemplate => None,
            ItemType::Event | ItemType::EventTemplate => {
                if !self.schedule.has_time() {
                    Some("An event needs a time")
                } else if self.duration.is_none() {
                    Some("An event needs a duration")
                } else {
                    None
                }
            }
            ItemType::Signal => (!self.schedule.has_time()).then_some("A signal needs a time"),
            ItemType::Marker => (!self.schedule.is_set()).then_some("A marker needs a date"),
            ItemType::Routine => (step_count == 0).then_some("Add at least one step"),
        }
    }

    pub fn build(
        &self,
        id: Uuid,
        mode: ItemType,
        title: &str,
        content: Option<String>,
        steps: Vec<RoutineStep>,
    ) -> Result<AnyItem, &'static str> {
        let title = title.trim();
        if let Some(error) = self.blocker(mode, title, steps.len()) {
            return Err(error);
        }
        let duration = self.duration.map(RelativeDuration::from);
        let item = match mode {
            ItemType::Action | ItemType::ActionTemplate => AnyItem::Action(Action {
                id,
                recurrence_id: id,
                ..Action::new(title)
                    .with_content(content)
                    .with_start(self.schedule.point())
                    .with_duration(duration)
                    .with_recurrence(self.recurrence)
                    .with_queued(self.queued)
                    .with_pinned(self.pinned && self.schedule.has_time())
            }),
            ItemType::Event | ItemType::EventTemplate => {
                let start = self
                    .schedule
                    .datetime()
                    .ok_or("An event needs a valid local time")?;
                let duration = duration.ok_or("An event needs a duration")?;
                AnyItem::Event(Event {
                    id,
                    lineage_id: id,
                    ..Event::new(title, start, duration)
                        .with_content(content)
                        .with_recurrence(self.recurrence)
                })
            }
            ItemType::Signal => {
                let datetime = self
                    .schedule
                    .datetime()
                    .ok_or("A signal needs a valid local time")?;
                let mut signal = Signal::new(title, datetime).with_content(content);
                if let Some(recurrence) = self.recurrence {
                    signal = signal.with_recurrence(recurrence);
                }
                AnyItem::Signal(Signal {
                    id,
                    lineage_id: id,
                    ..signal
                })
            }
            ItemType::Marker => {
                let date = self.schedule.date.ok_or("A marker needs a date")?;
                let mut marker = Marker::new(title, date);
                marker.content = content;
                marker.end_date = self.marker_end_date()?;
                marker.recurrence = self.recurrence;
                AnyItem::Marker(Marker {
                    id,
                    lineage_id: id,
                    ..marker
                })
            }
            ItemType::Routine => {
                let mut routine = Routine::new(title).with_steps(steps);
                routine.content = content;
                routine.target = self.schedule.point();
                routine.recurrence = self.recurrence;
                AnyItem::Routine(Routine {
                    id,
                    recurrence_id: id,
                    ..routine
                })
            }
        };
        validate_item_timing(&item)?;
        Ok(item)
    }
}

pub(crate) fn stepped_duration(
    duration: Option<Duration>,
    steps: i64,
    step_minutes: i64,
) -> Result<Option<Duration>, &'static str> {
    let current = duration.unwrap_or_else(Duration::zero).num_minutes();
    let duration = steps
        .checked_mul(step_minutes)
        .and_then(|delta| current.checked_add(delta))
        .and_then(Duration::try_minutes)
        .ok_or("Duration is out of range")?;
    Ok((duration > Duration::zero()).then_some(duration))
}

fn marker_end(start: NaiveDate, span: u32) -> Result<Option<NaiveDate>, &'static str> {
    let days = span
        .checked_sub(1)
        .ok_or("A marker needs at least one day")?;
    if days == 0 {
        return Ok(None);
    }
    start
        .checked_add_signed(Duration::days(i64::from(days)))
        .map(Some)
        .ok_or("The end date is outside the supported calendar range")
}

fn recurrence_presets() -> [RecurrenceRule; 5] {
    [
        RecurrenceRule::days(1),
        RecurrenceRule::WeeklyDays(chrono::WeekdaySet::from_array([
            chrono::Weekday::Mon,
            chrono::Weekday::Tue,
            chrono::Weekday::Wed,
            chrono::Weekday::Thu,
            chrono::Weekday::Fri,
        ])),
        RecurrenceRule::weeks(1),
        RecurrenceRule::months(1),
        RecurrenceRule::years(1),
    ]
}

pub fn format_date(date: NaiveDate) -> String {
    let today = today();
    match (date - today).num_days() {
        0 => "Today".into(),
        1 => "Tomorrow".into(),
        -1 => "Yesterday".into(),
        2..=6 => date.format("%a").to_string(),
        _ if date.year() == today.year() => date.format("%a %-d %b").to_string(),
        _ => date.format("%-d %b %Y").to_string(),
    }
}

pub fn format_time(time: NaiveTime) -> String {
    if time.minute() == 0 {
        time.format("%-I%P").to_string()
    } else {
        time.format("%-I:%M%P").to_string()
    }
}

pub fn format_duration(duration: Duration) -> String {
    let minutes = duration.num_minutes();
    if minutes <= 0 {
        return "none".into();
    }
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

pub fn format_recurrence(recurrence: &Recurrence) -> String {
    let weekdays = chrono::WeekdaySet::from_array([
        chrono::Weekday::Mon,
        chrono::Weekday::Tue,
        chrono::Weekday::Wed,
        chrono::Weekday::Thu,
        chrono::Weekday::Fri,
    ]);
    let weekends = chrono::WeekdaySet::from_array([chrono::Weekday::Sat, chrono::Weekday::Sun]);

    match recurrence.rule {
        RecurrenceRule::Relative { unit, interval } if interval.get() == 1 => match unit {
            RecurrenceUnit::Days => "daily".into(),
            RecurrenceUnit::Weeks => "weekly".into(),
            RecurrenceUnit::Months => "monthly".into(),
            RecurrenceUnit::Years => "yearly".into(),
        },
        RecurrenceRule::WeeklyDays(days) if days == weekdays => "weekdays".into(),
        RecurrenceRule::WeeklyDays(days) if days == weekends => "weekends".into(),
        _ => recurrence.rule.describe().replacen("every ", "", 1),
    }
}
