use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::{America::New_York, Asia::Tokyo, Tz};
use parser::{
    EntityKind, HighlightKind, ParseDraft, ParseError, WhenSpec, parse_in_timezone,
    recurrence_to_rule_in_timezone,
};

fn parse(
    text: &str,
    now: &str,
    timezone: Tz,
    date: Option<NaiveDate>,
) -> Result<ParseDraft, ParseError> {
    parse_in_timezone(
        text,
        EntityKind::Action,
        DateTime::parse_from_rfc3339(now)
            .unwrap()
            .with_timezone(&timezone),
        Duration::minutes(5),
        date,
    )
}

fn date(value: &str) -> NaiveDate {
    value.parse().unwrap()
}

fn instant(value: &str) -> Option<WhenSpec> {
    Some(WhenSpec::DateTime(
        DateTime::parse_from_rfc3339(value).unwrap().to_utc(),
    ))
}

#[test]
fn explicit_timezone_controls_calendar_default_date_and_relative_phrases() {
    let now = "2026-01-31T02:02:00Z";
    let selected = Some(date("2030-06-05"));
    for (text, expected) in [
        ("Read today", Some(WhenSpec::NaiveDate(date("2026-01-30")))),
        (
            "Read tomorrow",
            Some(WhenSpec::NaiveDate(date("2026-01-31"))),
        ),
        ("Read at 9am", instant("2030-06-05T13:00:00Z")),
        ("Read 9am tomorrow", instant("2026-01-31T14:00:00Z")),
        ("Read tomorrow at 9am", instant("2026-01-31T14:00:00Z")),
        ("Read now", instant("2026-01-31T02:05:00Z")),
        ("Read in 2 hours", instant("2026-01-31T04:02:00Z")),
        (
            "Read in 1 month",
            Some(WhenSpec::NaiveDate(date("2026-02-28"))),
        ),
        ("Read in 1 month at 9am", instant("2026-02-28T14:00:00Z")),
        ("Read at 9am in 2 months", instant("2026-03-30T13:00:00Z")),
        (
            "Read in 1 year",
            Some(WhenSpec::NaiveDate(date("2027-01-30"))),
        ),
        ("Read March 5 at noon", instant("2026-03-05T17:00:00Z")),
        ("Read 2027-07-01T05:00:00Z", instant("2027-07-01T05:00:00Z")),
    ] {
        let parsed = parse(text, now, New_York, selected).unwrap();
        assert_eq!(parsed.title, "Read", "{text}");
        assert_eq!(parsed.when, expected, "{text}");
    }
    assert_eq!(
        parse("Read today", now, Tokyo, selected).unwrap().when,
        Some(WhenSpec::NaiveDate(date("2026-01-31")))
    );
    let implicit = parse("Read", now, New_York, selected).unwrap();
    assert_eq!(implicit.when, selected.map(WhenSpec::NaiveDate));
    assert!(
        implicit
            .highlights
            .iter()
            .all(|(_, kind)| *kind == HighlightKind::Title)
    );
}

#[test]
fn dst_gaps_fail_and_folds_choose_the_earlier_instant() {
    let now = "2026-03-01T00:00:00Z";
    for text in ["Read 2026-03-08 at 2:30am", "Read at 2:30am"] {
        let error = parse(text, now, New_York, Some(date("2026-03-08"))).unwrap_err();
        assert!(
            matches!(error, ParseError::InvalidDate { reason, .. } if reason.contains("does not exist"))
        );
    }
    assert_eq!(
        parse("Read 2026-11-01 at 1:30am", now, New_York, None)
            .unwrap()
            .when,
        instant("2026-11-01T05:30:00Z")
    );
    assert_eq!(
        parse("Read in 2 hours", "2026-03-08T06:30:00Z", New_York, None)
            .unwrap()
            .when,
        instant("2026-03-08T08:30:00Z")
    );
    assert!(
        parse(
            "Read every Sunday at 2:30am",
            now,
            New_York,
            Some(date("2026-03-07"))
        )
        .is_err()
    );
}

#[test]
fn weekday_alignment_and_recurrence_keep_the_requested_timezone_across_dst() {
    let draft = parse(
        "Read every Monday at 9am until March 31",
        "2026-03-07T23:00:00Z",
        New_York,
        None,
    )
    .unwrap();
    assert_eq!(draft.when, instant("2026-03-09T13:00:00Z"));
    assert_eq!(draft.recurrence_end_date, Some(date("2026-03-31")));
    let recurrence = recurrence_to_rule_in_timezone(draft.recurrence.as_ref(), New_York)
        .unwrap()
        .unwrap();
    assert_eq!(recurrence.timezone, Some(New_York));
    let before = Utc.with_ymd_and_hms(2026, 3, 2, 14, 0, 0).unwrap();
    let next = recurrence.next(before).unwrap();
    assert_eq!(
        next,
        subroutine_core::SchedulePoint::DateTime(
            Utc.with_ymd_and_hms(2026, 3, 9, 13, 0, 0).unwrap()
        )
    );
    let selected = date("2030-06-03");
    let draft = parse(
        "Read every Monday",
        "2026-03-07T23:00:00Z",
        Tokyo,
        Some(selected),
    )
    .unwrap();
    assert_eq!(draft.when, Some(WhenSpec::NaiveDate(selected)));
}

#[test]
fn unicode_titles_keep_adjacent_codepoints_and_original_byte_ranges() {
    let input = "🧑🏽‍💻 café 東京 e\u{301} tomorrow for 15m";
    let draft = parse(input, "2026-10-15T12:00:00Z", Tokyo, None).unwrap();
    assert_eq!(draft.title, "🧑🏽‍💻 café 東京 e\u{301}");
    for (range, _) in draft.highlights {
        assert!(input.get(range).is_some());
    }
}

#[test]
fn extreme_calendar_arithmetic_returns_errors_not_panics() {
    for input in [
        "Read in 18446744073709551615 months",
        "Read in 18446744073709551615 years",
        "Read in 18446744073709551615 days",
        "Read every 0 days",
        "Read every 4294967296 years",
    ] {
        assert!(parse(input, "2026-10-15T12:00:00Z", Tokyo, None).is_err());
    }
    let result = parse_in_timezone(
        "Read today",
        EntityKind::Action,
        DateTime::<Utc>::MAX_UTC.with_timezone(&Tokyo),
        Duration::minutes(5),
        None,
    );
    assert!(result.is_err());
}
