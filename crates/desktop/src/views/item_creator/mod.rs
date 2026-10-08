use crate::components::transition::{self, WindowTransitionExt as _};
use crate::easing::ease_out_cubic;
use gpui_kit::foundation::StyledExt as _;
use gpui_kit::motion::{Interpolate, MotionSpec};
use gpui_kit::overlay::{GlassExt as _, GlassPreset};
use std::time::{Duration as StdDuration, Instant};

use crate::components::Button;
use crate::components::ButtonVariants;
use crate::components::Label;
use crate::components::elastic_overscroll::ElasticOverscroll;
use crate::components::ext::ElementExt;
use crate::components::timed_toast;
use crate::icons::Icon;
use crate::presentation::{RecognitionPaint, UxColor};
use chrono::{DateTime, Local, Utc};
use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, DefiniteLength, Div, ElementId, Entity,
    FocusHandle, Focusable, FontWeight, Hsla, Pixels, SharedString, Subscription, Window, actions,
    div, prelude::*, px, rems,
};
use gpui_kit::controls::editor::{Editor, EditorEvent};
use gpui_kit::controls::input::{Cancel as InputCancel, TextInput, TextInputEvent};
use gpui_kit::controls::toggle::Switch;
use gpui_kit::display::badge::Tone;
use gpui_kit::foundation::Disableable;
use gpui_kit::foundation::Selectable;
use gpui_kit::foundation::Sizable;
use gpui_kit::foundation::ThemeOverlay;
use gpui_kit_theme::ActiveTheme;
use gpui_kit_theme::{Surface, Theme};

use parser::ParseDraft;
use subroutine_core::{Action, AnyItem, ItemType, SchedulePoint};

mod action_creator;
mod chip;
mod draft;
mod event_creator;
mod marker_creator;
mod routine_creator;
mod signal_creator;

pub(crate) use chip::CreatorChip;
use chip::{CHIP_HEIGHT, stagger};
use draft::{Clause, ParseOutcome};
pub(crate) use draft::{ItemDraft, format_date, format_duration, format_recurrence, format_time};
use routine_creator::{STEP_GAP, STEP_LIST_PADDING, StepDelegate, StepEntry, steps_area_height};

use crate::{
    AppIcon,
    color::ColorExt as _,
    components::{
        CloseOverlay, DynamicList, DynamicListState, OverlayPosition, item_icon, overlay_with_scrim,
    },
    keys::key,
    settings::Settings,
    stores::AppDatabaseStore,
};

actions!(
    item_creator,
    [
        ToggleBatchMode,
        SubmitItem,
        NextType,
        PreviousType,
        SelectAction,
        SelectEvent,
        SelectRoutine,
        SelectMarker,
        SelectSignal,
    ]
);

pub const DEFAULT_CREATOR_MODE: ItemType = ItemType::Action;

pub(crate) type CreatorMode = ItemType;

const MODES: [ItemType; 5] = [
    ItemType::Action,
    ItemType::Event,
    ItemType::Routine,
    ItemType::Marker,
    ItemType::Signal,
];

const CREATOR_WIDTH: Pixels = px(160. * 4.);
const DETAILS_WIDTH: Pixels = px(720.);
const HEADER_HEIGHT: Pixels = px(56.);
const CREATOR_RADIUS: f32 = 28.;

pub(super) fn creator_secondary_text(theme: &Theme) -> Hsla {
    theme.colors.text_muted.mix(theme.colors.text, 0.7)
}

fn creator_input_theme(theme: &Theme) -> Theme {
    let placeholder = creator_secondary_text(theme);
    theme
        .clone()
        .modify(|theme| theme.colors.text_placeholder = placeholder)
}

const OPTIONS_SUMMARY_HEIGHT: Pixels = px(48.);
const OPTIONS_HEIGHT: Pixels = px(88.);
const DETAILS_NOTES_HEIGHT: Pixels = px(216.);
const FOOTER_HEIGHT: Pixels = px(52.);
const STEP_INPUT_HEIGHT: Pixels = px(48.);

const TYPE_BUTTON_LEFT: Pixels = px(12.);
const TYPE_BUTTON_SIZE: Pixels = px(36.);
const TYPE_TRIGGER_WIDTH: Pixels = px(36.);
const BATCH_STATUS_WIDTH: Pixels = px(116.);
const STRIP_ITEM: Pixels = px(120.);
const STRIP_WIDTH: Pixels = px(120. * 5.);

const FRAME_PADDING: Pixels = px(16.);
const CHIP_GAP: Pixels = px(6.);

const EXPAND: MotionSpec = MotionSpec::new(220, transition::EASE_OUT_CUBIC);
const STRIP: MotionSpec = MotionSpec::new(180, transition::EASE_OUT_CUBIC);
const BODY_CUE_MS: StdDuration = StdDuration::from_millis(420);
const CLAIM_FLASH_MS: StdDuration = StdDuration::from_millis(520);

pub fn init(cx: &mut App) {
    let context = Some("ItemCreator");
    cx.bind_keys([
        key("cmd-b", ToggleBatchMode, context),
        key("cmd-enter", SubmitItem, context),
        key("cmd-[", PreviousType, context),
        key("cmd-]", NextType, context),
        key("cmd-1", SelectAction, context),
        key("cmd-2", SelectEvent, context),
        key("cmd-3", SelectRoutine, context),
        key("cmd-4", SelectMarker, context),
        key("cmd-5", SelectSignal, context),
    ]);
}

struct Cue {
    started: Option<Instant>,
    duration: StdDuration,
}

impl Cue {
    fn new(duration: StdDuration) -> Self {
        Self {
            started: None,
            duration,
        }
    }

    fn trigger(&mut self) {
        self.started = Some(Instant::now());
    }

    fn progress(&self, window: &mut Window) -> f32 {
        let Some(started) = self.started else {
            return 1.0;
        };
        let t = started.elapsed().as_secs_f32() / self.duration.as_secs_f32();
        if t >= 1.0 {
            return 1.0;
        }
        window.request_animation_frame();
        ease_out_cubic(t)
    }
}

fn animate<T: Interpolate + PartialEq + 'static>(
    key: impl Into<ElementId>,
    target: T,
    spec: MotionSpec,
    window: &mut Window,
    cx: &mut App,
) -> T {
    let transition = window.keyed_transition(key, cx, spec, move || target);
    transition.set(target, cx);
    transition.animate(window, cx)
}

#[derive(Clone, Copy)]
pub(crate) struct OptionsUi {
    pub reveal: f32,
    claimed: Option<(Clause, f32)>,
}

impl OptionsUi {
    pub fn at(&self, ix: usize) -> f32 {
        stagger(self.reveal, ix)
    }

