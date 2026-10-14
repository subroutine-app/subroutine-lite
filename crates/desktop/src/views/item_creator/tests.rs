use chrono::{Duration, Local, NaiveDate, NaiveTime, TimeZone as _};
use parser::{EntityKind, ParseDraft};
use subroutine_core::{AnyItem, Event, ItemType, SchedulePoint, Signal};
use uuid::Uuid;

use super::{
    BatchRun, ItemDraft, batch_needs_timed_placement,
    draft::{Clause, Schedule},
    fresh_draft, parse_title,
};

fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2030, 6, 12).unwrap()
}

fn parse(text: &str, kind: EntityKind) -> ParseDraft {
    let now = Local
        .from_local_datetime(&date().and_hms_opt(12, 0, 0).unwrap())
        .earliest()
        .unwrap();
    parser::parse_with_context(text, kind, now, Duration::minutes(5)).unwrap()
}

fn sync_title(draft: &mut ItemDraft, text: &str, mode: ItemType) {
    let parsed = parse_title(text, mode, draft.parse_date()).unwrap();
    draft.sync_from_parse(parsed.as_ref()).unwrap();
}

fn action(draft: &ItemDraft) -> subroutine_core::Action {
    let AnyItem::Action(action) = draft
        .build(Uuid::nil(), ItemType::Action, "Review", None, Vec::new())
        .unwrap()
    else {
        panic!("expected an action");
    };
    action
}

#[test]
fn type_defaults_do_not_become_action_details() {
    for date in [None, Some(date())] {
        let mut draft = ItemDraft::new();
        draft.schedule.date = date;
        for mode in [
            ItemType::Action,
            ItemType::Event,
            ItemType::Signal,
            ItemType::Routine,
            ItemType::Marker,
            ItemType::Action,
        ] {
            draft.apply_mode_defaults(mode);
            assert_eq!(draft.schedule, Schedule { date, time: None });
            assert_eq!(draft.duration, None);
            assert!(!draft.queued);
            assert!(!draft.pinned);
            if mode == ItemType::Event {
                assert_eq!(draft.duration_for(mode), Some(Duration::hours(1)));
            }
        }
        let action = action(&draft);
        assert_eq!(action.start, date.map(SchedulePoint::Date));
        assert_eq!(action.duration, None);
        assert!(!action.queued);
        assert!(!action.pinned);
    }
}

#[test]
fn date_only_text_keeps_required_time_missing() {
    for (kind, mode, reason) in [
        (EntityKind::Event, ItemType::Event, "An event needs a time"),
        (
            EntityKind::Signal,
            ItemType::Signal,
            "A signal needs a time",
        ),
    ] {
        for text in ["Review tomorrow", "Review every Monday"] {
            let parsed = parse(text, kind.clone());
            let mut draft = ItemDraft::new();
            draft.apply_mode_defaults(mode);
            draft.sync_from_parse(Some(&parsed)).unwrap();
            assert!(draft.schedule.date.is_some());
            assert_eq!(draft.schedule.time, None);
            assert_eq!(draft.blocker(mode, "Review", 0), Some(reason));
            assert!(
                draft
                    .build(Uuid::nil(), mode, "Review", None, Vec::new())
                    .is_err()
            );

            draft.apply_mode_defaults(ItemType::Action);
            let action = action(&draft);
            assert!(matches!(action.start, Some(SchedulePoint::Date(_))));
            assert_eq!(action.duration, None);
            assert!(!action.queued);
            assert!(!action.pinned);
        }
    }
}

#[test]
fn explicit_details_and_parser_ownership_survive_type_switches() {
    let parsed = parse(
        "Review 2030-06-12 at 14:30 for 1h every day",
        EntityKind::Action,
    );
    let mut draft = ItemDraft::new();
    draft.sync_from_parse(Some(&parsed)).unwrap();
    let schedule = Schedule {
        date: Some(date()),
        time: NaiveTime::from_hms_opt(14, 30, 0),
    };
    assert_eq!(draft.schedule, schedule);
    assert!(draft.recurrence.is_some());
    let recurrence = draft.recurrence;
    draft.queued = false;
    draft.pinned = true;

    for mode in [
        ItemType::Event,
        ItemType::Marker,
        ItemType::Signal,
        ItemType::Action,
    ] {
        draft.apply_mode_defaults(mode);
        draft.sync_from_parse(Some(&parsed)).unwrap();
        assert_eq!(draft.schedule, schedule);
        assert_eq!(draft.duration, Some(Duration::hours(1)));
        assert_eq!(draft.recurrence, recurrence);
        assert!(!draft.queued);
        assert!(draft.pinned);
    }
    let action = action(&draft);
    assert_eq!(action.duration, Some(Duration::hours(1).into()));
    assert!(!action.queued);
    assert!(action.pinned);

    draft.sync_from_parse(None).unwrap();
    assert_eq!(draft.schedule, Schedule::default());
    assert_eq!(draft.duration, None);
    assert_eq!(draft.recurrence, None);
}

