use std::{collections::HashMap, ops::Range};

use chrono::{Datelike, NaiveDate};
use gpui::{FontWeight, Pixels, SharedString, Window, px};

use super::{
    agenda::{Agenda, UpcomingRow},
    sections::UpcomingSections,
};

pub(super) const HEADER_PADDING: Pixels = px(5.);
pub(super) const HEADER_GAP: Pixels = px(8.);
pub(super) const CHEVRON_WIDTH: Pixels = px(28.);
pub(super) const MARKER_HEIGHT: Pixels = px(28.);
pub(super) const MARKER_GAP: Pixels = px(4.);
pub(super) const CONTROLS_HEIGHT: Pixels = px(38.);
const DATE_WIDTH_STEP: f32 = 16.;

#[derive(Default)]
pub(super) struct DayHeaders {
    pub days: HashMap<NaiveDate, DayHeader>,
    pub add_width: Pixels,
    pub side_width: Pixels,
}

pub(super) struct DayHeader {
    pub label: String,

    pub date_width: Pixels,
    pub marker_widths: Vec<Pixels>,
    pub marker_rows: Vec<Range<usize>>,
}

impl DayHeaders {
    pub fn measure(
        agenda: &Agenda,
        sections: &UpcomingSections,
        today: NaiveDate,
        width: Pixels,
        window: &Window,
    ) -> Self {
        let content_width = (width - px(32.)).max(px(0.));
        let add_width =
            text_width("Add item", 0.875, FontWeight::NORMAL, window) + px(24. + 16. + 4.);
        let side_width = ((content_width - add_width - HEADER_GAP * 2.) / 2.).max(px(0.));
        let mut days = HashMap::new();
        for row in &agenda.rows {
            let UpcomingRow::Day {
                date,
                relative,
                markers,
            } = row
            else {
                continue;
            };
            let label = day_label(*date, today, *relative);
            let weekday_width = text_width(
                &date.format("%a").to_string(),
                0.75,
                FontWeight::NORMAL,
                window,
            );
            let date_width = quantized_date_width(
                text_width(&label, 0.875, FontWeight::BOLD, window)
                    + weekday_width
                    + px(16. + 8.),
            );
            let (date_width, marker_width) = if !sections.add_in_header(agenda, *date) {
                (
                    date_width,
                    (content_width - date_width - HEADER_GAP).max(px(1.)),
                )
            } else {
                (date_width.min(side_width), side_width)
            };
            let marker_width = (marker_width - CHEVRON_WIDTH - HEADER_GAP).max(px(1.));

            let marker_widths: Vec<_> = markers
                .iter()
                .map(|entry| {
                    (text_width(&entry.marker.title, 0.75, FontWeight::NORMAL, window) + px(20.))
                        .clamp(px(80.), px(200.))
                        .min(marker_width)
                })
                .collect();
            let marker_rows = wrap_markers(&marker_widths, marker_width);
            days.insert(
                *date,
                DayHeader {
                    label,

                    date_width,
                    marker_widths,
                    marker_rows,
                },
            );
        }
        Self {
            days,
            add_width,
            side_width,
        }
    }

    pub fn height(&self, date: NaiveDate) -> Pixels {
        let rows = self
            .days
            .get(&date)
            .map_or(0, |day| day.marker_rows.len())
            .max(1);

        MARKER_HEIGHT * rows as f32 + MARKER_GAP * (rows - 1) as f32 + HEADER_PADDING * 2.
    }
}

fn quantized_date_width(content_width: Pixels) -> Pixels {
    px((content_width.as_f32() / DATE_WIDTH_STEP).ceil() * DATE_WIDTH_STEP)
}

fn day_label(date: NaiveDate, today: NaiveDate, relative: Option<&str>) -> String {
    let months = (date.year() - today.year()) * 12 + date.month() as i32 - today.month() as i32;
    match relative {
        Some(relative) => relative.to_owned(),
        None if months.abs() < 12 => date.format("%B %-d").to_string(),
        None => date.format("%B %-d, %Y").to_string(),
    }
}

fn text_width(text: &str, rems: f32, weight: FontWeight, window: &Window) -> Pixels {
    let text = SharedString::from(text.replace(['\n', '\r'], " "));
    let mut style = window.text_style();
    style.font_weight = weight;
    let run = style.to_run(text.len());
    px(window
        .text_system()
        .shape_line(text, window.rem_size() * rems, &[run], None)
        .width
        .as_f32()
        .ceil())
}

fn wrap_markers(widths: &[Pixels], available: Pixels) -> Vec<Range<usize>> {
    let mut rows = Vec::new();
    let mut start = 0;
    let mut used = px(0.);
    for (index, width) in widths.iter().enumerate() {
        let gap = if index == start { px(0.) } else { MARKER_GAP };
        if index > start && used + gap + *width > available {
            rows.push(start..index);
            start = index;
            used = *width;
        } else {
            used += gap + *width;
        }
    }
    if start < widths.len() {
        rows.push(start..widths.len());
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_width_rounds_up_to_the_smallest_16_pixel_step() {
        for (content, expected) in [(127.5, 128.), (128., 128.), (128.5, 144.), (200., 208.)] {
            assert_eq!(quantized_date_width(px(content)), px(expected));
        }
    }

    #[test]
    fn wrapping_accounts_for_gaps_without_losing_markers() {
        assert_eq!(wrap_markers(&[], px(164.)), Vec::<Range<usize>>::new());
        assert_eq!(wrap_markers(&[px(80.); 3], px(164.)), [0..2, 2..3]);
        assert_eq!(wrap_markers(&[px(80.); 3], px(163.)), [0..1, 1..2, 2..3]);
    }

    #[test]
    fn year_labels_follow_calendar_month_distance() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 31).unwrap();
        for (date, expected) in [
            ((2027, 1, 1), "January 1"),
            ((2027, 9, 30), "September 30"),
            ((2027, 10, 1), "October 1, 2027"),
            ((2025, 10, 31), "October 31, 2025"),
        ] {
            let date = NaiveDate::from_ymd_opt(date.0, date.1, date.2).unwrap();
            assert_eq!(day_label(date, today, None), expected);
        }
        assert_eq!(day_label(today, today, Some("Today")), "Today");
    }
}