    pub fn flash(&self, clause: Clause) -> f32 {
        match self.claimed {
            Some((claimed, flash)) if claimed == clause => flash,
            _ => 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct BatchRun {
    cursor: Option<DateTime<Utc>>,
    placed: usize,
}

pub struct ItemCreator {
    pub focus_handle: FocusHandle,
    db_store: Entity<AppDatabaseStore>,
    workspace_generation: u64,

    mode: CreatorMode,
    queue_required: bool,
    selecting_mode: bool,
    details_open: bool,

    title_input: Entity<TextInput>,
    notes_input: Entity<Editor>,
    step_input: Entity<TextInput>,
    step_list: Entity<DynamicListState<StepDelegate>>,

    title: String,
    parsed: Option<ParseDraft>,
    draft: ItemDraft,

    batch: Option<BatchRun>,
    input_bounds: Bounds<Pixels>,
    step_input_bounds: Bounds<Pixels>,
    highlight_refresh_pending: bool,

    body_cue: Cue,
    claimed: Option<Clause>,
    claim_cue: Cue,
    details_overscroll: ElasticOverscroll,

    _subscriptions: Vec<Subscription>,
}

impl ItemCreator {
    pub fn new(mode: CreatorMode, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let db_store = AppDatabaseStore::global(cx);

        let title_input = cx.new(|cx| {
            TextInput::new("item-creator.title", window, cx)
                .placeholder(title_placeholder(mode))
                .name("Title")
                .bare(true)
        });
        window.focus(&title_input.read(cx).focus_handle(cx), cx);
        let notes_input = cx.new(|cx| {
            Editor::new("item-creator.notes", "Markdown source", "", window, cx)
                .rows(8)
                .line_numbers(false)
        });
        let step_input = cx.new(|cx| {
            TextInput::new("item-creator.step", window, cx)
                .placeholder("Add a step, then press Enter")
                .name("Step")
                .bare(true)
        });
        let step_list = cx.new(|cx| {
            DynamicListState::new(StepDelegate::new(), window, cx)
                .gap(STEP_GAP)
                .elastic_overscroll(false)
        });

        let mut subscriptions = Vec::new();

        subscriptions.push(cx.subscribe_in(
            &title_input,
            window,
            |this, _, event: &TextInputEvent, window, cx| match event {
                TextInputEvent::Change(_) => this.title_changed(window, cx),
                TextInputEvent::Submit => this.title_submitted(false, window, cx),
                TextInputEvent::Focus if this.selecting_mode => {
                    this.selecting_mode = false;
                    cx.notify();
                }
                _ => {}
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &notes_input,
            window,
            |this, _, event: &EditorEvent, window, cx| match event {
                EditorEvent::Changed(_) => cx.notify(),
                EditorEvent::Submitted => this.submit(window, cx),
                EditorEvent::Cancelled => {
                    window.dispatch_action(Box::new(CloseOverlay), cx);
                }
                _ => {}
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &step_input,
            window,
            |this, _, event: &TextInputEvent, window, cx| match event {
                TextInputEvent::Change(_) => {
                    this.highlight_refresh_pending = true;
                    cx.notify();
                }
                TextInputEvent::Submit => this.submit_step(window, cx),
                _ => {}
            },
        ));
        subscriptions.push(cx.observe(&step_list, |_, _, cx| cx.notify()));

        let mut draft = ItemDraft::new();
        draft.apply_mode_defaults(mode);

        Self {
            focus_handle: cx.focus_handle(),
            workspace_generation: db_store.read(cx).workspace_generation(),
            db_store,
            mode,
            queue_required: false,
            selecting_mode: false,
            details_open: false,
            title_input,
            notes_input,
            step_input,
            step_list,
            title: String::new(),
            parsed: None,
            draft,
            batch: None,
            input_bounds: Bounds::default(),
            step_input_bounds: Bounds::default(),
            highlight_refresh_pending: false,
            body_cue: Cue::new(BODY_CUE_MS),
            claimed: None,
            claim_cue: Cue::new(CLAIM_FLASH_MS),
            details_overscroll: ElasticOverscroll::default(),
            _subscriptions: subscriptions,
        }
    }

    pub fn new_on_date(
        mode: CreatorMode,
        date: chrono::NaiveDate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut creator = Self::new(mode, window, cx);
        creator.draft.schedule.date = Some(date);
        creator
    }

    pub fn new_queued_action(
        date: Option<chrono::NaiveDate>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut creator = Self::new(ItemType::Action, window, cx);
        creator.queue_required = true;
        creator.draft.queued = true;
        creator.draft.schedule.date = date;
        creator
    }

    pub fn new_marker_on_range(
        range: crate::dates::InclusiveDateRange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut creator = Self::new(ItemType::Marker, window, cx);
        creator.draft.set_marker_range(range);
        creator
    }

    fn is_expanded(&self) -> bool {
        !self.title.trim().is_empty()
    }

    fn step_count(&self, cx: &App) -> usize {
        self.step_list.read(cx).delegate().steps.len()
    }

    fn title_changed(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let value = self.title_input.read(cx).value().to_string();
        let was_expanded = self.is_expanded();
        self.title = value.trim().to_string();
        self.reparse(cx);
        if !was_expanded && self.is_expanded() {
            self.body_cue.trigger();
        } else if was_expanded && !self.is_expanded() {
            self.details_open = false;
            self.notes_input
                .update(cx, |editor, cx| editor.set_value("", cx));
        }
        self.highlight_refresh_pending = true;
        cx.notify();
    }

    fn title_submitted(&mut self, force: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !force && self.mode == ItemType::Routine && !self.title.is_empty() {
            cx.focus_view(&self.step_input, window);
            return;
        }
        self.submit(window, cx);
    }

    fn reparse(&mut self, _cx: &mut Context<Self>) {
        let text = self.title.as_str();
        self.parsed = if text.is_empty() {
            None
        } else {
            match self.mode {
                ItemType::Action => parser::parse_action(text).ok(),
                ItemType::Event => parser::parse_event(text).ok(),
                ItemType::Routine => parser::parse_action(text).ok(),
                ItemType::Marker => parser::parse_marker(text).ok(),
                ItemType::Signal => parser::parse_signal(text).ok(),
                ItemType::ActionTemplate => parser::parse_action(text).ok(),
                ItemType::EventTemplate => parser::parse_event(text).ok(),
            }
        };

        let outcome = match (&self.parsed, text.is_empty()) {
            (Some(parsed), _) => ParseOutcome::Read(parsed),
            (None, true) => ParseOutcome::Empty,
            (None, false) => ParseOutcome::Unreadable,
        };
        self.draft.sync_from_parse(outcome);
        self.draft.apply_mode_defaults(self.mode);
        if self.queue_required && self.mode == ItemType::Action {
            self.draft.queued = true;
        }
    }

    fn resolved_title(&self) -> &str {
        match self.parsed.as_ref() {
            Some(parsed) if !parsed.title.trim().is_empty() => parsed.title.trim(),
            _ => self.title.trim(),
        }
    }

    fn claim(&mut self, clause: Clause, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(range) = self
            .parsed
            .as_ref()
            .and_then(|parsed| clause_span(parsed, clause))
        else {
            return;
        };

        let stripped = strip_clause(&self.title, range);
        self.draft.disown(clause);
        self.claimed = Some(clause);
        self.claim_cue.trigger();
        self.title = stripped.trim().to_string();
        let value = self.title.clone();
        self.title_input
            .update(cx, |input, cx| input.set_text_quietly(value, cx));
        self.reparse(cx);
        self.highlight_refresh_pending = true;
    }

    fn claim_recurrence(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(parsed) = self.parsed.as_ref() else {
            return;
        };
        let ranges: Vec<_> = [
            Clause::Recurrence,
            Clause::RecurrenceEnd,
            Clause::RecurrenceCount,
        ]
        .into_iter()
        .filter_map(|clause| clause_span(parsed, clause))
        .collect();
        if ranges.is_empty() {
            return;
        }
        let stripped = strip_clauses(&self.title, ranges);
        self.draft.disown(Clause::Recurrence);
        self.draft.disown(Clause::RecurrenceEnd);
        self.draft.disown(Clause::RecurrenceCount);
        self.claimed = Some(Clause::Recurrence);
        self.claim_cue.trigger();
        self.title = stripped.trim().to_string();
        let value = self.title.clone();
        self.title_input
            .update(cx, |input, cx| input.set_text_quietly(value, cx));
        self.reparse(cx);
        self.highlight_refresh_pending = true;
    }

    fn toggle_details(&mut self, cx: &mut Context<Self>) {
        self.details_open = !self.details_open;
        if self.details_open {
            let notes_empty = self
                .notes_input
                .read(cx)
                .snapshot(cx)
                .text
                .trim()
                .is_empty();
            if notes_empty
                && let Some(content) = self
                    .parsed
                    .as_ref()
                    .and_then(|parsed| parsed.content.clone())
            {
                self.notes_input
                    .update(cx, |editor, cx| editor.set_value(content, cx));
            }
            self.body_cue.trigger();
        }
        cx.notify();
    }

    fn set_mode(&mut self, mode: CreatorMode, _window: &mut Window, cx: &mut Context<Self>) {
        self.selecting_mode = false;
        if self.mode == mode {
            cx.notify();
            return;
        }
        self.mode = mode;
        if mode != ItemType::Action {
            self.batch = None;
        }
        self.draft.forget_reading();
        self.reparse(cx);
        self.body_cue.trigger();
        self.title_input.update(cx, |input, cx| {
            input.set_placeholder(title_placeholder(mode), cx);
        });
        cx.notify();
    }

    fn step_mode(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let current = MODES.iter().position(|m| *m == self.mode).unwrap_or(0) as isize;
        let next = (current + delta).rem_euclid(MODES.len() as isize) as usize;
        self.set_mode(MODES[next], window, cx);
    }

    fn submit_step(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.step_input.read(cx).value().to_string();
        let raw = raw.trim();
        if raw.is_empty() {
            return;
        }

        let (title, duration) = match parser::parse_routine_step(raw) {
            Ok(parsed) if !parsed.title.trim().is_empty() => {
                (parsed.title.trim().to_string(), parsed.duration)
            }
            Ok(parsed) => (raw.to_string(), parsed.duration),
            Err(_) => (raw.to_string(), None),
        };

        self.step_list.update(cx, |list, cx| {
            list.update_items(cx, |delegate, _| {
                delegate.push(StepEntry::new(title, duration))
            });
            let last = list.delegate().steps.len().saturating_sub(1);
            list.scroll_item_into_view(last, cx);
        });
        self.step_input
            .update(cx, |input, cx| input.set_text_quietly("", cx));
        cx.notify();
    }

    fn is_batching(&self) -> bool {
        self.batch.is_some() && self.mode == ItemType::Action
    }

    fn toggle_batch(&mut self, cx: &mut Context<Self>) {
        if self.mode != ItemType::Action {
            return;
        }
        self.batch = match self.batch {
            Some(_) => None,
            None => Some(BatchRun::default()),
        };
        cx.notify();
    }

    fn batch_slot(&self, cx: &App) -> Option<(DateTime<Local>, DateTime<Local>)> {
        let run = self.batch.as_ref()?;
        let settings = Settings::global(cx);
        let context = self.db_store.read(cx).pipeline(&settings);

        let named = self
            .draft
            .schedule
            .has_time()
            .then(|| self.draft.schedule.datetime())
            .flatten();

        let probe = Action::new("")
            .with_duration(self.draft.duration.map(Into::into))
            .with_start(named.map(SchedulePoint::DateTime));
        let cursor = run.cursor.unwrap_or_else(|| context.batch_start());
        let placement = context.place_in_batch(cursor, probe);

        let start = DateTime::<Utc>::from(placement.action.start?);
        let end = start + context.effective_duration(&placement.action);
        Some((start.with_timezone(&Local), end.with_timezone(&Local)))
    }

    fn batch_summary(&self, cx: &App) -> Option<String> {
        let run = self.batch.as_ref()?;
        let (start, end) = self.batch_slot(cx)?;
        Some(format!(
            "#{} in batch · {} – {}",
            run.placed + 1,
            format_time(start.time()),
            format_time(end.time()),
        ))
    }

    fn blocker(&self, cx: &App) -> Option<&'static str> {
        let store = self.db_store.read(cx);
        if store.workspace_generation() != self.workspace_generation || !store.is_ready() {
            return Some(
                "The account workspace changed or is loading. Reopen the creator in the intended workspace.",
            );
        }
        self.draft
            .blocker(self.mode, &self.title, self.step_count(cx))
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.queue_required && self.mode == ItemType::Action {
            self.draft.queued = true;
        }
        if let Some(reason) = self.blocker(cx) {
            gpui_kit::overlay::toast::push(
                window,
                cx,
                timed_toast("item-creator.blocked", reason).tone(Tone::Warning),
            );
            return;
        }

        let steps = self.step_list.read(cx).delegate().to_steps();
        let typed_notes = self.notes_input.read(cx).snapshot(cx).text.to_string();
        let content = if typed_notes.trim().is_empty() {
            self.parsed
                .as_ref()
                .and_then(|parsed| parsed.content.clone())
        } else {
            Some(typed_notes)
        };
        let Some(item) = self
            .draft
            .build(self.mode, self.resolved_title(), content, steps)
        else {
            return;
        };

        for warning in self
            .parsed
            .as_ref()
            .map(|parsed| parsed.warnings.clone())
            .unwrap_or_default()
        {
            gpui_kit::overlay::toast::push(
                window,
                cx,
                timed_toast(format!("item-creator.warning.{warning}"), warning).tone(Tone::Warning),
            );
        }

        let batch = self.is_batching().then(|| self.batch.unwrap_or_default());
        let settings = Settings::global(cx);
        let mut advanced = None;

        self.db_store.update(cx, |store, cx| match item {
            AnyItem::Action(action) => match batch {
                Some(run) => {
                    advanced = Some(store.batch_action(action, run.cursor, &settings, cx));
                }
                None => store.upsert_action(action, cx),
            },
            AnyItem::Event(event) => store.upsert_event(event, cx),
            AnyItem::Routine(routine) => store.upsert_routine(routine, cx),
            AnyItem::Marker(marker) => store.upsert_marker(marker, cx),
            AnyItem::Signal(signal) => store.upsert_signal(signal, cx),
            item @ (AnyItem::ActionTemplate(_) | AnyItem::EventTemplate(_)) => {
                store.create_items(vec![item], cx)
            }
        });

        match (batch, advanced) {
            (Some(run), Some(cursor)) => {
                self.batch = Some(BatchRun {
                    cursor: Some(cursor),
                    placed: run.placed + 1,
                });
                self.reset(window, cx);
            }
            _ => window.dispatch_action(Box::new(CloseOverlay), cx),
        }
    }

    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.title.clear();
        self.parsed = None;
        self.draft = ItemDraft::new();
        self.draft.apply_mode_defaults(self.mode);
        if self.queue_required && self.mode == ItemType::Action {
            self.draft.queued = true;
        }
        self.details_open = false;
        self.title_input
            .update(cx, |input, cx| input.set_text_quietly("", cx));
        self.notes_input
            .update(cx, |editor, cx| editor.set_value("", cx));
        self.step_input
            .update(cx, |input, cx| input.set_text_quietly("", cx));
        self.step_list.update(cx, |list, cx| {
            list.update_items(cx, |delegate, _| delegate.clear());
        });
        cx.focus_view(&self.title_input, window);
        cx.notify();
    }

    pub(crate) fn date_chip(&self, ui: OptionsUi, ix: usize, cx: &Context<Self>) -> CreatorChip {
        CreatorChip::new("creator-date")
            .icon(AppIcon::CalendarPlus)
            .value(
                self.draft
                    .schedule
                    .date
                    .map(format_date)
                    .unwrap_or_else(|| "No date".into()),
            )
            .active(self.draft.schedule.is_set())
            .reveal(ui.at(ix))
            .flash(ui.flash(Clause::When))
            .on_click(cx.listener(|this, _, window, cx| {
                this.draft.toggle_date();
                this.claim(Clause::When, window, cx);
                cx.notify();
            }))
            .stepper(
                cx.listener(|this, _, window, cx| {
                    this.draft.step_days(-1);
                    this.claim(Clause::When, window, cx);
                    cx.notify();
                }),
                cx.listener(|this, _, window, cx| {
                    this.draft.step_days(1);
                    this.claim(Clause::When, window, cx);
                    cx.notify();
                }),
            )
    }

    pub(crate) fn time_chip(&self, ui: OptionsUi, ix: usize, cx: &Context<Self>) -> CreatorChip {
        CreatorChip::new("creator-time")
            .icon(AppIcon::Clock)
            .value(
                self.draft
                    .schedule
                    .time
                    .map(format_time)
                    .unwrap_or_else(|| "Any time".into()),
            )
            .active(self.draft.schedule.has_time())
            .reveal(ui.at(ix))
            .flash(ui.flash(Clause::When))
            .on_click(cx.listener(|this, _, window, cx| {
                this.draft.toggle_time();
                this.claim(Clause::When, window, cx);
                cx.notify();
            }))
            .stepper(
                cx.listener(|this, _, window, cx| {
                    this.draft.step_minutes(-1);
                    this.claim(Clause::When, window, cx);
                    cx.notify();
                }),
                cx.listener(|this, _, window, cx| {
                    this.draft.step_minutes(1);
                    this.claim(Clause::When, window, cx);
                    cx.notify();
                }),
            )
    }

    pub(crate) fn duration_chip(
        &self,
        ui: OptionsUi,
        ix: usize,
        cx: &Context<Self>,
    ) -> CreatorChip {
        CreatorChip::new("creator-duration")
            .icon(AppIcon::Timeline)
            .value(
                self.draft
                    .duration
                    .map(format_duration)
                    .unwrap_or_else(|| "No length".into()),
            )
            .active(self.draft.duration.is_some())
            .reveal(ui.at(ix))
            .flash(ui.flash(Clause::Duration))
            .on_click(cx.listener(|this, _, window, cx| {
                this.draft.toggle_duration();
                this.claim(Clause::Duration, window, cx);
                cx.notify();
            }))
            .stepper(
                cx.listener(|this, _, window, cx| {
                    this.draft.step_duration(-1);
                    this.claim(Clause::Duration, window, cx);
                    cx.notify();
                }),
                cx.listener(|this, _, window, cx| {
                    this.draft.step_duration(1);
                    this.claim(Clause::Duration, window, cx);
                    cx.notify();
                }),
            )
    }

    pub(crate) fn repeat_chip(&self, ui: OptionsUi, ix: usize, cx: &Context<Self>) -> CreatorChip {
        CreatorChip::new("creator-repeat")
            .icon(AppIcon::Repeat)
            .value(
                self.draft
                    .recurrence
                    .as_ref()
                    .map(format_recurrence)
                    .unwrap_or_else(|| "Once".into()),
            )
            .active(self.draft.recurrence.is_some())
            .reveal(ui.at(ix))
            .flash(ui.flash(Clause::Recurrence))
            .on_click(cx.listener(|this, _, window, cx| {
                this.draft.cycle_recurrence(true);
                this.claim_recurrence(window, cx);
                cx.notify();
            }))
            .stepper(
                cx.listener(|this, _, window, cx| {
                    this.draft.cycle_recurrence(false);
                    this.claim_recurrence(window, cx);
                    cx.notify();
                }),
                cx.listener(|this, _, window, cx| {
                    this.draft.cycle_recurrence(true);
                    this.claim_recurrence(window, cx);
                    cx.notify();
                }),
            )
    }

    pub(crate) fn recurrence_end_chip(
        &self,
        ui: OptionsUi,
        ix: usize,
        cx: &Context<Self>,
    ) -> CreatorChip {
        let recurrence = self.draft.recurrence;
        CreatorChip::new("creator-repeat-until")
            .icon(AppIcon::CalendarClock)
            .value(
                recurrence
                    .and_then(|recurrence| recurrence.end_date)
                    .map(format_date)
                    .unwrap_or_else(|| "No end".into()),
            )
            .active(recurrence.is_some_and(|recurrence| recurrence.end_date.is_some()))
            .disabled(recurrence.is_none())
            .reveal(ui.at(ix))
            .flash(ui.flash(Clause::RecurrenceEnd))
            .on_click(cx.listener(|this, _, window, cx| {
                this.draft.toggle_recurrence_end_date();
                this.claim(Clause::RecurrenceCount, window, cx);
                this.claim(Clause::RecurrenceEnd, window, cx);
                cx.notify();
            }))
            .stepper(
                cx.listener(|this, _, window, cx| {
                    this.draft.step_recurrence_end_date(-1);
                    this.claim(Clause::RecurrenceCount, window, cx);
                    this.claim(Clause::RecurrenceEnd, window, cx);
                    cx.notify();
                }),
                cx.listener(|this, _, window, cx| {
                    this.draft.step_recurrence_end_date(1);
                    this.claim(Clause::RecurrenceCount, window, cx);
                    this.claim(Clause::RecurrenceEnd, window, cx);
                    cx.notify();
                }),
            )
    }

    pub(crate) fn recurrence_count_chip(
        &self,
        ui: OptionsUi,
        ix: usize,
        cx: &Context<Self>,
    ) -> CreatorChip {
        let recurrence = self.draft.recurrence;
        CreatorChip::new("creator-repeat-count")
            .icon(AppIcon::ListOrdered)
            .value(
                recurrence
                    .and_then(|recurrence| recurrence.remaining)
                    .map(|remaining| format!("{remaining} more"))
                    .unwrap_or_else(|| "No count".into()),
            )
            .active(recurrence.is_some_and(|recurrence| recurrence.remaining.is_some()))
            .disabled(recurrence.is_none())
            .reveal(ui.at(ix))
            .flash(ui.flash(Clause::RecurrenceCount))
            .on_click(cx.listener(|this, _, window, cx| {
                this.draft.toggle_recurrence_remaining();
                this.claim(Clause::RecurrenceEnd, window, cx);
                this.claim(Clause::RecurrenceCount, window, cx);
                cx.notify();
            }))
            .stepper(
                cx.listener(|this, _, window, cx| {
                    this.draft.step_recurrence_remaining(-1);
                    this.claim(Clause::RecurrenceEnd, window, cx);
                    this.claim(Clause::RecurrenceCount, window, cx);
                    cx.notify();
                }),
                cx.listener(|this, _, window, cx| {
                    this.draft.step_recurrence_remaining(1);
                    this.claim(Clause::RecurrenceEnd, window, cx);
                    this.claim(Clause::RecurrenceCount, window, cx);
                    cx.notify();
                }),
            )
    }

    fn target_height(&self, cx: &App) -> Pixels {
        if !self.is_expanded() {
            return HEADER_HEIGHT;
        }
        self.body_height(cx)
    }

    fn body_height(&self, cx: &App) -> Pixels {
        let options_height = if self.details_open {
            OPTIONS_HEIGHT
        } else {
            OPTIONS_SUMMARY_HEIGHT
        };
        let mut height = HEADER_HEIGHT + options_height + FOOTER_HEIGHT;
        if self.details_open {
            height += DETAILS_NOTES_HEIGHT;
        }
        if self.mode == ItemType::Routine {
            height += steps_area_height(self.step_count(cx)) + STEP_INPUT_HEIGHT;
        }
        height
    }

    fn current_summary(&self, cx: &App) -> String {
        match self.mode {
            ItemType::Action => self
                .batch_summary(cx)
                .unwrap_or_else(|| self.action_summary()),
            ItemType::Event => self.event_summary(),
            ItemType::Routine => self.routine_summary(cx),
            ItemType::Marker => self.marker_summary(),
            ItemType::Signal => self.signal_summary(),
            ItemType::ActionTemplate => self.action_summary(),
            ItemType::EventTemplate => self.event_summary(),
        }
    }

    fn render_options(&mut self, ui: OptionsUi, cx: &mut Context<Self>) -> AnyElement {
        if !self.details_open {
            return div()
                .row()
                .w_full()
                .h(OPTIONS_SUMMARY_HEIGHT)
                .flex_none()
                .items_center()
                .px(FRAME_PADDING)
                .overflow_hidden()
                .when(ui.reveal < 1.0, |this| this.opacity(ui.reveal))
                .child(
                    Label::new(self.current_summary(cx))
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .text_color(creator_secondary_text(cx.theme())),
                )
                .into_any_element();
        }

        match self.mode {
            ItemType::Action => self.action_options(ui, cx),
            ItemType::Event => self.event_options(ui, cx),
            ItemType::Routine => self.routine_options(ui, cx),
            ItemType::Marker => self.marker_options(ui, cx),
            ItemType::Signal => self.signal_options(ui, cx),
            ItemType::ActionTemplate => self.action_options(ui, cx),
            ItemType::EventTemplate => self.event_options(ui, cx),
        }
    }

    fn render_notes(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .column()
            .w_full()
            .h(DETAILS_NOTES_HEIGHT)
            .flex_none()
            .px_4()
            .py_3()
            .gap_2()
            .border_t_1()
            .border_color(cx.theme().colors.hairline)
            .child(
                Label::new("Notes")
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(creator_secondary_text(cx.theme())),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_hidden()
                    .rounded_lg()
                    .bg(cx.theme().colors.raised.alpha(0.36))
                    .child(self.notes_input.clone()),
            )
            .into_any_element()
    }

    fn render_type_picker(&self, strip: f32, cx: &Context<Self>) -> AnyElement {
        let top = (HEADER_HEIGHT - TYPE_BUTTON_SIZE) / 2.;
        let strip_width = STRIP_WIDTH * strip;

        div()
            .absolute()
            .left(TYPE_BUTTON_LEFT)
            .top(top)
            .h(TYPE_BUTTON_SIZE)
            .child(
                div()
                    .absolute()
                    .size(TYPE_BUTTON_SIZE)
                    .when(strip > 0.0, |this| this.opacity(1.0 - strip))
                    .child(
                        Button::new("creator-mode")
                            .ghost()
                            .h(TYPE_BUTTON_SIZE)
                            .w(TYPE_TRIGGER_WIDTH)
                            .rounded_full()
                            .bg(cx.theme().colors.raised.alpha(0.6))
                            .border_1()
                            .border_color(cx.theme().colors.hairline)
                            .hover(|s| s.bg(cx.theme().colors.hover))
                            .icon(
                                item_icon(self.mode)
                                    .size_4()
                                    .text_color(cx.theme().colors.text_muted),
                            )
                            .when(!self.selecting_mode && strip <= f32::EPSILON, |this| {
                                this.on_click(cx.listener(|this, _, _, cx| {
                                    this.selecting_mode = true;
                                    cx.notify();
                                }))
                            }),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .h(TYPE_BUTTON_SIZE)
                    .w(strip_width)
                    .rounded_full()
                    .overflow_hidden()
                    .bg(cx.theme().colors.canvas.alpha(0.6))
                    .child(
                        div().row().w(STRIP_WIDTH).flex_none().h_full().children(
                            MODES
                                .iter()
                                .enumerate()
                                .map(|(ix, mode)| self.render_type_option(ix, *mode, strip, cx)),
                        ),
                    ),
            )
            .into_any_element()
    }

    fn render_type_option(
        &self,
        ix: usize,
        mode: ItemType,
        strip: f32,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let paint = type_option_paint(mode, self.mode, cx.theme());
        let reveal = stagger(strip, ix);

        div()
            .id(mode_option_id(mode))
            .row()
            .items_center()
            .justify_center()
            .gap_1p5()
            .h(TYPE_BUTTON_SIZE)
            .w(STRIP_ITEM)
            .flex_none()
            .rounded_full()
            .cursor_pointer()
            .bg(paint.background)
            .hover(|s| s.bg(paint.hover))
            .when(reveal < 1.0, |this| this.opacity(reveal))
            .child(item_icon(mode).size_4().flex_none().text_color(paint.icon))
            .child(
                Label::new(mode_label(mode))
                    .w(STRIP_ITEM - px(48.))
                    .flex_none()
                    .text_sm()
                    .font_weight(paint.label_weight)
                    .text_color(paint.label),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.set_mode(mode, window, cx);
            }))
    }

    fn render_highlights(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Div> {
        let Some(parsed) = self.parsed.as_ref() else {
            return Vec::new();
        };
        Self::render_input_highlights(&self.title_input, parsed, self.input_bounds, window, cx)
    }

    fn render_step_highlights(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Div> {
        let raw = self.step_input.read(cx).value().to_string();
        let Ok(parsed) = parser::parse_routine_step(&raw) else {
            return Vec::new();
        };
        Self::render_input_highlights(
            &self.step_input,
            &parsed,
            self.step_input_bounds,
            window,
            cx,
        )
    }

    fn render_input_highlights(
        input: &Entity<TextInput>,
        parsed: &ParseDraft,
        field_bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let pad = px(3.);
        let field_size = field_bounds.size;
        let recognition = RecognitionPaint::new(cx.theme());
        let ranges: Vec<std::ops::Range<usize>> = parsed
            .highlights
            .iter()
            .filter(|(_, kind)| *kind != parser::HighlightKind::Title)
            .map(|(range, _)| range.clone())
            .collect();

        ranges
            .iter()
            .filter_map(|range| {
                crate::components::text_input::range_bounds(input, range, field_size, window, cx)
            })
            .map(|bounds| {
                let centered_y =
                    bounds.origin.y + ((field_size.height - bounds.size.height) / 2.).max(px(0.));
                div()
                    .absolute()
                    .top(centered_y)
                    .left(bounds.origin.x - pad)
                    .h(bounds.size.height)
                    .w(bounds.size.width + pad * 2.)
                    .rounded_md()
                    .bg(recognition.fill)
                    .border_1()
                    .border_color(recognition.border)
            })
            .collect()
    }

    fn render_footer(&self, divider: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let blocker = self.blocker(cx);
        let can_submit = blocker.is_none();
        let submit_label = if self.is_batching() {
            "Add to batch"
        } else {
            submit_label(self.mode)
        };
        let batchable = self.mode == ItemType::Action;
        let warning = UxColor::Attention.color(cx.theme());
        let warning_text = UxColor::Attention.text(cx.theme());

        div()
            .row()
            .h(FOOTER_HEIGHT)
            .flex_none()
            .w_full()
            .px_4()
            .gap_2()
            .items_center()
            .border_t_1()
            .border_color(if divider {
                cx.theme().colors.hairline
            } else {
                gpui::transparent_black()
            })
            .child(
                div()
                    .row()
                    .items_center()
                    .gap_2()
                    .flex_none()
                    .child(
                        Button::new("creator-details")
                            .ghost()
                            .small()
                            .icon(AppIcon::Sliders)
                            .label("Details")
                            .selected(self.details_open)
                            .tooltip(if self.details_open {
                                "Hide item details"
                            } else {
                                "Show item details"
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_details(cx))),
                    )
                    .when(batchable, |this| {
                        this.child(
                            Switch::new("creator-batch")
                                .on(self.batch.is_some())
                                .label("Batch")
                                .on_change({
                                    let creator = cx.entity().clone();
                                    move |_on: bool, _window, cx| {
                                        creator.update(cx, |this, cx| this.toggle_batch(cx));
                                    }
                                }),
                        )
                    }),
            )
            .child(
                div()
                    .id("creator-submit-reason")
                    .row()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .when_some(blocker, |this, reason| {
                        this.child(Icon::new(AppIcon::Info).size_3().text_color(warning))
                            .child(
                                Label::new(reason)
                                    .min_w_0()
                                    .truncate()
                                    .text_xs()
                                    .text_color(warning_text),
                            )
                    }),
            )
            .child(
                div()
                    .row()
                    .flex_none()
                    .gap_2()
                    .child(
                        Button::new("creator-cancel")
                            .ghost()
                            .small()
                            .label("Cancel")
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(CloseOverlay), cx);
                            }),
                    )
                    .child(
                        Button::new("creator-submit")
                            .small()
                            .primary()
                            .label(submit_label)
                            .disabled(!can_submit)
                            .tooltip(blocker.unwrap_or(submit_label))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.submit(window, cx);
                            })),
                    ),
            )
    }
}