#[test]
fn entered_duration_is_distinct_from_the_event_suggestion() {
    let mut controlled = ItemDraft::new();
    controlled.apply_mode_defaults(ItemType::Event);
    controlled.step_duration(-1).unwrap();
    controlled.step_duration(1).unwrap();

    let mut contextual = ItemDraft::new();
    contextual.duration = Some(Duration::hours(1));

    let mut typed = ItemDraft::new();
    typed.apply_mode_defaults(ItemType::Event);
    typed
        .sync_from_parse(Some(&parse("Review for 1h", EntityKind::Event)))
        .unwrap();

    for mut draft in [controlled, contextual, typed.clone()] {
        draft.apply_mode_defaults(ItemType::Event);
        draft.apply_mode_defaults(ItemType::Action);
        assert_eq!(action(&draft).duration, Some(Duration::hours(1).into()));
    }

    sync_title(&mut typed, "Review", ItemType::Event);
    assert_eq!(typed.duration, None);
    assert_eq!(
        typed.duration_for(ItemType::Event),
        Some(Duration::hours(1))
    );
    typed.apply_mode_defaults(ItemType::Action);
    assert_eq!(action(&typed).duration, None);

    let mut cleared = ItemDraft::new();
    cleared.apply_mode_defaults(ItemType::Event);
    cleared.toggle_duration();
    cleared
        .sync_from_parse(Some(&parse("Review", EntityKind::Event)))
        .unwrap();
    assert_eq!(cleared.duration_for(ItemType::Event), None);
    cleared.apply_mode_defaults(ItemType::Action);
    assert_eq!(action(&cleared).duration, None);
}

#[test]
fn batch_reset_restores_entry_context_without_adding_precision() {
    let mut initial = ItemDraft::new();
    initial.schedule.date = Some(date());
    initial.span_days = 3;
    initial.queued = true;

    let mut draft = fresh_draft(&initial, ItemType::Action);
    let proposed = action(&draft);
    assert_eq!(proposed.start, Some(SchedulePoint::Date(date())));
    assert_eq!(proposed.duration, None);
    assert_eq!(draft.schedule.datetime(), None);
    assert!(!batch_needs_timed_placement(&proposed));
    let run = BatchRun::default().after_save(None);
    assert_eq!(run.placed, 1);
    assert_eq!(run.cursor, None);

    draft
        .sync_from_parse(Some(&parse(
            "Review tomorrow at 14:30 for 1h",
            EntityKind::Action,
        )))
        .unwrap();
    draft.pinned = true;
    assert!(draft != initial);
    assert!(batch_needs_timed_placement(&action(&draft)));
    let cursor = draft.schedule.datetime().unwrap() + Duration::hours(1);
    let run = run.after_save(Some(cursor));
    draft = fresh_draft(&initial, ItemType::Action);
    draft
        .sync_from_parse(Some(&parse("Next review", EntityKind::Action)))
        .unwrap();

    assert_eq!(
        draft.schedule,
        Schedule {
            date: Some(date()),
            time: None
        }
    );
    assert_eq!(draft.span_days, 3);
    assert_eq!(draft.duration, None);
    assert!(draft.queued);
    assert!(!draft.pinned);
    let proposed = action(&draft);
    assert_eq!(proposed.start, Some(SchedulePoint::Date(date())));
    assert_eq!(proposed.duration, None);
    assert!(!batch_needs_timed_placement(&proposed));
    let run = run.after_save(None);
    assert_eq!(run.placed, 3);
    assert_eq!(run.cursor, Some(cursor));

    draft.duration = Some(Duration::minutes(30));
    let proposed = action(&draft);
    assert_eq!(proposed.start, Some(SchedulePoint::Date(date())));
    assert_eq!(proposed.duration, Some(Duration::minutes(30).into()));
    assert!(!batch_needs_timed_placement(&proposed));
    assert!(batch_needs_timed_placement(&action(&ItemDraft::new())));
}

