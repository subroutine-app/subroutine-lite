use std::collections::HashSet;

use chrono::{DateTime, Local, Utc};
use gpui::{App, Context, Window};
use gpui_kit::{display::badge::Tone, overlay::toast};
use subroutine_core::{AnyItem, SchedulePoint};
use uuid::Uuid;

use crate::{
    components::{DraggedItems, timed_toast},
    item_manager::ItemManager,
    stores::AppDatabaseStore,
    views::drop_confirmation::confirm_drop,
};

use super::{QueueLane, QueueView};

pub(super) fn accepts(dragged: &DraggedItems, target: QueueLane, cx: &App) -> bool {
    if target == QueueLane::Missed {
        return false;
    }
    let now = Utc::now();
    live_actions(dragged.actions().map(|action| action.id), now, cx)
        .into_iter()
        .any(|item| move_to_lane(item, target, now).is_some())
}

pub(super) fn has_actions(dragged: &DraggedItems, cx: &App) -> bool {
    !live_actions(dragged.actions().map(|action| action.id), Utc::now(), cx).is_empty()
}

pub(super) fn hint(target: QueueLane) -> &'static str {
    match target {
        QueueLane::Missed => "Drag missed items to Today or Unscheduled",
        QueueLane::Today => "Move to Today (date only)",
        QueueLane::Unscheduled => "Move to Unscheduled (no date or time)",
    }
}

fn live_actions(ids: impl IntoIterator<Item = Uuid>, now: DateTime<Utc>, cx: &App) -> Vec<AnyItem> {
    let store = AppDatabaseStore::global(cx);
    let store = store.read(cx);
    if !store.is_ready() {
        return Vec::new();
    }
    let manager = ItemManager::global(cx);
    let manager = manager.read(cx);
    let mut seen = HashSet::new();
    ids.into_iter()
        .filter(|id| seen.insert(*id) && !manager.is_completing(*id))
        .filter_map(|id| store.get_item(id))
        .filter(|item| QueueLane::for_item(item, now).is_some())
        .collect()
}

fn move_to_lane(item: AnyItem, target: QueueLane, now: DateTime<Utc>) -> Option<AnyItem> {
    if QueueLane::for_item(&item, now)? == target {
        return None;
    }
    let AnyItem::Action(mut action) = item else {
        return None;
    };
    let today = now.with_timezone(&Local).date_naive();
    action.start = match target {
        QueueLane::Missed => return None,
        QueueLane::Today => Some(SchedulePoint::Date(today)),
        QueueLane::Unscheduled => None,
    };
    action.pinned = false;
    Some(AnyItem::Action(action))
}

