use crate::components::Button;
use crate::components::ButtonVariants;
use crate::components::Label;
use crate::icons::Icon;
use chrono::{Duration, NaiveTime};
use chronoutil::RelativeDuration;
use gpui::{
    AnyElement, App, Context, ElementId, FontWeight, IntoElement, ParentElement, Pixels, Styled,
    Window, div, px,
};
use gpui_kit::foundation::StyledExt as _;
use gpui_kit_theme::ActiveTheme;
use subroutine_core::RoutineStep;
use uuid::Uuid;

use crate::{
    AppIcon,
    components::{DynamicListDelegate, DynamicListState, apply_reorder},
    views::item_creator::{
        ItemCreator, OptionsUi,
        chip::{COMPACT_CHIP_WIDTH, CreatorChip},
        chip_row,
        draft::{format_duration, format_recurrence, format_time},
        options_frame, summary_text,
    },
};

pub const STEP_HEIGHT: Pixels = px(40.);
pub const STEP_GAP: Pixels = px(6.);
pub const MAX_VISIBLE_STEPS: usize = 5;
pub const STEP_LIST_PADDING: Pixels = px(8.);
const EMPTY_STEPS_HEIGHT: Pixels = px(76.);

const STEP_DURATION_STEP_MINUTES: i64 = 5;

#[derive(Clone)]
pub struct StepEntry {
    pub id: Uuid,
    pub title: String,
    pub duration: Option<Duration>,
}

impl StepEntry {
    pub fn new(title: impl Into<String>, duration: Option<Duration>) -> Self {
        Self {
            id: Uuid::now_v7(),
            title: title.into(),
            duration,
        }
    }

    fn to_step(&self) -> RoutineStep {
        let step = RoutineStep::new(self.title.clone());
        match self.duration {
            Some(duration) => step.with_duration(RelativeDuration::from(duration)),
            None => step,
        }
    }
}

#[derive(Clone)]
pub struct StepDelegate {
    pub steps: Vec<StepEntry>,
}

impl StepDelegate {
    pub fn new() -> Self {
        Self { steps: Vec::new() }
    }

    pub fn push(&mut self, entry: StepEntry) {
        self.steps.push(entry);
    }

    pub fn remove(&mut self, id: Uuid) {
        self.steps.retain(|entry| entry.id != id);
    }

    pub fn clear(&mut self) {
        self.steps.clear();
    }

    fn step_duration(&mut self, id: Uuid, steps: i64) {
        let Some(entry) = self.steps.iter_mut().find(|entry| entry.id == id) else {
            return;
        };
        let minutes = entry.duration.unwrap_or_else(Duration::zero).num_minutes()
            + steps * STEP_DURATION_STEP_MINUTES;
        entry.duration = (minutes > 0).then(|| Duration::minutes(minutes));
    }

    pub fn to_steps(&self) -> Vec<RoutineStep> {
        self.steps.iter().map(StepEntry::to_step).collect()
    }

    pub fn total_duration(&self) -> Option<Duration> {
        let total = self
            .steps
            .iter()
            .filter_map(|entry| entry.duration)
            .fold(Duration::zero(), |acc, duration| acc + duration);
        (!total.is_zero()).then_some(total)
    }
}

impl DynamicListDelegate for StepDelegate {
    type Item = AnyElement;

    fn items_count(&self, _cx: &App) -> usize {
        self.steps.len()
    }

    fn item_id(&self, ix: usize, _cx: &App) -> ElementId {
        match self.steps.get(ix) {
            Some(entry) => ElementId::Uuid(entry.id),
            None => ElementId::Integer(ix as u64),
        }
    }

    fn item_height(&self, _ix: usize, _cx: &App) -> Pixels {
        STEP_HEIGHT
    }

    fn move_item(
        &mut self,
        from: usize,
        to: usize,
        _window: &mut Window,
        _cx: &mut Context<DynamicListState<Self>>,
    ) {
        apply_reorder(&mut self.steps, from, to);
    }