impl Focusable for ItemCreator {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.title_input.focus_handle(cx)
    }
}

impl Render for ItemCreator {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let details_overscroll_y = self.details_overscroll.advance(cx);
        if self.details_overscroll.needs_frame(cx) {
            window.request_animation_frame();
        }
        let max_height = px(f32::from(window.viewport_size().height) * 0.75);
        let height = animate(
            "creator-height",
            self.target_height(cx).min(max_height),
            EXPAND,
            window,
            cx,
        );
        let width = animate(
            "creator-width",
            if self.details_open {
                DETAILS_WIDTH
            } else {
                CREATOR_WIDTH
            },
            EXPAND,
            window,
            cx,
        );

        let strip = animate(
            "creator-strip",
            if self.selecting_mode { 1.0f32 } else { 0.0 },
            STRIP,
            window,
            cx,
        );
        let expanded = self.is_expanded();
        let option_height = if self.details_open {
            OPTIONS_HEIGHT
        } else {
            OPTIONS_SUMMARY_HEIGHT
        };
        let room = (f32::from(height - HEADER_HEIGHT) / f32::from(option_height)).clamp(0.0, 1.0);
        let body = if expanded {
            self.body_cue.progress(window)
        } else {
            room
        };

        let scrim_opacity = 0.42;
        let ui = OptionsUi {
            reveal: body,
            claimed: self
                .claimed
                .map(|clause| (clause, 1.0 - self.claim_cue.progress(window))),
        };
        let is_routine = self.mode == ItemType::Routine;
        let show_body = expanded || height > HEADER_HEIGHT + px(1.);
        let show_footer = expanded && height >= HEADER_HEIGHT + FOOTER_HEIGHT;
        let body_height = self.body_height(cx);
        let footer_top = body_height - FOOTER_HEIGHT;
        let notes_top = footer_top
            - if self.details_open {
                DETAILS_NOTES_HEIGHT
            } else {
                px(0.)
            };
        let step_input_top = notes_top - STEP_INPUT_HEIGHT;
        let radius = px(CREATOR_RADIUS);
        let divider_clear = |top: Pixels| height >= top + radius;
        if std::mem::take(&mut self.highlight_refresh_pending) {
            window.request_animation_frame();
        }
        let highlights = self.render_highlights(window, cx);
        let input_left = (TYPE_BUTTON_LEFT + TYPE_TRIGGER_WIDTH + px(8.))
            .max(TYPE_BUTTON_LEFT + STRIP_WIDTH * strip + px(8.));
        let batch_status = if expanded {
            None
        } else {
            self.batch.map(|run| {
                if run.placed == 0 {
                    "Batch on".to_string()
                } else {
                    format!("Batch · {} added", run.placed)
                }
            })
        };
        let input_right = if batch_status.is_some() {
            TYPE_BUTTON_LEFT + BATCH_STATUS_WIDTH + px(8.)
        } else {
            px(16.)
        };
        let entity = cx.entity();

