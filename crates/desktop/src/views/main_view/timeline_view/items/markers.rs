use std::{cell::Cell, rc::Rc};

use chrono::{DateTime, Duration, Local, NaiveDate};
use gpui::{
    App, Bounds, Context, ElementId, InteractiveElement, MouseButton, ParentElement, Pixels,
    StatefulInteractiveElement as _, Styled, point, px, size,
};
use gpui_kit_theme::ActiveTheme as _;
use subroutine_core::Marker;

use crate::{
    components::{Button, ButtonVariants as _, Label, ext::ElementExt as _},
    item_manager::ItemManager,
    selection::SelectionManager,
};

use super::super::TimelineView;
use super::ANNOTATION_CHIP_HEIGHT;

pub(in crate::views::main_view::timeline_view) struct MarkerSummary {
    start: NaiveDate,
    end: NaiveDate,
    entries: Vec<(Marker, bool)>,
    with_date: bool,
}

impl MarkerSummary {
    pub(in crate::views::main_view::timeline_view) fn with_date(mut self) -> Self {
        self.with_date = true;
        self
    }

    fn label(&self, cx: &App) -> String {
        let title = (self.start == self.end).then(|| marker_title(&self.entries[0].0, cx));
        let label = summary_label(title.as_deref(), self.entries.len());
        if self.with_date {
            format!("{} · {label}", self.start.format("%a %-d"))
        } else {
            label
        }
    }

    fn description(&self, cx: &App) -> String {
        if self.start == self.end {
            format!(
                "{} · {}",
                self.label(cx),
                self.start.format("%a, %b %-d, %Y")
            )
        } else {
            format!(
                "{} stored markers · {} – {}",
                self.entries.len(),
                self.start.format("%b %-d, %Y"),
                self.end.format("%b %-d, %Y"),
            )
        }
    }
}

fn summary_label(title: Option<&str>, count: usize) -> String {
    match (title, count) {
        (Some(title), 1) => title.to_owned(),
        (Some(title), _) => format!("{title} · +{}", count - 1),
        (None, 1) => "1 marker".into(),
        (None, _) => format!("{count} markers"),
    }
}

fn marker_title(marker: &Marker, cx: &App) -> String {
    let manager = ItemManager::global(cx);
    let title = manager
        .read(cx)
        .editing_item
        .as_ref()
        .filter(|editing| editing.id() == marker.id)
        .map(|editing| {
            editing
                .parsed
                .as_ref()
                .ok()
                .and_then(Option::as_ref)
                .map_or_else(
                    || editing.value(cx).to_string(),
                    |parsed| parsed.title.clone(),
                )
        })
        .unwrap_or_else(|| marker.title.clone());
    if title.trim().is_empty() {
        "New marker".into()
    } else {
        title
    }
}

fn stored_markers<'a>(
    markers: impl Iterator<Item = &'a Marker>,
    start: NaiveDate,
    end: NaiveDate,
) -> Vec<(Marker, bool)> {
    markers
        .filter(|marker| marker.date <= end && marker.end_date.unwrap_or(marker.date) >= start)
        .cloned()
        .map(|marker| (marker, false))
        .collect()
}

impl TimelineView {
    pub(in crate::views::main_view::timeline_view) fn marker_summary(
        &self,
        start: DateTime<Local>,
        end: DateTime<Local>,
    ) -> Option<MarkerSummary> {
        if !self.loaded || end <= start {
            return None;
        }
        let start = start.date_naive();
        let end = (end - Duration::nanoseconds(1)).date_naive();
        let mut entries = if start == end {
            self.project_markers(start, end)
        } else {
            // Broad headings summarize saved items, not an unbounded recurrence forecast.
            stored_markers(self.known_markers(), start, end)
        };
        entries.sort_by_key(|(marker, projected)| {
            (*projected, marker.date, marker.lineage_id, marker.id)
        });
        (!entries.is_empty()).then_some(MarkerSummary {
            start,
            end,
            entries,
            with_date: false,
        })
    }

    pub(in crate::views::main_view::timeline_view) fn render_marker_summary(
        &self,
        summary: MarkerSummary,
        id: impl Into<ElementId>,
        max_width: Pixels,
        cx: &Context<Self>,
    ) -> Button {
        let anchor = Rc::new(Cell::new(None::<Bounds<Pixels>>));
        let measured = anchor.clone();
        let label = summary.label(cx);
        let description = summary.description(cx);
        Button::new(id)
            .ghost()
            .xsmall()
            .compact()
            .h(ANNOTATION_CHIP_HEIGHT)
            .max_w(max_width)
            .min_w_0()
            .px_1()
            .key_context("TimelineMarkers")
            .role(gpui::Role::Button)
            .aria_label(description.clone())
            .tooltip(description)
            .text_color(cx.theme().colors.text_muted)
            .block_mouse_except_scroll()
            .on_prepaint(move |bounds, _, _| measured.set(Some(bounds)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                SelectionManager::claim_press(cx);
                cx.stop_propagation();
            })
            .on_click(cx.listener(move |view, _, window, cx| {
                cx.stop_propagation();
                let (Some(mut anchor), Some(viewport)) = (anchor.get(), view.bounds) else {
                    return;
                };
                anchor.origin -= viewport.origin;
                if view.open_marker_panel(summary.entries.clone(), anchor, window, cx)
                    && let Some(panel) = &mut view.marker_panel
                {
                    panel.set_scope(summary.start, summary.end);
                }
            }))
            .child(Label::new(label).truncate().min_w_0())
    }

    pub(super) fn marker_anchor(&self, time: DateTime<Local>) -> Bounds<Pixels> {
        Bounds::new(
            point(
                self.item_area_left(),
                self.geometry.y(self.time_to_offset(time)),
            ),
            size(px(180.), ANNOTATION_CHIP_HEIGHT),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, month, day).unwrap()
    }

    #[test]
    fn broad_summaries_never_expand_into_marker_titles() {
        assert_eq!(summary_label(Some("Vacation"), 3), "Vacation · +2");
        assert_eq!(summary_label(Some("Vacation"), 1), "Vacation");
        assert_eq!(summary_label(None, 1), "1 marker");
        assert_eq!(summary_label(None, 250), "250 markers");
    }

    #[test]
    fn period_counts_include_complete_spans_and_each_marker_only_once() {
        let mut vacation = Marker::new("Vacation", date(9, 28));
        vacation.end_date = Some(date(10, 4));
        let markers = [
            vacation,
            Marker::new("First day", date(10, 1)),
            Marker::new("Last day", date(10, 31)),
            Marker::new("Next month", date(11, 1)),
        ];
        let month = stored_markers(markers.iter(), date(10, 1), date(10, 31));
        assert_eq!(month.len(), 3);
        assert_eq!(
            stored_markers(markers.iter(), date(10, 4), date(10, 4)).len(),
            1
        );
        assert!(stored_markers(markers.iter(), date(10, 5), date(10, 5)).is_empty());
    }
}
