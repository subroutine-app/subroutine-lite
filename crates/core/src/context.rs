use chrono::{DateTime, Days, Duration, Local, NaiveDate, Utc};
use chronoutil::RelativeDuration;

use crate::{
    Action, AnyItem, Event, Marker, Routine, SchedulePoint, Signal, StartPrecision,
    schedule::{
        ScheduleConfig, find_free_slot, find_free_slot_backward, requeue_at, scheduled_time,
    },
};

pub struct PipelineContext<'a> {
    pub now: DateTime<Local>,
    pub config: ScheduleConfig,
    pub actions: &'a [Action],
    pub events: &'a [Event],
    pub routines: &'a [Routine],
    pub markers: &'a [Marker],
    pub signals: &'a [Signal],
}

fn visible_event(event: &Event) -> bool {
    event.source_provider.is_some() || event.id == event.lineage_id || event.recurrence.is_none()
}

fn visible_marker(marker: &Marker) -> bool {
    marker.source_provider.is_some()
        || marker.id == marker.lineage_id
        || marker.recurrence.is_none()
}

impl<'a> PipelineContext<'a> {
    pub fn new(
        now: DateTime<Local>,
        config: ScheduleConfig,
        actions: &'a [Action],
        events: &'a [Event],
        routines: &'a [Routine],
        markers: &'a [Marker],
        signals: &'a [Signal],
    ) -> Self {
        Self {
            actions,
            events,
            routines,
            markers,
            signals,
            now,
            config,
        }
    }

    pub fn now_utc(&self) -> DateTime<Utc> {
        self.now.with_timezone(&Utc)
    }

    pub fn quantized_now(&self) -> DateTime<Utc> {
        self.quantize_ceil(self.now_utc())
    }

    pub fn today(&self) -> NaiveDate {
        self.now.date_naive()
    }

    pub fn tomorrow(&self) -> NaiveDate {
        self.now.date_naive() + RelativeDuration::days(1)
    }

    pub fn quantize_floor(&self, dt: DateTime<Utc>) -> DateTime<Utc> {
        crate::schedule::quantize_floor(dt, self.config.granularity)
    }

    pub fn quantize_ceil(&self, dt: DateTime<Utc>) -> DateTime<Utc> {
        crate::schedule::quantize_ceil(dt, self.config.granularity)
    }

    pub fn quantize_duration(&self, duration: Duration) -> Duration {
        crate::schedule::quantize_duration(duration, self.config.granularity)
    }

    pub fn effective_duration(&self, action: &Action) -> RelativeDuration {
        action
            .duration
            .unwrap_or(self.config.default_action_duration)
    }

    pub fn queue(&self) -> Vec<&Action> {
        let mut queued: Vec<&Action> = self
            .actions
            .iter()
            .filter(|a| a.queued && !a.is_completed())
            .collect();
        queued.sort_by_key(|a| (a.start.is_none(), a.start.map(DateTime::<Utc>::from)));
        queued
    }

    pub fn queue_items(&self) -> Vec<AnyItem> {
        let mut items: Vec<AnyItem> = self
            .queue()
            .into_iter()
            .cloned()
            .map(AnyItem::Action)
            .chain(
                self.events
                    .iter()
                    .filter(|event| visible_event(event))
                    .cloned()
                    .map(AnyItem::Event),
            )
            .collect();
        items.sort_by_key(|item| {
            (
                item.start().is_none(),
                item.start().map(DateTime::<Utc>::from),
            )
        });
        items
    }

    pub fn waiting(&self) -> Vec<&Action> {
        self.actions
            .iter()
            .filter(|a| !a.queued && !a.is_completed())
            .collect()
    }

    pub fn timeline_items(&self) -> Vec<AnyItem> {
        let mut items = self.placed_items(StartPrecision::DateTime);
        items.extend(self.placed_items(StartPrecision::Date));
        items.retain(|item| self.is_scheduled_work(item));
        items.sort_by_key(|item| (item.start().map(DateTime::<Utc>::from), item.id()));
        items
    }