#[test]
fn creator_parsing_preserves_selected_dates_and_clocks() {
    let selected = Schedule {
        date: Some(date()),
        time: NaiveTime::from_hms_opt(10, 45, 0),
    };
    let mut draft = ItemDraft::new();
    draft.schedule = selected;
    draft.apply_mode_defaults(ItemType::Event);
    sync_title(&mut draft, "Review", ItemType::Event);
    assert_eq!(draft.schedule, selected);
    assert!(!draft.queued);

    sync_title(&mut draft, "Review at 3pm", ItemType::Event);
    assert_eq!(draft.schedule.date, selected.date);
    assert_eq!(draft.schedule.time, NaiveTime::from_hms_opt(15, 0, 0));
    assert!(draft.queued);
    draft.toggle_queued();
    sync_title(&mut draft, "Review", ItemType::Event);
    assert_eq!(draft.schedule, selected);
    assert!(!draft.queued);

    sync_title(&mut draft, "Review at 3pm", ItemType::Event);
    draft.step_days(1);
    draft.disown(Clause::When);
    sync_title(&mut draft, "Review", ItemType::Event);
    let changed_date = date().succ_opt();
    assert_eq!(draft.schedule.date, changed_date);
    assert_eq!(draft.schedule.time, NaiveTime::from_hms_opt(15, 0, 0));
    sync_title(&mut draft, "Review at 4pm", ItemType::Event);
    assert_eq!(draft.schedule.date, changed_date);
    assert_eq!(draft.schedule.time, NaiveTime::from_hms_opt(16, 0, 0));

    sync_title(&mut draft, "Review 2031-07-05 at 2pm", ItemType::Event);
    assert_eq!(draft.schedule.date, NaiveDate::from_ymd_opt(2031, 7, 5));
    assert_eq!(draft.schedule.time, NaiveTime::from_hms_opt(14, 0, 0));
}

#[test]
fn recurrence_dates_preserve_control_owned_time_and_queue_intent() {
    let selected = Schedule {
        date: Some(date()),
        time: NaiveTime::from_hms_opt(11, 0, 0),
    };
    let mut draft = ItemDraft::new();
    draft.schedule = selected;
    sync_title(&mut draft, "Review every Monday", ItemType::Event);
    assert_ne!(draft.schedule.date, selected.date);
    assert_eq!(draft.schedule.time, selected.time);
    assert!(!draft.queued);
    sync_title(&mut draft, "Review", ItemType::Event);
    assert_eq!(draft.schedule, selected);

    sync_title(&mut draft, "Review until 2030-07-01", ItemType::Event);
    sync_title(
        &mut draft,
        "Review every day until 2030-07-01",
        ItemType::Event,
    );
    assert_eq!(draft.schedule, selected);
    assert_eq!(
        draft.recurrence.unwrap().end_date,
        NaiveDate::from_ymd_opt(2030, 7, 1)
    );
    assert!(!draft.queued);
}

#[test]
fn timed_item_edits_reject_cleared_clocks() {
    let start = Local
        .from_local_datetime(&date().and_hms_opt(14, 37, 23).unwrap())
        .earliest()
        .unwrap()
        .to_utc();
    let next_date = date().succ_opt().unwrap();
    for original in [
        AnyItem::Event(Event::new("Review", start, Duration::minutes(45))),
        AnyItem::Signal(Signal::new("Review", start)),
    ] {
        let baseline = ItemDraft::from_item(&original).unwrap();
        let mut draft = baseline.clone();
        draft.schedule.date = Some(next_date);
        let updated = draft
            .apply_to(&baseline, &original, "Review", None)
            .unwrap();
        let expected = Local
            .from_local_datetime(&next_date.and_time(start.with_timezone(&Local).time()))
            .earliest()
            .unwrap()
            .to_utc();
        assert_eq!(updated.start(), Some(SchedulePoint::DateTime(expected)));

        draft.schedule.time = None;
        assert_eq!(draft.schedule.datetime(), None);
        assert!(
            draft
                .apply_to(&baseline, &original, "Review", None)
                .is_err()
        );

        draft.schedule.time = NaiveTime::from_hms_opt(16, 0, 0);
        let updated = draft
            .apply_to(&baseline, &original, "Review", None)
            .unwrap();
        assert_eq!(updated.start(), draft.schedule.point());

        draft.schedule = Schedule::default();
        assert!(
            draft
                .apply_to(&baseline, &original, "Review", None)
                .is_err()
        );
    }
}
