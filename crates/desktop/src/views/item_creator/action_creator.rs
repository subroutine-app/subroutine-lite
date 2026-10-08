use gpui::{AnyElement, Context, ParentElement};

use crate::{
    AppIcon,
    views::item_creator::{
        ItemCreator, OptionsUi,
        chip::{CreatorChip, TOGGLE_CHIP_WIDTH},
        chip_row,
        draft::{format_date, format_duration, format_recurrence, format_time},
        options_frame,
    },
};

impl ItemCreator {
    pub(super) fn action_options(&mut self, ui: OptionsUi, cx: &mut Context<Self>) -> AnyElement {
        let can_pin = self.draft.schedule.has_time();

        options_frame(
            chip_row()
                .child(self.date_chip(ui, 0, cx))
                .child(self.time_chip(ui, 1, cx))
                .child(self.duration_chip(ui, 2, cx))
                .child(self.repeat_chip(ui, 3, cx)),
            chip_row()
                .child(
                    CreatorChip::new("creator-queued")
                        .width(TOGGLE_CHIP_WIDTH)
                        .icon(AppIcon::Play)
                        .label("Queue")
                        .selected(self.draft.queued)
                        .disabled(self.queue_required)
                        .reveal(ui.at(4))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.draft.toggle_queued();
                            cx.notify();
                        })),
                )
                .child(
                    CreatorChip::new("creator-pinned")
                        .width(TOGGLE_CHIP_WIDTH)
                        .icon(AppIcon::MapPin)
                        .label("Pin")
                        .selected(self.draft.pinned && can_pin)
                        .disabled(!can_pin)
                        .reveal(ui.at(5))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.draft.toggle_pinned();
                            cx.notify();
                        })),
                )
                .child(self.recurrence_end_chip(ui, 6, cx))
                .child(self.recurrence_count_chip(ui, 7, cx)),
        )
    }

    pub(super) fn action_summary(&self) -> String {
        let schedule = &self.draft.schedule;
        let mut summary = match (self.draft.queued, schedule.date, schedule.time) {
            (true, Some(date), Some(time)) => {
                format!("Queued · {} · {}", format_date(date), format_time(time))
            }
            (true, Some(date), None) => format!("Queued · {}", format_date(date)),
            (true, None, _) => "Queued".to_string(),
            (false, Some(date), Some(time)) => {
                format!("Unqueued · {} · {}", format_date(date), format_time(time))
            }
            (false, Some(date), None) => {
                format!("Unqueued · {}", format_date(date))
            }
            (false, None, _) => "Unqueued".to_string(),
        };
        if self.draft.pinned && schedule.has_time() {
            summary.push_str(" · pinned");
        }
        if let Some(duration) = self.draft.duration {
            summary.push_str(&format!(" · {}", format_duration(duration)));
        }
        if let Some(recurrence) = self.draft.recurrence.as_ref() {
            summary.push_str(&format!(" · repeats {}", format_recurrence(recurrence)));
        }
        summary
    }
}
