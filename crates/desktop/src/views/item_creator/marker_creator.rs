use gpui::{AnyElement, Context, ParentElement};

use crate::{
    AppIcon,
    views::item_creator::{
        ItemCreator, OptionsUi,
        chip::CreatorChip,
        chip_row,
        draft::{Clause, format_date, format_recurrence},
        options_frame,
    },
};

impl ItemCreator {
    pub(super) fn marker_options(&mut self, ui: OptionsUi, cx: &mut Context<Self>) -> AnyElement {
        let span = self.draft.span_days;

        options_frame(
            chip_row()
                .child(self.date_chip(ui, 0, cx))
                .child(
                    CreatorChip::new("creator-span")
                        .icon(AppIcon::Calendars)
                        .value(match span {
                            1 => "1 day".to_string(),
                            n => format!("{n} days"),
                        })
                        .active(span > 1)
                        .reveal(ui.at(1))
                        .flash(ui.flash(Clause::Duration))
                        .stepper(
                            cx.listener(|this, _, window, cx| {
                                this.draft.step_span(-1);
                                this.claim(Clause::Duration, window, cx);
                                cx.notify();
                            }),
                            cx.listener(|this, _, window, cx| {
                                this.draft.step_span(1);
                                this.claim(Clause::Duration, window, cx);
                                cx.notify();
                            }),
                        ),
                )
                .child(self.repeat_chip(ui, 2, cx)),
            chip_row()
                .child(self.recurrence_end_chip(ui, 3, cx))
                .child(self.recurrence_count_chip(ui, 4, cx)),
        )
    }

    pub(super) fn marker_summary(&self) -> String {
        let Some(date) = self.draft.schedule.date else {
            return String::new();
        };

        let mut summary = match self.draft.marker_end_date() {
            Ok(Some(end)) => format!(
                "{} – {} ({} days)",
                format_date(date),
                format_date(end),
                self.draft.span_days
            ),
            Ok(None) => format_date(date),
            Err(error) => return error.to_owned(),
        };
        if let Some(recurrence) = self.draft.recurrence.as_ref() {
            summary.push_str(&format!(" · repeats {}", format_recurrence(recurrence)));
        }
        summary
    }
}
