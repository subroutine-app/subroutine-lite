use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    Action, PipelineContext, SchedulePoint,
    schedule::{duration_end, find_free_slot, scheduled_time},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchPlacement {
    pub action: Action,
    pub displaced: Vec<Action>,
    pub cursor: DateTime<Utc>,
}

impl BatchPlacement {
    pub fn moved(&self) -> impl Iterator<Item = &Action> {
        std::iter::once(&self.action).chain(self.displaced.iter())
    }
}

impl PipelineContext<'_> {
    pub fn batch_start(&self) -> Result<DateTime<Utc>, &'static str> {
        self.quantized_now()
    }

    pub fn place_in_batch(
        &self,
        cursor: DateTime<Utc>,
        action: Action,
    ) -> Result<BatchPlacement, &'static str> {
        let actions: Vec<_> = self
            .actions
            .iter()
            .filter(|existing| existing.id != action.id)
            .cloned()
            .collect();
        let context = PipelineContext {
            actions: &actions,
            ..*self
        };
        let from = match action.start {
            Some(SchedulePoint::DateTime(named)) => named,
            _ => cursor,
        };
        let duration = context.effective_duration(&action);

        let mut unavailable = context.build_anchors(Some(from))?;
        for action in context
            .actions
            .iter()
            .filter(|action| !action.pinned && !action.is_completed())
        {
            if let Some(start) = scheduled_time(action) {
                let end = duration_end(start, context.effective_duration(action))?;
                if end > from {
                    unavailable.push((start, end));
                }
            }
        }
        let start = find_free_slot(from, duration, &unavailable, context.config.granularity)?;

        let mut placed = action;
        placed.set_queued(true);
        placed.set_start(Some(SchedulePoint::DateTime(start)));

        Ok(BatchPlacement {
            displaced: Vec::new(),
            action: placed,
            cursor: context.quantize_ceil(duration_end(start, duration)?)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};
    use chronoutil::RelativeDuration;

    use crate::{Event, ScheduleConfig};

    fn time(hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 16, hour, minute, 0).unwrap()
    }

    fn context<'a>(actions: &'a [Action], events: &'a [Event]) -> PipelineContext<'a> {
        PipelineContext::new(
            time(13, 0).into(),
            ScheduleConfig::new(RelativeDuration::minutes(5), Duration::minutes(5)),
            actions,
            events,
            &[],
            &[],
            &[],
        )
    }

    fn scheduled(hour: u32, minute: u32, duration: i64) -> Action {
        Action::new("Existing")
            .with_queued(true)
            .with_start(Some(time(hour, minute).into()))
            .with_duration(Some(RelativeDuration::minutes(duration)))
    }

    #[test]
    fn batch_skips_occupied_slots_without_moving_existing_actions() {
        let mut actions = vec![scheduled(15, 0, 5), scheduled(14, 0, 10)];
        let mut cursor = time(13, 50);

        for expected in [time(13, 50), time(13, 55), time(14, 10), time(14, 15)] {
            let placement = context(&actions, &[])
                .place_in_batch(cursor, Action::new("Next"))
                .unwrap();
            assert_eq!(placement.action.start, Some(expected.into()));
            assert_eq!(placement.cursor, expected + Duration::minutes(5));
            assert!(placement.displaced.is_empty());
            cursor = placement.cursor;
            actions.push(placement.action);
        }
    }

    #[test]
    fn batch_finds_a_full_gap_after_overlapping_actions_and_events() {
        let actions = [
            scheduled(14, 40, 5),
            scheduled(14, 20, 5).with_pinned(true),
            scheduled(14, 0, 10),
        ];
        let events = [Event::new("Meeting", time(14, 8), Duration::minutes(9))];
        let placement = context(&actions, &events)
            .place_in_batch(
                time(13, 0),
                Action::new("Next")
                    .with_start(Some(time(13, 55).into()))
                    .with_duration(Some(RelativeDuration::minutes(10))),
            )
            .unwrap();

        assert_eq!(placement.action.start, Some(time(14, 25).into()));
        assert_eq!(placement.cursor, time(14, 35));
        assert!(placement.displaced.is_empty());
    }

    #[test]
    fn batch_ignores_itself_and_items_that_do_not_block_time() {
        let action = scheduled(14, 0, 5);
        let actions = [
            action.clone(),
            scheduled(14, 0, 30).with_completion(Some(time(13, 0))),
            Action::new("Unscheduled").with_duration(Some(RelativeDuration::minutes(30))),
        ];
        let mut event = Event::new("Available", time(14, 0), Duration::hours(1));
        event.source_busy = Some(false);
        let placement = context(&actions, &[event])
            .place_in_batch(time(14, 0), action)
            .unwrap();

        assert_eq!(placement.action.start, Some(time(14, 0).into()));
        assert!(placement.displaced.is_empty());
    }
}
