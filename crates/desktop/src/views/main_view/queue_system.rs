use chrono::{DateTime, Duration, Local, Utc};
use subroutine_core::{Action, AnyItem, Event, SchedulePoint};

use super::{EventMoment, TemporalSnapshot};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum QueueLane {
    Missed,
    Today,
    Unscheduled,
}

impl QueueLane {
    pub(crate) const ALL: [Self; 3] = [Self::Missed, Self::Today, Self::Unscheduled];

    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::Missed => "missed",
            Self::Today => "today",
            Self::Unscheduled => "unscheduled",
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Missed => "Missed",
            Self::Today => "Today",
            Self::Unscheduled => "Unscheduled",
        }
    }

    pub(crate) fn for_item(item: &AnyItem, now: DateTime<Utc>) -> Option<Self> {
        match item {
            AnyItem::Action(action) => Self::for_action(action, now),
            _ => None,
        }
    }

    fn for_action(action: &Action, now: DateTime<Utc>) -> Option<Self> {
        if !action.queued || action.is_completed() {
            return None;
        }
        let today = now.with_timezone(&Local).date_naive();
        let date = match action.start {
            Some(SchedulePoint::DateTime(start)) if start <= now => {
                start.with_timezone(&Local).date_naive()
            }
            Some(SchedulePoint::Date(date)) if date <= today => date,
            None => return Some(Self::Unscheduled),
            _ => return None,
        };
        Some(if date < today {
            Self::Missed
        } else {
            Self::Today
        })
    }
}

pub(crate) struct QueueSnapshot {
    pub candidates: Vec<AnyItem>,
    pub blocker: Option<EventMoment>,
}