    fn render_item(
        &mut self,
        ix: usize,
        _window: &mut Window,
        cx: &mut Context<DynamicListState<Self>>,
    ) -> Option<Self::Item> {
        let entry = self.steps.get(ix)?.clone();
        let id = entry.id;
        let key = id.as_u64_pair().1;

        Some(
            div()
                .row()
                .size_full()
                .pl_1p5()
                .pr_1p5()
                .gap_2()
                .rounded(px(cx.theme().radii.control))
                .border_1()
                .border_color(cx.theme().colors.hairline.alpha(0.5))
                .bg(cx.theme().colors.raised.alpha(0.6))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size_6()
                        .flex_none()
                        .rounded_full()
                        .text_color(cx.theme().colors.text_muted)
                        .bg(cx.theme().colors.raised)
                        .child(
                            Label::new(format!("{}", ix + 1))
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD),
                        ),
                )
                .child(
                    Label::new(entry.title.clone())
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_sm(),
                )
                .child(
                    CreatorChip::new(("step-duration", key))
                        .width(COMPACT_CHIP_WIDTH)
                        .icon(AppIcon::Clock)
                        .value(
                            entry
                                .duration
                                .map(format_duration)
                                .unwrap_or_else(|| "—".into()),
                        )
                        .active(entry.duration.is_some())
                        .stepper(
                            cx.listener(move |list, _, _window, cx| {
                                list.update_items(cx, |delegate, _| delegate.step_duration(id, -1));
                            }),
                            cx.listener(move |list, _, _window, cx| {
                                list.update_items(cx, |delegate, _| delegate.step_duration(id, 1));
                            }),
                        ),
                )
                .child(
                    Button::new(("step-remove", key))
                        .ghost()
                        .size_6()
                        .child(Icon::new(AppIcon::Close).size_3())
                        .on_click(cx.listener(move |list, _, _window, cx| {
                            list.update_items(cx, |delegate, _| delegate.remove(id));
                        })),
                )
                .into_any_element(),
        )
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<DynamicListState<Self>>,
    ) -> impl IntoElement {
        div()
            .column()
            .size_full()
            .items_center()
            .justify_center()
            .gap_1()
            .text_color(cx.theme().colors.text_muted)
            .child(Icon::new(AppIcon::ListOrdered).size_4())
            .child(Label::new("No steps").text_xs())
    }
}

pub fn steps_area_height(count: usize) -> Pixels {
    if count == 0 {
        return EMPTY_STEPS_HEIGHT;
    }
    let visible = count.min(MAX_VISIBLE_STEPS);
    STEP_HEIGHT * visible as f32
        + STEP_GAP * visible.saturating_sub(1) as f32
        + STEP_LIST_PADDING * 2.
}

pub fn steps_summary(delegate: &StepDelegate, target: Option<NaiveTime>) -> String {
    let mut summary = match delegate.steps.len() {
        0 => "No steps".to_string(),
        1 => "1 step".to_string(),
        n => format!("{n} steps"),
    };
    if let Some(total) = delegate.total_duration() {
        summary.push_str(&format!(" · {}", format_duration(total)));
    }
    if let Some(time) = target {
        summary.push_str(&format!(" · starts {}", format_time(time)));
    }
    summary
}

impl ItemCreator {
    pub(super) fn routine_options(
        &mut self,
        ui: OptionsUi,
        cx: &mut Context<ItemCreator>,
    ) -> AnyElement {
        let summary = steps_summary(self.step_list.read(cx).delegate(), None);
        options_frame(
            chip_row()
                .child(self.date_chip(ui, 0, cx))
                .child(self.time_chip(ui, 1, cx))
                .child(self.repeat_chip(ui, 2, cx)),
            chip_row()
                .child(self.recurrence_end_chip(ui, 3, cx))
                .child(self.recurrence_count_chip(ui, 4, cx))
                .child(summary_text(summary, ui.at(5), cx)),
        )
    }

    pub(super) fn routine_summary(&self, cx: &App) -> String {
        let delegate = self.step_list.read(cx).delegate();
        let mut summary = steps_summary(delegate, self.draft.schedule.time);
        if let Some(recurrence) = self.draft.recurrence.as_ref() {
            summary.push_str(&format!(" · repeats {}", format_recurrence(recurrence)));
        }
        summary
    }
}
