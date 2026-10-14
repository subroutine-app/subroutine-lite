use chrono::{Duration, Local};
use parser::{ParseDraft, WhenSpec};
use subroutine_core::{Marker, Recurrence};

pub(super) fn apply(
    original: &Marker,
    draft: &ParseDraft,
    recurrence: Option<Recurrence>,
) -> Result<Marker, &'static str> {
    let date = match draft.when {
        Some(WhenSpec::NaiveDate(date)) => date,
        Some(WhenSpec::DateTime(time)) => time.with_timezone(&Local).date_naive(),
        None => original.date,
    };
    let span = match draft.duration {
        Some(duration) => {
            if duration < Duration::days(1) {
                return Err("A marker needs at least one day");
            }
            duration
                .checked_sub(&Duration::days(1))
                .ok_or("The marker span is out of range")?
        }
        None => original
            .end_date
            .map(|end| end - original.date)
            .unwrap_or_default(),
    };
    let end = date
        .checked_add_signed(span)
        .ok_or("The end date is outside the supported calendar range")?;
    if end < date {
        return Err("The end date must not be before the start date");
    }

    let mut marker = original.clone();
    marker.title = draft.title.clone();
    marker.date = date;
    marker.end_date = (end > date).then_some(end);
    if let Some(recurrence) = recurrence {
        marker.recurrence = Some(recurrence);
    }
    if let Some(content) = &draft.content {
        marker.content = Some(content.clone());
    }
    Ok(marker)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn edited(marker: &Marker, text: &str) -> Marker {
        let draft = parser::parse_marker_on_date(text, marker.date).unwrap();
        let recurrence = parser::recurrence_to_rule(draft.recurrence.as_ref()).unwrap();
        apply(marker, &draft, recurrence).unwrap()
    }

    #[test]
    fn title_edits_keep_the_selected_day_or_range() {
        let date = NaiveDate::from_ymd_opt(2030, 11, 20).unwrap();
        for days in [1, 4] {
            let mut marker = Marker::new("", date).with_content("Keep these notes");
            marker.end_date = (days > 1).then(|| date + Duration::days(days - 1));
            for text in ["Conference", "Conference at noon", "Conference at 9am"] {
                let updated = edited(&marker, text);
                assert_eq!(updated.id, marker.id);
                assert_eq!(updated.title, "Conference");
                assert_eq!(updated.date, date);
                assert_eq!(updated.end_date, marker.end_date);
                assert_eq!(updated.content, marker.content);
            }
        }
    }

    #[test]
    fn explicit_dates_move_the_span_and_removing_clauses_restores_the_selection() {
        let first = NaiveDate::from_ymd_opt(2030, 11, 20).unwrap();
        let marker = Marker::new("", first).with_end_date(first + Duration::days(3));
        let moved = NaiveDate::from_ymd_opt(2030, 12, 5).unwrap();
        for (text, date, span) in [
            ("Conference 2030-12-05", moved, 3),
            ("Conference 2030-12-05 for 2 days", moved, 1),
            ("Conference for 2 days", first, 1),
            ("Conference", first, 3),
        ] {
            let updated = edited(&marker, text);
            assert_eq!(updated.date, date);
            assert_eq!(updated.end_date, Some(date + Duration::days(span)));
        }
    }

    #[test]
    fn moving_a_span_beyond_the_calendar_range_is_rejected() {
        let date = NaiveDate::from_ymd_opt(2030, 11, 20).unwrap();
        let marker = Marker::new("", date).with_end_date(date + Duration::days(3));
        let mut draft = parser::parse_marker_on_date("Conference", date).unwrap();
        draft.when = Some(WhenSpec::NaiveDate(NaiveDate::MAX));
        assert!(apply(&marker, &draft, None).is_err());
    }
}