    pub fn all_day_items(&self) -> Vec<AnyItem> {
        let mut items = self.placed_items(StartPrecision::Date);
        items.retain(|item| self.is_scheduled_work(item));
        items
    }

    fn is_scheduled_work(&self, item: &AnyItem) -> bool {
        if !item.occupies_time() || item.is_completed() {
            return false;
        }
        match item {
            AnyItem::Action(action) => action.queued,
            _ => true,
        }
    }

    fn placed_items(&self, precision: StartPrecision) -> Vec<AnyItem> {
        let mut items: Vec<AnyItem> = self
            .actions
            .iter()
            .cloned()
            .map(AnyItem::Action)
            .chain(
                self.events
                    .iter()
                    .filter(|event| visible_event(event))
                    .cloned()
                    .map(AnyItem::Event),
            )
            .chain(self.routines.iter().cloned().map(AnyItem::Routine))
            .chain(
                self.markers
                    .iter()
                    .filter(|marker| visible_marker(marker))
                    .cloned()
                    .map(AnyItem::Marker),
            )
            .filter(|item| item.start_precision() == precision)
            .collect();
        items.sort_by_key(|item| item.start().map(DateTime::<Utc>::from));
        items
    }

    pub fn unfinished_events(&self) -> Vec<&Event> {
        let mut unfinished = self
            .events
            .iter()
            .filter(|event| visible_event(event))
            .filter(|e| !e.is_expired(self.now_utc()))
            .collect::<Vec<_>>();
        unfinished.sort_by_key(|e| e.start);
        unfinished
    }

    pub fn overdue_actions(&self) -> Vec<Action> {
        let now = self.quantized_now();
        let mut missed: Vec<Action> = self
            .actions
            .iter()
            .filter(|a| a.is_overdue(now))
            .cloned()
            .collect();
        missed.sort_by_key(scheduled_time);
        missed
    }

    pub fn build_anchors(
        &self,
        future_only_from: Option<DateTime<Utc>>,
    ) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
        let mut v: Vec<(DateTime<Utc>, DateTime<Utc>)> = self
            .events
            .iter()
            .filter(|event| visible_event(event) && event.blocks_time())
            .map(|e| (e.start, e.end_time()))
            .chain(self.actions.iter().filter_map(|a| {
                if !a.pinned {
                    return None;
                }
                let start = scheduled_time(a)?;
                if let Some(from) = future_only_from
                    && start < from
                {
                    return None;
                }
                Some((start, start + self.effective_duration(a)))
            }))
            .collect();

        let local_events: Vec<AnyItem> = self
            .events
            .iter()
            .filter(|event| event.source_provider.is_none() && visible_event(event))
            .cloned()
            .map(AnyItem::Event)
            .collect();
        let projection_start = self.now.date_naive();
        let projection_end = projection_start + Days::new(366);
        v.extend(
            crate::projected_items_between(&local_events, projection_start, projection_end)
                .into_iter()
                .filter_map(|item| match item {
                    AnyItem::Event(event) if event.blocks_time() => {
                        Some((event.start, event.end_time()))
                    }
                    _ => None,
                }),
        );