impl QueueSnapshot {
    pub(crate) fn new(
        actions: &[Action],
        events: &[Event],
        now: DateTime<Utc>,
        granularity: Duration,
    ) -> Self {
        let mut ranked = actions
            .iter()
            .filter_map(|action| {
                let lane = QueueLane::for_action(action, now)?;
                let rank = match (lane, action.start) {
                    (QueueLane::Unscheduled, _) => 2,
                    (QueueLane::Today, Some(SchedulePoint::Date(_))) => 1,
                    _ => 0,
                };
                Some((
                    rank,
                    action.start.map(DateTime::<Utc>::from),
                    action.id,
                    action,
                ))
            })
            .collect::<Vec<_>>();
        ranked.sort_by_key(|(rank, scheduled, id, _)| (*rank, *scheduled, *id));
        let candidates = ranked
            .into_iter()
            .map(|(_, _, _, action)| AnyItem::Action(action.clone()))
            .collect();
        let blocker = TemporalSnapshot::new(events, &[], now, granularity, granularity)
            .event_notice()
            .cloned();
        Self {
            candidates,
            blocker,
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, TimeZone};
    use chronoutil::RelativeDuration;
    use subroutine_core::{ActionTemplate, Recurrence, RecurrenceRule};
    use uuid::Uuid;

    use super::*;

    fn local(day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
        Local
            .from_local_datetime(
                &NaiveDate::from_ymd_opt(2026, 1, day)
                    .unwrap()
                    .and_hms_opt(hour, minute, 0)
                    .unwrap(),
            )
            .earliest()
            .unwrap()
            .with_timezone(&Utc)
    }

    fn action(id: u128, start: Option<SchedulePoint>) -> Action {
        let mut action = Action::new("Queue action")
            .with_queued(true)
            .with_start(start);
        action.id = Uuid::from_u128(id);
        action.recurrence_id = action.id;
        action
    }

    fn ids(snapshot: &QueueSnapshot) -> Vec<Uuid> {
        snapshot.candidates.iter().map(AnyItem::id).collect()
    }

    #[test]
    fn ranks_only_queued_actions_doable_now_and_classifies_local_dates() {
        let now = local(5, 12, 0);
        let today = now.with_timezone(&Local).date_naive();
        let yesterday = today.pred_opt().unwrap();
        let actions = vec![
            action(9, None),
            action(5, Some(today.into())),
            action(4, Some(now.into())),
            action(3, Some(local(5, 9, 0).into())),
            action(2, Some(local(5, 9, 0).into())),
            action(1, Some(yesterday.into())),
            action(8, None).with_template_id(Uuid::from_u128(100)),
            action(6, Some(today.into())),
            action(20, Some((now + Duration::nanoseconds(1)).into())),
            action(21, Some(today.succ_opt().unwrap().into())),
            action(22, Some(yesterday.into())).with_queued(false),
            action(23, None).with_queued(false),
            action(24, Some(yesterday.into())).with_completion(Some(now)),
        ];
        let snapshot = QueueSnapshot::new(&actions, &[], now, Duration::minutes(5));
        assert_eq!(
            ids(&snapshot),
            [1, 2, 3, 4, 5, 6, 8, 9].map(Uuid::from_u128)
        );
        let mut reversed = actions.clone();
        reversed.reverse();
        assert_eq!(
            ids(&snapshot),
            ids(&QueueSnapshot::new(
                &reversed,
                &[],
                now,
                Duration::minutes(5)
            ))
        );
        assert_eq!(
            snapshot
                .candidates
                .iter()
                .map(|item| QueueLane::for_item(item, now))
                .collect::<Vec<_>>(),
            [
                QueueLane::Missed,
                QueueLane::Today,
                QueueLane::Today,
                QueueLane::Today,
                QueueLane::Today,
                QueueLane::Today,
                QueueLane::Unscheduled,
                QueueLane::Unscheduled,
            ]
            .map(Some)
        );
        for action in &actions[8..] {
            assert_eq!(
                QueueLane::for_item(&AnyItem::Action(action.clone()), now),
                None
            );
        }
        assert_eq!(
            QueueLane::for_item(
                &AnyItem::Action(action(25, Some(local(4, 23, 0).into()))),
                now
            ),
            Some(QueueLane::Missed)
        );
        assert_eq!(
            QueueLane::for_item(
                &AnyItem::ActionTemplate(ActionTemplate::new("Template")),
                now
            ),
            None
        );
        assert_eq!(
            QueueLane::for_item(
                &AnyItem::Event(Event::new("Event", now, RelativeDuration::hours(1))),
                now
            ),
            None
        );
    }

    #[test]
    fn blocks_at_threshold_and_start_but_releases_at_end_without_losing_candidates() {
        let start = local(5, 12, 0);
        let granularity = Duration::minutes(5);
        let events = [Event::new("Meeting", start, RelativeDuration::minutes(30))];
        let actions = [action(2, None), action(1, None)];
        let snapshot = |now| QueueSnapshot::new(&actions, &events, now, granularity);
        let before = snapshot(start - granularity - Duration::nanoseconds(1));
        assert!(before.blocker.is_none());
        let threshold = snapshot(start - granularity);
        assert!(
            matches!(threshold.blocker, Some(EventMoment::Upcoming { until, within_threshold: true, .. }) if until == granularity)
        );
        let active = snapshot(start);
        assert!(
            matches!(active.blocker, Some(EventMoment::InProgress { progress: 0.0, remaining, .. }) if remaining == Duration::minutes(30))
        );
        let end = events[0].end_time().unwrap();
        assert!(matches!(
            snapshot(end - Duration::nanoseconds(1)).blocker,
            Some(EventMoment::InProgress { .. })
        ));
        let finished = snapshot(end);
        assert!(finished.blocker.is_none());
        assert_eq!(ids(&before), ids(&threshold));
        assert_eq!(ids(&before), ids(&active));
        assert_eq!(ids(&before), ids(&finished));
        assert!(actions.iter().all(|action| action.start.is_none()));
    }

    #[test]
    fn active_event_takes_precedence_over_an_upcoming_event() {
        let now = local(5, 12, 0);
        let upcoming = Event::new(
            "Next",
            now + Duration::minutes(1),
            RelativeDuration::minutes(30),
        );
        let active = Event::new(
            "Current",
            now - Duration::minutes(10),
            RelativeDuration::minutes(30),
        );
        let ending = Event::new(
            "Ending first",
            now - Duration::minutes(5),
            RelativeDuration::minutes(10),
        );
        let expected = ending.id;
        let snapshot =
            QueueSnapshot::new(&[], &[upcoming, active, ending], now, Duration::minutes(5));
        assert!(
            matches!(snapshot.blocker, Some(EventMoment::InProgress { event, .. }) if event.id == expected)
        );
    }

    #[test]
    fn recurring_blocker_crosses_midnight_and_uses_the_stored_occurrence_when_present() {
        let source = Event::new("Nightly", local(2, 23, 55), RelativeDuration::minutes(30))
            .with_recurrence(Some(Recurrence::new(RecurrenceRule::days(1))));
        let occurrence = source.next_recurrence().unwrap().next_recurrence().unwrap();
        let events = [source.clone()];
        let granularity = Duration::minutes(5);
        let snapshot = |now| QueueSnapshot::new(&[], &events, now, granularity);
        assert!(
            snapshot(occurrence.start - granularity - Duration::nanoseconds(1))
                .blocker
                .is_none()
        );
        let upcoming = snapshot(occurrence.start - granularity).blocker.unwrap();
        assert!(matches!(
            upcoming,
            EventMoment::Upcoming {
                within_threshold: true,
                ..
            }
        ));
        assert_eq!(upcoming.notice_id(), occurrence.id);
        assert_eq!(upcoming.card_event().id, source.id);
        let now = local(5, 0, 10);
        let active = snapshot(now).blocker.unwrap();
        assert!(matches!(active, EventMoment::InProgress { .. }));
        assert_eq!(active.notice_id(), occurrence.id);
        assert!(snapshot(occurrence.end_time().unwrap()).blocker.is_none());

        let stored = occurrence.with_duration(RelativeDuration::minutes(10));
        assert!(
            QueueSnapshot::new(&[], &[source.clone(), stored.clone()], now, granularity)
                .blocker
                .is_none()
        );
        let active = QueueSnapshot::new(&[], &[source, stored.clone()], stored.start, granularity)
            .blocker
            .unwrap();
        assert_eq!(active.card_event(), &stored);
    }
}
