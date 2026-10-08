use gpui::{AnyElement, Context, ParentElement};

use crate::views::item_creator::{
    ItemCreator, OptionsUi, chip_row,
    draft::{format_date, format_duration, format_recurrence, format_time},
    options_frame,
};

impl ItemCreator {
    pub(super) fn event_options(&mut self, ui: OptionsUi, cx: &mut Context<Self>) -> AnyElement {
        options_frame(
            chip_row()
                .child(self.date_chip(ui, 0, cx))
                .child(self.time_chip(ui, 1, cx))
                .child(self.duration_chip(ui, 2, cx))
                .child(self.repeat_chip(ui, 3, cx)),
            chip_row()
                .child(self.recurrence_end_chip(ui, 4, cx))
                .child(self.recurrence_count_chip(ui, 5, cx)),
        )
    }

    pub(super) fn event_summary(&self) -> String {
        let schedule = &self.draft.schedule;
        let (Some(date), Some(start)) = (schedule.date, schedule.time) else {
            return schedule.date.map(format_date).unwrap_or_default();
        };

        let mut summary = match self.draft.duration {
            Some(duration) => format!(
                "{}, {} – {} ({})",
                format_date(date),
                format_time(start),
                format_time(start + duration),
                format_duration(duration),
            ),
            None => format!("{} · {}", format_date(date), format_time(start)),
        };
        if let Some(recurrence) = self.draft.recurrence.as_ref() {
            summary.push_str(&format!(" · repeats {}", format_recurrence(recurrence)));
        }
        summary
    }
}
