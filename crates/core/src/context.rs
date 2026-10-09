use chrono::{DateTime, Days, Duration, Local, NaiveDate, Utc};
use chronoutil::RelativeDuration;

use crate::{
    Action, AnyItem, Event, Marker, Routine, SchedulePoint, Signal, StartPrecision,
    schedule::{
        ScheduleConfig, duration_end, find_free_slot, find_free_slot_backward, overlaps,
        requeue_at, scheduled_time,
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

    pub fn quantized_now(&self) -> Result<DateTime<Utc>, &'static str> {
        self.quantize_ceil(self.now_utc())
    }

    pub fn today(&self) -> NaiveDate {
        self.now.date_naive()
    }

    pub fn tomorrow(&self) -> Option<NaiveDate> {
        self.today().succ_opt()
    }

    pub fn quantize_floor(&self, dt: DateTime<Utc>) -> Result<DateTime<Utc>, &'static str> {
        crate::schedule::quantize_floor(dt, self.config.granularity)
    }

    pub fn quantize_ceil(&self, dt: DateTime<Utc>) -> Result<DateTime<Utc>, &'static str> {
        crate::schedule::quantize_ceil(dt, self.config.granularity)
    }

    pub fn quantize_duration(&self, duration: Duration) -> Result<Duration, &'static str> {
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

    pub fn unfinished_events(&self) -> Result<Vec<&Event>, &'static str> {
        let now = self.now_utc();
        let mut unfinished = Vec::new();
        for event in self.events.iter().filter(|event| visible_event(event)) {
            if !event.is_expired(now)? {
                unfinished.push(event);
            }
        }
        unfinished.sort_by_key(|event| event.start);
        Ok(unfinished)
    }

    pub fn overdue_actions(&self) -> Result<Vec<Action>, &'static str> {
        let now = self.quantized_now()?;
        let mut missed: Vec<Action> = self
            .actions
            .iter()
            .filter(|a| !a.pinned && !a.is_completed() && a.is_overdue(now))
            .cloned()
            .collect();
        missed.sort_by_key(|action| (scheduled_time(action), action.id));
        Ok(missed)
    }

    pub fn build_anchors(
        &self,
        future_only_from: Option<DateTime<Utc>>,
    ) -> Result<Vec<(DateTime<Utc>, DateTime<Utc>)>, &'static str> {
        let mut anchors = Vec::new();
        for event in self
            .events
            .iter()
            .filter(|event| visible_event(event) && event.blocks_time())
        {
            anchors.push((event.start, duration_end(event.start, event.duration)?));
        }
        for action in self
            .actions
            .iter()
            .filter(|action| action.pinned && !action.is_completed())
        {
            if let Some(start) = scheduled_time(action) {
                anchors.push((start, duration_end(start, self.effective_duration(action))?));
            }
        }

        let local_events: Vec<AnyItem> = self
            .events
            .iter()
            .filter(|event| {
                event.source_provider.is_none()
                    && visible_event(event)
                    && event.blocks_time()
                    && event.recurrence.is_some()
            })
            .cloned()
            .map(AnyItem::Event)
            .collect();
        if !local_events.is_empty() {
            let projection_start = self.today();
            let projection_end = projection_start
                .checked_add_days(Days::new(366))
                .ok_or("event projection is outside the supported calendar range")?;
            for item in
                crate::projected_items_between(&local_events, projection_start, projection_end)
            {
                if let AnyItem::Event(event) = item {
                    anchors.push((event.start, duration_end(event.start, event.duration)?));
                }
            }
        }

        anchors.retain(|(_, end)| future_only_from.is_none_or(|from| *end > from));
        anchors.sort_by_key(|(start, _)| *start);
        Ok(anchors)
    }

    pub fn requeue_actions(&self) -> Result<Vec<Action>, &'static str> {
        let now = self.quantized_now()?;
        let missed = self.overdue_actions()?;
        let anchors = self.build_anchors(Some(now))?;

        let mut cursor = now;
        let mut updates = Vec::with_capacity(missed.len());

        for action in missed {
            let duration = self.effective_duration(&action);
            let start = find_free_slot(cursor, duration, &anchors, self.config.granularity)?;
            cursor = duration_end(start, duration)?;
            updates.push(requeue_at(&action, start));
        }

        if cursor > now {
            let cascade = self.push_actions_forward(now, (cursor - now).into())?;
            updates.extend(cascade);
        }

        Ok(updates)
    }

    pub fn expedite_actions(&self, horizon: DateTime<Utc>) -> Result<Vec<Action>, &'static str> {
        let now = self.quantized_now()?;
        if horizon < now {
            return Err("expedite horizon is before the next available scheduling time");
        }

        let mut candidates: Vec<_> = self
            .actions
            .iter()
            .filter(|action| !action.pinned && !action.is_completed())
            .filter_map(|action| scheduled_time(action).map(|start| (start, action)))
            .collect();
        candidates.sort_by_key(|(start, action)| (*start, action.id));

        let anchors = self.build_anchors(None)?;
        let mut cursor = horizon;
        let mut updates = Vec::new();

        for (old_start, action) in candidates.into_iter().rev() {
            let duration = self.effective_duration(action);
            let old_end = duration_end(old_start, duration)?;
            if old_end <= cursor && !overlaps(old_start, old_end, &anchors) {
                cursor = old_start;
                continue;
            }

            let start = find_free_slot_backward(
                cursor,
                duration,
                &anchors,
                now,
                old_start,
                self.config.granularity,
            )?;
            cursor = start;
            if start != old_start {
                updates.push(requeue_at(action, start));
            }
        }

        Ok(updates)
    }

    pub fn push_actions_forward(
        &self,
        new_start: DateTime<Utc>,
        new_duration: RelativeDuration,
    ) -> Result<Vec<Action>, &'static str> {
        let mut cursor = self.quantize_ceil(duration_end(new_start, new_duration)?)?;
        let anchors = self.build_anchors(Some(new_start))?;

        let mut candidates: Vec<_> = self
            .actions
            .iter()
            .filter(|action| !action.pinned && !action.is_completed())
            .filter_map(|action| scheduled_time(action).map(|start| (start, action)))
            .filter(|(start, _)| *start >= new_start)
            .collect();
        candidates.sort_by_key(|(start, action)| (*start, action.id));

        let mut updates = Vec::new();
        for (old_start, action) in candidates {
            let duration = self.effective_duration(action);
            let start = find_free_slot(
                cursor.max(old_start),
                duration,
                &anchors,
                self.config.granularity,
            )?;
            cursor = duration_end(start, duration)?;
            if start != old_start {
                updates.push(requeue_at(action, start));
            }
        }

        Ok(updates)
    }

    fn floating_queue_end(&self) -> Result<Option<DateTime<Utc>>, &'static str> {
        self.actions
            .iter()
            .filter(|action| !action.pinned && !action.is_completed())
            .filter_map(|action| scheduled_time(action).map(|start| (start, action)))
            .try_fold(None, |latest, (start, action)| {
                let end = duration_end(start, self.effective_duration(action))?;
                Ok(Some(
                    latest.map_or(end, |latest: DateTime<Utc>| latest.max(end)),
                ))
            })
    }

    pub fn next_slot(&self, duration: RelativeDuration) -> Result<DateTime<Utc>, &'static str> {
        let now = self.quantized_now()?;
        let queue_end = self.floating_queue_end()?.unwrap_or(now).max(now);

        find_free_slot(
            queue_end,
            duration,
            &self.build_anchors(Some(now))?,
            self.config.granularity,
        )
    }

    pub fn next_slot_for(&self, action: &Action) -> Result<DateTime<Utc>, &'static str> {
        self.next_slot(self.effective_duration(action))
    }

    pub fn auto_queue_due_backlogged(&self) -> Result<Vec<Action>, &'static str> {
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
            return Ok(vec![]);
        }

        let anchors = self.build_anchors(Some(self.quantized_now()?))?;
        let mut cursor = self.next_slot_for(eligible[0])?;
        let mut changed = Vec::with_capacity(eligible.len());
        let mut updated: Vec<Action> = self.actions.to_vec();

        for action in eligible {
            let duration = self.effective_duration(action);
            let start = find_free_slot(cursor, duration, &anchors, self.config.granularity)?;

            if let Some(entry) = updated.iter_mut().find(|a| a.id == action.id) {
                entry.set_queued(true);
                entry.set_start(Some(SchedulePoint::DateTime(start)));
                changed.push(entry.clone());
            }
            cursor = self.quantize_ceil(duration_end(start, duration)?)?;
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
        changed.extend(updated_context.requeue_actions()?);

        Ok(changed)
    }
}