        let frame = div()
            .id("item-creator")
            .key_context("ItemCreator")
            .track_focus(&self.focus_handle)
            .flex()
            .flex_col()
            .relative()
            .w(width)
            .h(height)
            .overflow_hidden()
            .rounded(radius)
            .border_1()
            .border_color(cx.theme().colors.hairline)
            .text_color(cx.theme().colors.text)
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
            .on_action(cx.listener(|this, _: &ToggleBatchMode, _, cx| this.toggle_batch(cx)))
            .on_action(cx.listener(|this, _: &SubmitItem, window, cx| this.submit(window, cx)))
            .on_action(cx.listener(|this, _: &CloseOverlay, _, cx| {
                if this.selecting_mode {
                    this.selecting_mode = false;
                    cx.notify();
                } else {
                    cx.propagate();
                }
            }))
            .capture_action::<InputCancel>(cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                if this.selecting_mode {
                    this.selecting_mode = false;
                    cx.notify();
                } else {
                    window.dispatch_action(Box::new(CloseOverlay), cx);
                }
            }))
            .on_action(cx.listener(|this, _: &NextType, window, cx| this.step_mode(1, window, cx)))
            .on_action(
                cx.listener(|this, _: &PreviousType, window, cx| this.step_mode(-1, window, cx)),
            )
            .on_action(cx.listener(|this, _: &SelectAction, window, cx| {
                this.set_mode(ItemType::Action, window, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectEvent, window, cx| {
                this.set_mode(ItemType::Event, window, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectRoutine, window, cx| {
                this.set_mode(ItemType::Routine, window, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectMarker, window, cx| {
                this.set_mode(ItemType::Marker, window, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectSignal, window, cx| {
                this.set_mode(ItemType::Signal, window, cx)
            }))
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(HEADER_HEIGHT)
                    .flex_none()
                    .border_b_1()
                    .border_color(if show_body && divider_clear(HEADER_HEIGHT) {
                        cx.theme().colors.hairline
                    } else {
                        gpui::transparent_black()
                    })
                    .child(
                        div()
                            .row()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(input_left)
                            .right(input_right)
                            .overflow_hidden()
                            .when(strip > 0.0, |this| this.opacity(1.0 - strip))
                            .children(highlights)
                            .on_prepaint(move |bounds, _, cx| {
                                entity.update(cx, |this, _| this.input_bounds = bounds)
                            })
                            .child(
                                div()
                                    .flex()
                                    .h(HEADER_HEIGHT)
                                    .w_full()
                                    .text_size(rems(1.35))
                                    .line_height(rems(2.0))
                                    .items_center()
                                    .child(ThemeOverlay::new(
                                        creator_input_theme,
                                        self.title_input.clone(),
                                    )),
                            ),
                    )
                    .child(self.render_type_picker(strip, cx))
                    .when_some(batch_status, |this, text| {
                        this.child(
                            div()
                                .id("creator-batch-status")
                                .row()
                                .absolute()
                                .right(TYPE_BUTTON_LEFT)
                                .top((HEADER_HEIGHT - px(28.)) / 2.)
                                .h(px(28.))
                                .w(BATCH_STATUS_WIDTH)
                                .items_center()
                                .justify_center()
                                .gap_1p5()
                                .rounded_full()
                                .border_1()
                                .border_color(cx.theme().colors.focus)
                                .bg(cx.theme().colors.selected)
                                .child(div().size_1p5().rounded_full().bg(cx.theme().colors.accent))
                                .child(
                                    Label::new(text)
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(creator_secondary_text(cx.theme())),
                                ),
                        )
                    }),
            );

        let frame = frame.when(show_body, |this| {
            let body = div()
                .id("item-creator.details-scroll")
                .column()
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_y_scroll()
                .on_scroll_wheel(cx.listener(|view, event, window, cx| {
                    if view.details_overscroll.handle_scroll(event, window, cx) {
                        cx.notify();
                    }
                }))
                .child(
                    div()
                        .relative()
                        .top(details_overscroll_y)
                        .child(self.render_options(ui, cx))
                        .when(is_routine, |this| {
                            this.child(self.render_steps(ui, cx))
                                .child(self.render_step_input(
                                    divider_clear(step_input_top),
                                    window,
                                    cx,
                                ))
                        })
                        .when(self.details_open, |this| this.child(self.render_notes(cx))),
                );
            this.child(body).when(show_footer, |this| {
                this.child(self.render_footer(divider_clear(footer_top), cx))
            })
        });

        overlay_with_scrim(
            "item-creator-overlay",
            div().column().child(ThemeOverlay::new(
                |theme| crate::views::fixed_glass_bevel_theme(theme, f32::from(HEADER_HEIGHT)),
                frame
                    .bg_glass()
                    .glass_surface(Surface::Panel)
                    .glass_radius_px(CREATOR_RADIUS)
                    .glass(|glass| glass.protect_text_contrast(false))
                    .when(cfg!(not(target_os = "macos")), |frame| {
                        frame.glass_preset(GlassPreset::Frosted)
                    }),
            )),
            OverlayPosition::Top(DefiniteLength::Fraction(0.125).into()),
            scrim_opacity,
            window,
            cx,
        )
    }
}

pub(crate) fn chip_row() -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(CHIP_GAP)
        .h(CHIP_HEIGHT)
        .flex_none()
}

pub(crate) fn options_frame(primary: impl IntoElement, secondary: impl IntoElement) -> AnyElement {
    div()
        .column()
        .w_full()
        .h(OPTIONS_HEIGHT)
        .flex_none()
        .px(FRAME_PADDING)
        .py_2p5()
        .gap_2()
        .overflow_hidden()
        .child(primary)
        .child(secondary)
        .into_any_element()
}

pub(crate) fn summary_text(
    text: impl Into<SharedString>,
    reveal: f32,
    cx: &App,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .h(CHIP_HEIGHT)
        .flex_1()
        .overflow_hidden()
        .when(reveal < 1.0, |this| this.opacity(reveal))
        .child(
            Label::new(text.into())
                .text_xs()
                .truncate()
                .text_color(cx.theme().colors.text_muted),
        )
}

impl ItemCreator {
    fn render_steps(&mut self, ui: OptionsUi, cx: &mut Context<Self>) -> AnyElement {
        let height = steps_area_height(self.step_count(cx));
        div()
            .w_full()
            .h(height)
            .flex_none()
            .px_4()
            .py(STEP_LIST_PADDING)
            .overflow_hidden()
            .when(ui.reveal < 1.0, |this| this.opacity(ui.reveal))
            .child(DynamicList::new(&self.step_list).size_full())
            .into_any_element()
    }

    fn render_step_input(
        &self,
        divider: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let highlights = self.render_step_highlights(window, cx);
        let entity = cx.entity();

        div()
            .row()
            .w_full()
            .h(STEP_INPUT_HEIGHT)
            .flex_none()
            .px_4()
            .gap_2()
            .items_center()
            .border_t_1()
            .border_color(if divider {
                cx.theme().colors.hairline
            } else {
                gpui::transparent_black()
            })
            .child(
                Icon::new(AppIcon::ListPlus)
                    .size_4()
                    .text_color(cx.theme().colors.text_muted),
            )
            .child(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .h_full()
                    .w_full()
                    .overflow_hidden()
                    .text_size(rems(0.9))
                    .children(highlights)
                    .on_prepaint(move |bounds, _, cx| {
                        entity.update(cx, |this, _| this.step_input_bounds = bounds)
                    })
                    .child(ThemeOverlay::new(
                        creator_input_theme,
                        self.step_input.clone(),
                    )),
            )
            .child(
                Button::new("creator-add-step")
                    .ghost()
                    .size_6()
                    .child(Icon::new(AppIcon::Plus).size_3())
                    .on_click(cx.listener(|this, _, window, cx| this.submit_step(window, cx))),
            )
    }
}

const SIGILS: [char; 3] = ['@', '~', '%'];

fn clause_span(parsed: &ParseDraft, clause: Clause) -> Option<std::ops::Range<usize>> {
    let kind = clause.highlight();
    parsed
        .highlights
        .iter()
        .find(|(range, highlight)| {
            if *highlight != kind {
                return false;
            }
            let text = parsed.raw[range.clone()].trim_start().to_ascii_lowercase();
            let is_recurrence_end = text.starts_with("until");
            let is_recurrence_count = text.starts_with("for ")
                && (text.ends_with(" occurrence")
                    || text.ends_with(" occurrences")
                    || text.ends_with(" times"));
            match clause {
                Clause::Recurrence => !is_recurrence_end && !is_recurrence_count,
                Clause::RecurrenceEnd => is_recurrence_end,
                Clause::RecurrenceCount => is_recurrence_count,
                Clause::When | Clause::Duration => true,
            }
        })
        .map(|(range, _)| range.clone())
}

fn strip_clauses(title: &str, mut ranges: Vec<std::ops::Range<usize>>) -> String {
    ranges.sort_by_key(|range| std::cmp::Reverse(range.start));
    ranges.into_iter().fold(title.to_string(), |title, range| {
        strip_clause(&title, range)
    })
}

fn strip_clause(title: &str, range: std::ops::Range<usize>) -> String {
    let mut start = range.start;
    let mut end = range.end.min(title.len());
    if start > end || !title.is_char_boundary(start) || !title.is_char_boundary(end) {
        return title.to_string();
    }

    if let Some(sigil) = title[..start].chars().next_back()
        && SIGILS.contains(&sigil)
    {
        start -= sigil.len_utf8();
    }

    let without_trailing = title[..start].trim_end_matches(' ');
    if without_trailing.len() < start {
        start = without_trailing.len();
    } else {
        let rest = &title[end..];
        end += rest.len() - rest.trim_start_matches(' ').len();
    }

    let mut out = String::with_capacity(title.len());
    out.push_str(&title[..start]);
    out.push_str(&title[end..]);
    out
}

#[derive(Debug, PartialEq)]
struct TypeOptionPaint {
    background: Hsla,
    icon: Hsla,
    label: Hsla,
    label_weight: FontWeight,
    hover: Hsla,
}

fn type_option_paint(mode: ItemType, current: ItemType, theme: &Theme) -> TypeOptionPaint {
    let selected = mode == current;
    TypeOptionPaint {
        background: if selected {
            theme.colors.selected
        } else {
            gpui::transparent_black()
        },
        icon: if selected {
            theme.colors.accent
        } else {
            theme.colors.text_muted
        },
        label: if selected {
            theme.colors.text
        } else {
            theme.colors.text_muted
        },
        label_weight: if selected {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        },
        hover: theme.colors.hover,
    }
}

fn mode_label(mode: ItemType) -> &'static str {
    match mode {
        ItemType::Action => "Action",
        ItemType::Event => "Event",
        ItemType::Routine => "Routine",
        ItemType::Marker => "Marker",
        ItemType::Signal => "Signal",
        ItemType::ActionTemplate => "Saved action",
        ItemType::EventTemplate => "Saved event",
    }
}

fn mode_option_id(mode: ItemType) -> &'static str {
    match mode {
        ItemType::Action => "creator-mode-option.action",
        ItemType::Event => "creator-mode-option.event",
        ItemType::Routine => "creator-mode-option.routine",
        ItemType::Marker => "creator-mode-option.marker",
        ItemType::Signal => "creator-mode-option.signal",
        ItemType::ActionTemplate => "creator-mode-option.action-template",
        ItemType::EventTemplate => "creator-mode-option.event-template",
    }
}

fn title_placeholder(mode: ItemType) -> &'static str {
    match mode {
        ItemType::Action => "What needs doing?",
        ItemType::Event => "What's happening, and when?",
        ItemType::Routine => "Name this routine",
        ItemType::Marker => "What's the occasion?",
        ItemType::Signal => "What should you be reminded of?",
        ItemType::ActionTemplate => "Name this saved action",
        ItemType::EventTemplate => "Name this saved event",
    }
}

fn submit_label(mode: ItemType) -> &'static str {
    match mode {
        ItemType::Action => "Add action",
        ItemType::Event => "Add event",
        ItemType::Routine => "Create routine",
        ItemType::Marker => "Add marker",
        ItemType::Signal => "Add signal",
        ItemType::ActionTemplate => "Save action",
        ItemType::EventTemplate => "Save event",
    }
}