impl QueueView {
    pub(super) fn commit_lane_drop(
        &mut self,
        dragged: &DraggedItems,
        target: QueueLane,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_drop_target(cx);
        let now = Utc::now();
        let ids: Vec<_> = live_actions(dragged.actions().map(|action| action.id), now, cx)
            .into_iter()
            .filter_map(|item| move_to_lane(item, target, now))
            .map(|item| item.id())
            .collect();
        confirm_drop(
            ids.len(),
            "Move",
            format!(
                "{}. Actions stay queued; times and pins are cleared.",
                hint(target)
            ),
            window,
            cx,
            move |window, cx| {
                let now = Utc::now();
                let updated = live_actions(ids, now, cx)
                    .into_iter()
                    .filter_map(|item| move_to_lane(item, target, now))
                    .collect::<Vec<_>>();
                if updated.is_empty() {
                    return;
                }
                let result = AppDatabaseStore::global(cx)
                    .update(cx, |store, cx| store.update_items(updated, cx));
                if let Err(error) = result {
                    toast::push(
                        window,
                        cx,
                        timed_toast("item.save-failed", error).tone(Tone::Warning),
                    );
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, NaiveDate, TimeZone};
    use chronoutil::RelativeDuration;
    use subroutine_core::{Action, ActionTemplate, Event, Recurrence, RecurrenceRule};

    use super::*;

    fn now() -> DateTime<Utc> {
        Local
            .from_local_datetime(
                &NaiveDate::from_ymd_opt(2026, 1, 5)
                    .unwrap()
                    .and_hms_opt(12, 0, 0)
                    .unwrap(),
            )
            .earliest()
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn moves_to_lane_dates_and_clears_only_the_pin() {
        let now = now();
        let today = now.with_timezone(&Local).date_naive();

        let action = Action::new("Queued action")
            .with_content(Some("Keep this content"))
            .with_queued(true)
            .with_pinned(true)
            .with_duration(Some(RelativeDuration::minutes(25)))
            .with_recurrence(Some(Recurrence::new(RecurrenceRule::days(1))))
            .with_recurrence_id(Uuid::from_u128(10))
            .with_routine_id(Uuid::from_u128(11))
            .with_template_id(Uuid::from_u128(12))
            .with_source_provider("calendar")
            .with_source_external_id("external-action");

        for (start, target, expected_start) in [
            (
                Some(now - Duration::days(2)),
                QueueLane::Today,
                Some(SchedulePoint::Date(today)),
            ),
            (Some(now - Duration::days(2)), QueueLane::Unscheduled, None),
            (Some(now), QueueLane::Unscheduled, None),
            (None, QueueLane::Today, Some(SchedulePoint::Date(today))),
        ] {
            let original = action
                .clone()
                .with_start(start.map(SchedulePoint::DateTime));
            let updated = move_to_lane(AnyItem::Action(original.clone()), target, now).unwrap();
            let expected = AnyItem::Action(original.with_start(expected_start).with_pinned(false));
            assert_eq!(QueueLane::for_item(&updated, now), Some(target));
            assert_eq!(
                serde_json::to_value(updated).unwrap(),
                serde_json::to_value(expected).unwrap()
            );
        }
    }

    #[test]
    fn missed_rejects_drops_from_every_lane() {
        let now = now();
        let action = Action::new("Queued action").with_queued(true);
        for (start, source) in [
            (
                Some(SchedulePoint::DateTime(now - Duration::days(2))),
                QueueLane::Missed,
            ),
            (Some(SchedulePoint::DateTime(now)), QueueLane::Today),
            (None, QueueLane::Unscheduled),
        ] {
            let item = AnyItem::Action(action.clone().with_start(start));
            assert_eq!(QueueLane::for_item(&item, now), Some(source));
            assert!(move_to_lane(item, QueueLane::Missed, now).is_none());
        }
    }

    #[test]
    fn invalid_candidates_and_same_lane_drops_are_no_ops() {
        let now = now();
        let today = now.with_timezone(&Local).date_naive();
        let action = Action::new("Queued action")
            .with_queued(true)
            .with_pinned(true);
        for (start, lane) in [
            (Some(SchedulePoint::DateTime(now)), QueueLane::Today),
            (
                Some(SchedulePoint::Date(today.pred_opt().unwrap())),
                QueueLane::Missed,
            ),
            (None, QueueLane::Unscheduled),
        ] {
            assert!(
                move_to_lane(AnyItem::Action(action.clone().with_start(start)), lane, now)
                    .is_none()
            );
        }

        let invalid = [
            AnyItem::Action(action.clone().with_queued(false)),
            AnyItem::Action(action.clone().with_completion(Some(now))),
            AnyItem::Action(
                action
                    .clone()
                    .with_start(Some(SchedulePoint::DateTime(now + Duration::seconds(1)))),
            ),
            AnyItem::Action(
                action.with_start(Some(SchedulePoint::Date(today.succ_opt().unwrap()))),
            ),
            AnyItem::ActionTemplate(ActionTemplate::new("Template")),
            AnyItem::Event(Event::new("Event", now, RelativeDuration::hours(1))),
        ];
        for item in invalid {
            for target in QueueLane::ALL {
                assert!(move_to_lane(item.clone(), target, now).is_none());
            }
        }
    }
}