        v.sort_by_key(|(start, _)| *start);
        v
    }

    pub fn requeue_actions(&self) -> Vec<Action> {
        let now = self.quantized_now();
        let missed = self.overdue_actions();
        let anchors = self.build_anchors(Some(now));

        let mut cursor = now;
        let mut updates = Vec::with_capacity(missed.len());

        for action in missed {
            let duration = self.effective_duration(&action);
            let start = find_free_slot(cursor, duration, &anchors, self.config.granularity);
            cursor = start + duration;
            updates.push(requeue_at(&action, start));
        }

        if cursor > now {
            let cascade = self.push_actions_forward(now, (cursor - now).into());
            updates.extend(cascade);
        }

        updates
    }

    pub fn expedite_actions(&self, horizon: DateTime<Utc>) -> Vec<Action> {
        let now = self.quantized_now();
        let horizon = self.quantize_ceil(horizon);

        let mut candidates: Vec<Action> = self
            .actions
            .iter()
            .filter(|a| a.is_scheduled() && !a.pinned)
            .cloned()
            .collect();
        candidates.sort_by_key(scheduled_time);

        let anchors = self.build_anchors(None);

        let mut cursor = horizon;
        let mut updates: Vec<Action> = Vec::new();

        for action in candidates.iter().rev() {
            let duration = self.effective_duration(action);
            let start =
                find_free_slot_backward(cursor, duration, &anchors, now, self.config.granularity);
            cursor = start;

            let old_time =
                scheduled_time(action).expect("candidates are filtered to only scheduled actions");
            if start < old_time {
                updates.push(requeue_at(action, start));
            }
        }

        updates
    }

    pub fn push_actions_forward(
        &self,
        new_start: DateTime<Utc>,
        new_duration: RelativeDuration,
    ) -> Vec<Action> {
        let mut cursor = self.quantize_ceil(new_start + new_duration);

        let anchors = self.build_anchors(Some(new_start));

        let mut candidates: Vec<Action> = self
            .actions
            .iter()
            .filter(|a| !a.pinned && scheduled_time(a).is_some_and(|start| start >= new_start))
            .cloned()
            .collect();

        candidates.sort_by_key(scheduled_time);

        let mut updates = Vec::new();

        for action in candidates {
            let old_start =
                scheduled_time(&action).expect("candidates are filtered to only scheduled actions");
            let duration = self.effective_duration(&action);

            if old_start <= cursor {
                let new_action_start =
                    find_free_slot(cursor, duration, &anchors, self.config.granularity);
                cursor = new_action_start + duration;
                if new_action_start != old_start {
                    updates.push(requeue_at(&action, new_action_start));
                }
            } else {
                cursor = cursor.max(old_start + duration);
            }
        }

        updates
    }

    fn floating_queue_end(&self) -> Option<DateTime<Utc>> {
        self.actions
            .iter()
            .filter(|a| !a.pinned)
            .filter_map(|a| scheduled_time(a).map(|start| start + self.effective_duration(a)))
            .max()
    }

    pub fn next_slot(&self, duration: RelativeDuration) -> DateTime<Utc> {
        let now = self.quantized_now();
        let queue_end = self.floating_queue_end().unwrap_or(now).max(now);

        find_free_slot(
            self.quantize_ceil(queue_end),
            duration,
            &self.build_anchors(Some(now)),
            self.config.granularity,
        )
    }

    pub fn next_slot_for(&self, action: &Action) -> DateTime<Utc> {
        self.next_slot(self.effective_duration(action))
    }

    pub fn auto_queue_due_backlogged(&self) -> Vec<Action> {
        let today = self.today();

        let eligible: Vec<&Action> = self
            .actions
            .iter()
            .filter(|a| {
                !a.is_completed()
                    && !a.queued
                    && matches!(a.start, Some(SchedulePoint::Date(date)) if date <= today)
            })
            .collect();

        if eligible.is_empty() {
            return vec![];
        }

        let anchors = self.build_anchors(Some(self.quantized_now()));
        let mut cursor = self.next_slot_for(eligible[0]);
        let mut changed = Vec::with_capacity(eligible.len());
        let mut updated: Vec<Action> = self.actions.to_vec();

        for action in eligible {
            let duration = self.effective_duration(action);
            let start = find_free_slot(cursor, duration, &anchors, self.config.granularity);

            if let Some(entry) = updated.iter_mut().find(|a| a.id == action.id) {
                entry.set_queued(true);
                entry.set_start(Some(SchedulePoint::DateTime(start)));
                changed.push(entry.clone());
            }
            cursor = self.quantize_ceil(start + duration);
        }

        let updated_context = PipelineContext {
            actions: &updated,
            events: self.events,
            routines: self.routines,
            signals: self.signals,
            markers: self.markers,
            now: self.now,
            config: self.config,
        };
        changed.extend(updated_context.requeue_actions());

        changed
    }
}
