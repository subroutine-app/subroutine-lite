use chrono::{TimeZone, Weekday};

use super::*;
use crate::{
    ast::{HighlightKind, RecurrenceSpec, WeekdaySet},
    build::{BuildTarget, BuiltEntity, ParserBuildError, build_entity},
};

fn now() -> DateTime<Local> {
    Local
        .with_ymd_and_hms(2026, 10, 13, 12, 0, 0)
        .single()
        .unwrap()
}

fn selected_dates() -> [NaiveDate; 2] {
    [5, 26].map(|day| NaiveDate::from_ymd_opt(2026, 10, day).unwrap())
}

fn parse_on_date(input: &str, date: NaiveDate) -> ParseDraft {
    parse_tokens(
        input,
        EntityKind::Marker,
        now(),
        Duration::minutes(5),
        Some(date),
    )
    .unwrap()
}

#[test]
fn selected_date_defaults_titles_and_bare_times_without_extra_highlights() {
    for selected in selected_dates() {
        let draft = parse_on_date("Trip", selected);
        assert_eq!(draft.title, "Trip");
        assert_eq!(draft.when, Some(WhenSpec::NaiveDate(selected)));
        assert_eq!(draft.highlights, vec![(0..4, HighlightKind::Title)]);

        for input in ["Trip at noon", "Trip noon", "Trip 12:00", "Trip at 12pm"] {
            let draft = parse_on_date(input, selected);
            let Some(WhenSpec::DateTime(datetime)) = draft.when else {
                panic!("expected a datetime for {input:?}");
            };
            assert_eq!(draft.title, "Trip");
            assert_eq!(
                datetime.with_timezone(&Local).naive_local(),
                selected.and_hms_opt(12, 0, 0).unwrap()
            );
            assert_eq!(
                draft.highlights,
                vec![
                    (5..input.len(), HighlightKind::When),
                    (0..4, HighlightKind::Title)
                ]
            );
        }
    }

    let draft =
        parse_with_context("Trip", EntityKind::Marker, now(), Duration::minutes(5)).unwrap();
    assert_eq!(draft.when, None);
    let draft = parse_with_context(
        "Trip at noon",
        EntityKind::Marker,
        now(),
        Duration::minutes(5),
    )
    .unwrap();
    assert_eq!(draft.when, Some(WhenSpec::DateTime(now().to_utc())));
}

#[test]
fn weekday_recurrence_keeps_a_matching_selected_date() {
    for selected in selected_dates() {
        for input in ["Trip every Monday", "Trip every Monday at noon"] {
            let draft = parse_on_date(input, selected);
            assert_eq!(draft.title, "Trip");
            assert_eq!(
                draft.recurrence,
                Some(RecurrenceSpec::OnWeekdays(WeekdaySet::new([Weekday::Mon])))
            );
            match draft.when.unwrap() {
                WhenSpec::NaiveDate(date) => {
                    assert_eq!(date, selected);
                    assert!(
                        !draft
                            .highlights
                            .iter()
                            .any(|(_, kind)| *kind == HighlightKind::When)
                    );
                }
                WhenSpec::DateTime(datetime) => {
                    assert_eq!(
                        datetime.with_timezone(&Local).naive_local(),
                        selected.and_hms_opt(12, 0, 0).unwrap()
                    );
                }
            }
        }
    }

    let draft = parse_with_context(
        "Trip every Tuesday",
        EntityKind::Marker,
        now(),
        Duration::minutes(5),
    )
    .unwrap();
    assert_eq!(
        draft.when,
        Some(WhenSpec::NaiveDate(
            NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()
        ))
    );
}

#[test]
fn explicit_dates_override_selection_and_remain_relative_to_now() {
    let today = now().date_naive();
    let absolute = NaiveDate::from_ymd_opt(2027, 1, 15).unwrap();
    for selected in selected_dates() {
        for (input, expected) in [
            ("Trip today", today),
            ("Trip tomorrow", today + Duration::days(1)),
            ("Trip in 2 days", today + Duration::days(2)),
            ("Trip 2027-01-15", absolute),
        ] {
            let draft = parse_on_date(input, selected);
            assert_eq!(draft.title, "Trip");
            assert_eq!(draft.when, Some(WhenSpec::NaiveDate(expected)), "{input}");
        }
        for (input, expected) in [
            ("Trip today at 12:00", today),
            ("Trip 12:00 tomorrow", today + Duration::days(1)),
            ("Trip at 12pm tomorrow", today + Duration::days(1)),
            ("Trip at noon tomorrow", today + Duration::days(1)),
            ("Trip tomorrow at noon", today + Duration::days(1)),
            ("Trip in 2 days at 12pm", today + Duration::days(2)),
            ("Trip in 2 weeks at midday", today + Duration::weeks(2)),
            ("Trip next week noon", today + Duration::weeks(1)),
            ("Trip 2027-01-15 at 12:00", absolute),
            ("Trip 2027-01-15 at noon", absolute),
        ] {
            let draft = parse_on_date(input, selected);
            let Some(WhenSpec::DateTime(datetime)) = draft.when else {
                panic!("expected a datetime for {input:?}");
            };
            assert_eq!(draft.title, "Trip");
            assert_eq!(
                datetime.with_timezone(&Local).naive_local(),
                expected.and_hms_opt(12, 0, 0).unwrap(),
                "{input}"
            );
            assert_eq!(
                draft.highlights,
                vec![
                    (5..input.len(), HighlightKind::When),
                    (0..4, HighlightKind::Title)
                ]
            );
        }
    }
}

#[test]
fn relative_times_ignore_selection() {
    for selected in selected_dates() {
        let draft = parse_on_date("Trip now", selected);
        assert_eq!(draft.when, Some(WhenSpec::DateTime(now().to_utc())));
        let draft = parse_on_date("Trip in 2 hours", selected);
        assert_eq!(
            draft.when,
            Some(WhenSpec::DateTime((now() + Duration::hours(2)).to_utc()))
        );

        for input in ["Trip tonight", "Trip this evening"] {
            let draft = parse_on_date(input, selected);
            let Some(WhenSpec::DateTime(datetime)) = draft.when else {
                panic!("expected a datetime for {input:?}");
            };
            assert_eq!(
                datetime.with_timezone(&Local).naive_local(),
                now().date_naive().and_hms_opt(20, 0, 0).unwrap()
            );
        }
    }
}

#[test]
fn selected_date_does_not_mask_invalid_dates_or_error_input() {
    for input in [
        "Trip 2026-02-30",
        "Trip at noon 2026-02-30",
        "Trip daily until 2026-02-30",
    ] {
        let error = parse_marker_on_date(input, selected_dates()[0]).unwrap_err();
        assert!(matches!(error, ParseError::InvalidDate { input: raw, .. } if raw == input));
    }
}

#[test]
fn selected_date_is_used_for_duration_end_date_validation() {
    let input = "Trip for 1 day";
    let error = parse_tokens(
        input,
        EntityKind::Marker,
        now(),
        Duration::minutes(5),
        Some(NaiveDate::MAX),
    )
    .unwrap_err();
    assert!(matches!(error, ParseError::InvalidDuration { input: raw, .. } if raw == input));
    assert!(parse_with_context(input, EntityKind::Marker, now(), Duration::minutes(5)).is_ok());
}

#[test]
fn contextual_api_preserves_dates_and_explicit_times() {
    for kind in [
        EntityKind::Action,
        EntityKind::Event,
        EntityKind::Signal,
        EntityKind::Marker,
    ] {
        for selected in selected_dates() {
            let draft = crate::parse_on_date("Trip", kind.clone(), selected).unwrap();
            assert_eq!(draft.kind, kind);
            assert_eq!(draft.title, "Trip");
            assert_eq!(draft.when, Some(WhenSpec::NaiveDate(selected)));
            assert_eq!(draft.highlights, vec![(0..4, HighlightKind::Title)]);

            let draft = crate::parse_on_date("Trip at 3pm", kind.clone(), selected).unwrap();
            assert_eq!(draft.title, "Trip");
            assert_eq!(
                draft
                    .when
                    .unwrap()
                    .unwrap_datetime()
                    .with_timezone(&Local)
                    .naive_local(),
                selected.and_hms_opt(15, 0, 0).unwrap()
            );
            assert_eq!(
                draft.highlights,
                vec![(5..11, HighlightKind::When), (0..4, HighlightKind::Title)]
            );
        }
    }

    for kind in [EntityKind::ActionTemplate, EntityKind::EventTemplate] {
        let draft = crate::parse_on_date("Trip at 3pm", kind.clone(), selected_dates()[0]).unwrap();
        assert_eq!(draft.when, None);
        assert_eq!(
            draft.naive_time,
            (kind == EntityKind::ActionTemplate)
                .then(|| chrono::NaiveTime::from_hms_opt(15, 0, 0).unwrap())
        );
    }
}

#[test]
fn events_and_signals_require_explicit_times() {
    let today = now().date_naive();
    let monday = NaiveDate::from_ymd_opt(2026, 10, 19).unwrap();
    for (kind, target) in [
        (EntityKind::Event, BuildTarget::Event),
        (EntityKind::Signal, BuildTarget::Signal),
    ] {
        let parse =
            |input| parse_with_context(input, kind.clone(), now(), Duration::minutes(5)).unwrap();
        for (input, expected) in [
            ("Trip tomorrow for 1h", today + Duration::days(1)),
            ("Trip in 2 days for 1h", today + Duration::days(2)),
            ("Trip in 2 weeks for 1h", today + Duration::weeks(2)),
            ("Trip every Monday for 1h", monday),
            ("Trip tomorrow every Monday for 1h", monday),
        ] {
            let draft = parse(input);
            assert_eq!(draft.title, "Trip");
            assert_eq!(draft.when, Some(WhenSpec::NaiveDate(expected)), "{input}");
            assert!(matches!(
                build_entity(&draft, target),
                Err(ParserBuildError::InvalidTime(reason)) if reason == "include a time of day"
            ));
        }
        assert!(matches!(
            build_entity(&parse("Trip for 1h"), target),
            Err(ParserBuildError::MissingTime)
        ));

        for (input, date, hour) in [
            (
                "Trip tomorrow at midnight for 1h",
                today + Duration::days(1),
                0,
            ),
            (
                "Trip in 2 days at noon for 1h",
                today + Duration::days(2),
                12,
            ),
            ("Trip every Monday at 3pm for 1h", monday, 15),
        ] {
            let draft = parse(input);
            assert_eq!(draft.title, "Trip");
            let datetime = match build_entity(&draft, target).unwrap() {
                BuiltEntity::Event(event) => event.start,
                BuiltEntity::Signal(signal) => signal.datetime,
                _ => panic!("expected an event or signal"),
            };
            assert_eq!(
                datetime.with_timezone(&Local).naive_local(),
                date.and_hms_opt(hour, 0, 0).unwrap(),
                "{input}"
            );
        }
    }
}
