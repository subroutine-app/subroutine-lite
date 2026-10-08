use crate::AppIcon;
use crate::components::ext::ElementExt as _;
use crate::components::{
    Button, ButtonVariants, CloseOverlay, EmptyState, Label, elastic_overscroll::ElasticOverscroll,
    item_icon,
};
use crate::dates::{self, ChronoDates};
use crate::icons::Icon;
use crate::item_subject::{ItemSubject, ItemSubjectKey, SavedItem};
use crate::selection::{SelectionManager, bulk};
use crate::stores::AppDatabaseStore;
use crate::views::item_creator::{
    CreatorChip, ItemDraft, format_date, format_duration, format_recurrence, format_time,
};

use chrono::{Local, NaiveTime, Timelike as _};
use gpui::{
    App, AppContext as _, Context, Entity, Focusable, FontWeight, InteractiveElement, IntoElement,
    ParentElement, Pixels, Render, SharedString, StatefulInteractiveElement, Styled, Subscription,
    Window, div, prelude::FluentBuilder, px,
};
use gpui_kit::content::{Markdown, MarkdownEvent};
use gpui_kit::controls::editor::{Editor, EditorEvent};
use gpui_kit::controls::input::{TextInput, TextInputEvent};
use gpui_kit::controls::toggle::Switch;
use gpui_kit::datetime::{DateInput, DateInputEvent, TimeInput, TimeInputEvent, TimeOfDay};
use gpui_kit::foundation::{Disableable as _, Sizable as _, StyledExt as _};
use gpui_kit::layout::ScrollArea;
use gpui_kit::overlay::Tooltip;
use gpui_kit_semantics::{NodeSpec, Role, Semantic as _};
use gpui_kit_theme::{ActiveTheme, ControlSize, TypeScale};
use subroutine_core::{AnyItem, ItemType};

const NOTES_MIN_ROWS: usize = 6;
const NOTES_MIN_HEIGHT: Pixels = px(144.);
const ITEM_EDITOR_HEADER_HEIGHT: Pixels = px(48.);

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScheduleEditor {
    Date,
    Time,
    RecurrenceEnd,
}

pub struct ItemInspector {
    current_item: Option<ItemSubject>,
    baseline_item: Option<ItemSubject>,
    draft: ItemDraft,
    baseline: ItemDraft,
    title_input: Entity<TextInput>,
    date_input: Entity<DateInput>,
    time_input: Entity<TimeInput>,
    recurrence_end_input: Entity<DateInput>,
    date_error: Option<SharedString>,
    time_error: Option<SharedString>,
    recurrence_end_error: Option<SharedString>,
    schedule_editor: Option<ScheduleEditor>,
    notes_input: Entity<Editor>,
    notes_editing: bool,
    notes_rows: usize,
    notes_overscroll: ElasticOverscroll,
    unavailable: bool,
    workspace_generation: u64,
    transient: bool,
    pending_item: Option<ItemSubject>,
    _subscriptions: Vec<Subscription>,
}

impl ItemInspector {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let title_input = cx.new(|cx| {
            TextInput::new("item-inspector.title", window, cx)
                .name("Title")
                .placeholder("Untitled item")
                .bare(true)
                .control_size(ControlSize::Lg)
        });
        let date_input = cx.new(|cx| {
            DateInput::new(
                "item-inspector.schedule.date",
                ChronoDates::shared(),
                window,
                cx,
            )
            .name("Schedule date")
        });
        let time_input = cx.new(|cx| {
            TimeInput::new(
                "item-inspector.schedule.time",
                ChronoDates::shared(),
                window,
                cx,
            )
            .disabled(true)
        });
        let recurrence_end_input = cx.new(|cx| {
            DateInput::new(
                "item-inspector.recurrence.end-date",
                ChronoDates::shared(),
                window,
                cx,
            )
            .name("Repeat until")
        });
        let notes_input = cx.new(|cx| {
            Editor::new("item-inspector.notes", "Markdown source", "", window, cx)
                .rows(NOTES_MIN_ROWS)
                .line_numbers(false)
        });
        let date_field = date_input.read(cx).field().clone();
        let recurrence_end_field = recurrence_end_input.read(cx).field().clone();
        let subscriptions = vec![
            cx.subscribe(&title_input, |_, _, event: &TextInputEvent, cx| {
                if matches!(event, TextInputEvent::Change(_)) {
                    cx.notify();
                }
            }),
            cx.subscribe(
                &date_input,
                |inspector, input, event: &DateInputEvent, cx| match event {
                    DateInputEvent::Changed(day) => {
                        let Some(date) = dates::to_naive_date(*day) else {
                            inspector.date_error =
                                Some("That date is outside the supported range.".into());
                            input.update(cx, |input, cx| input.set_invalid(true, cx));
                            cx.notify();
                            return;
                        };
                        inspector.date_error = None;
                        inspector.draft.schedule.date = Some(date);
                        input.update(cx, |input, cx| {
                            input.set_invalid(false, cx);
                            input.set_value(Some(*day), cx);
                        });
                        cx.notify();
                    }
                    DateInputEvent::Unparsable { .. } => {
                        inspector.date_error = None;
                        input.update(cx, |input, cx| input.set_invalid(false, cx));
                        cx.notify();
                    }
                    DateInputEvent::Submit => inspector.save(cx),
                    _ => {}
                },
            ),
            cx.subscribe(
                &date_field,
                |inspector, _field, event: &TextInputEvent, cx| {
                    if let TextInputEvent::Change(text) = event
                        && text.trim().is_empty()
                    {
                        inspector.date_error = None;
                        inspector.time_error = None;
                        inspector.draft.schedule.date = None;
                        inspector.draft.schedule.time = None;
                        inspector.date_input.update(cx, |input, cx| {
                            input.set_invalid(false, cx);
                            input.set_value(None, cx);
                        });
                        inspector.sync_time_input(cx);
                        cx.notify();
                    }
                },
            ),
            cx.subscribe(
                &time_input,
                |inspector, input, event: &TimeInputEvent, cx| {
                    let TimeInputEvent::Changed(value) = event;
                    let Some(time) = time_input_to_naive(*value) else {
                        inspector.time_error =
                            Some("That time is outside the supported range.".into());
                        input.update(cx, |input, cx| input.set_invalid(true, cx));
                        cx.notify();
                        return;
                    };
                    inspector.time_error = None;
                    inspector.draft.schedule.time = Some(time);
                    input.update(cx, |input, cx| input.set_invalid(false, cx));
                    cx.notify();
                },
            ),
            cx.subscribe(
                &recurrence_end_input,
                |inspector, input, event: &DateInputEvent, cx| match event {
                    DateInputEvent::Changed(day) => {
                        let Some(date) = dates::to_naive_date(*day) else {
                            inspector.recurrence_end_error =
                                Some("That date is outside the supported range.".into());
                            input.update(cx, |input, cx| input.set_invalid(true, cx));
                            cx.notify();
                            return;
                        };
                        inspector.recurrence_end_error = None;
                        inspector.draft.set_recurrence_end_date(Some(date));
                        input.update(cx, |input, cx| {
                            input.set_invalid(false, cx);
                            input.set_value(Some(*day), cx);
                        });
                        cx.notify();
                    }
                    DateInputEvent::Unparsable { .. } => {
                        inspector.recurrence_end_error = None;
                        input.update(cx, |input, cx| input.set_invalid(false, cx));
                        cx.notify();
                    }
                    DateInputEvent::Submit => inspector.save(cx),
                    _ => {}
                },
            ),
            cx.subscribe(
                &recurrence_end_field,
                |inspector, _field, event: &TextInputEvent, cx| {
                    if let TextInputEvent::Change(text) = event
                        && text.trim().is_empty()
                    {
                        inspector.recurrence_end_error = None;
                        inspector.draft.set_recurrence_end_date(None);
                        inspector.recurrence_end_input.update(cx, |input, cx| {
                            input.set_invalid(false, cx);
                            input.set_value(None, cx);
                        });
                        cx.notify();
                    }
                },
            ),
            cx.subscribe(
                &notes_input,
                |inspector, _, event: &EditorEvent, cx| match event {
                    EditorEvent::Changed(_) => cx.notify(),
                    EditorEvent::Cancelled => {
                        inspector.notes_editing = false;
                        cx.notify();
                    }
                    _ => {}
                },
            ),
        ];

        Self {
            current_item: None,
            baseline_item: None,
            draft: ItemDraft::new(),
            baseline: ItemDraft::new(),
            title_input,
            date_input,
            time_input,
            recurrence_end_input,
            date_error: None,
            time_error: None,
            recurrence_end_error: None,
            schedule_editor: None,
            notes_input,
            notes_editing: false,
            notes_rows: NOTES_MIN_ROWS,
            notes_overscroll: ElasticOverscroll::default(),
            unavailable: false,
            workspace_generation: AppDatabaseStore::global(cx).read(cx).workspace_generation(),
            transient: false,
            pending_item: None,
            _subscriptions: subscriptions,
        }
    }

    pub(crate) fn show(&mut self, item: Option<AnyItem>, cx: &mut Context<Self>) {
        let item = item.map(|item| {
            let series_id = item.lineage_id();
            let is_virtual_series_occurrence = item.id() != series_id
                && match &item {
                    AnyItem::Event(event) => {
                        event.source_provider.is_none() && event.recurrence.is_some()
                    }
                    AnyItem::Marker(marker) => {
                        marker.source_provider.is_none() && marker.recurrence.is_some()
                    }
                    AnyItem::Signal(signal) => signal.recurrence.is_some(),
                    AnyItem::Action(_)
                    | AnyItem::Routine(_)
                    | AnyItem::ActionTemplate(_)
                    | AnyItem::EventTemplate(_) => false,
                };
            if is_virtual_series_occurrence {
                AppDatabaseStore::global(cx)
                    .read(cx)
                    .get_item(series_id)
                    .unwrap_or(item)
            } else {
                item
            }
        });

        self.show_subject(item.map(ItemSubject::Live), cx);
    }

    pub(crate) fn show_saved(&mut self, item: Option<SavedItem>, cx: &mut Context<Self>) {
        self.show_subject(item.map(ItemSubject::Saved), cx);
    }

    fn show_subject(&mut self, item: Option<ItemSubject>, cx: &mut Context<Self>) {
        if (self.unavailable || self.transient) && self.is_dirty(cx) {
            return;
        }
        let same_item =
            self.current_item.as_ref().map(ItemSubject::key) == item.as_ref().map(ItemSubject::key);
        if self.is_dirty(cx) && !same_item {
            if item.is_some() {
                self.pending_item = item;
                cx.notify();
            }
            return;
        }
        if same_item && self.is_dirty(cx) {
            self.current_item = item;
            self.unavailable = false;
            cx.notify();
            return;
        }
        self.load(item, cx);
    }

    pub(crate) fn current_key(&self) -> Option<ItemSubjectKey> {
        self.current_item.as_ref().map(ItemSubject::key)
    }

    pub(crate) fn show_transient(&mut self, item: AnyItem, cx: &mut Context<Self>) {
        self.load(Some(ItemSubject::Live(item)), cx);
        self.transient = true;
    }

    pub(crate) fn editor_focus_handle(&self, cx: &App) -> gpui::FocusHandle {
        self.title_input.read(cx).focus_handle(cx)
    }

    pub(crate) fn mark_unavailable(&mut self, cx: &mut Context<Self>) {
        self.unavailable = true;
        cx.notify();
    }

    fn load(&mut self, item: Option<ItemSubject>, cx: &mut Context<Self>) {
        self.workspace_generation = AppDatabaseStore::global(cx).read(cx).workspace_generation();
        self.current_item = item;
        self.baseline_item = self.current_item.clone();
        self.unavailable = false;
        self.transient = false;
        self.pending_item = None;
        self.draft = self
            .current_item
            .as_ref()
            .map(|subject| match subject {
                ItemSubject::Live(item) => ItemDraft::from_item(item),
                ItemSubject::Saved(item) => ItemDraft::from_saved(item),
            })
            .unwrap_or_default();
        self.baseline = self.draft.clone();
        let title = self
            .current_item
            .as_ref()
            .map(|item| item.title().to_string())
            .unwrap_or_default();
        let notes = self
            .current_item
            .as_ref()
            .and_then(ItemSubject::content)
            .unwrap_or_default();
        self.title_input
            .update(cx, |input, cx| input.set_text_quietly(title, cx));
        self.notes_input
            .update(cx, |input, cx| input.set_value(notes, cx));
        self.date_error = None;
        self.time_error = None;
        self.recurrence_end_error = None;
        self.schedule_editor = None;
        self.sync_schedule_inputs(cx);
        self.notes_editing = false;
        cx.notify();
    }

    fn sync_schedule_inputs(&mut self, cx: &mut Context<Self>) {
        let required = self.current_item.as_ref().is_some_and(|item| {
            !item.is_saved()
                && matches!(
                    item.item_type(),
                    ItemType::Event | ItemType::Marker | ItemType::Signal
                )
        });
        let day = self.draft.schedule.date.map(dates::from_naive_date);
        let invalid = self.date_error.is_some();
        self.date_input.update(cx, |input, cx| {
            input.set_required(required, cx);
            input.set_invalid(invalid, cx);
            input.set_value(day, cx);
        });
        self.sync_time_input(cx);
        self.sync_recurrence_end_input(cx);
    }

    fn sync_after_recurrence_change(&mut self, cx: &mut Context<Self>) {
        if self.draft.recurrence.is_none() {
            self.recurrence_end_error = None;
            if self.schedule_editor == Some(ScheduleEditor::RecurrenceEnd) {
                self.schedule_editor = None;
            }
        }
        self.sync_recurrence_end_input(cx);
    }

    fn sync_recurrence_end_input(&mut self, cx: &mut Context<Self>) {
        let day = self
            .draft
            .recurrence
            .and_then(|recurrence| recurrence.end_date)
            .map(dates::from_naive_date);
        let invalid = self.recurrence_end_error.is_some();
        self.recurrence_end_input.update(cx, |input, cx| {
            input.set_invalid(invalid, cx);
            input.set_value(day, cx);
        });
    }

    fn sync_time_input(&mut self, cx: &mut Context<Self>) {
        let time = self.draft.schedule.time;
        let invalid = self.time_error.is_some();
        let saved = self
            .current_item
            .as_ref()
            .is_some_and(ItemSubject::is_saved);
        self.time_input.update(cx, |input, cx| {
            input.set_disabled(!saved && time.is_none(), cx);
            input.set_invalid(invalid, cx);
            if let Some(time) = time {
                input.set_value(naive_to_time_input(time), cx);
            }
        });
    }

    fn title(&self, cx: &App) -> String {
        self.title_input.read(cx).value().to_string()
    }

    fn content(&self, cx: &App) -> Option<String> {
        let content = self.notes_input.read(cx).snapshot(cx).text.to_string();
        (!content.trim().is_empty()).then_some(content)
    }

    fn begin_notes_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.notes_editing = true;
        let area = self.notes_input.read(cx).text_area().clone();
        window.focus(&area.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    pub(crate) fn is_dirty(&self, cx: &App) -> bool {
        let Some(item) = self.baseline_item.as_ref() else {
            return false;
        };
        self.draft != self.baseline
            || self.date_input.read(cx).is_invalid()
            || self.date_error.is_some()
            || self.time_error.is_some()
            || self.recurrence_end_input.read(cx).is_invalid()
            || self.recurrence_end_error.is_some()
            || self.title(cx) != item.title()
            || self.content(cx) != normalized_content(item.content())
    }

    fn step_count(&self) -> usize {
        match self.current_item.as_ref().and_then(ItemSubject::as_live) {
            Some(AnyItem::Routine(routine)) => routine.steps.len(),
            _ => 0,
        }
    }

    fn blocker(&self, cx: &App) -> Option<&'static str> {
        let store = AppDatabaseStore::global(cx);
        let store = store.read(cx);
        if store.workspace_generation() != self.workspace_generation || !store.is_ready() {
            return Some(
                "The account workspace changed or is loading. This draft cannot be saved here.",
            );
        }
        let item = self.current_item.as_ref()?;
        if self.date_input.read(cx).is_invalid() || self.date_error.is_some() {
            return Some("Enter a valid date");
        }
        if self.time_error.is_some() {
            return Some("Enter a valid time");
        }
        if self.recurrence_end_input.read(cx).is_invalid() || self.recurrence_end_error.is_some() {
            return Some("Enter a valid repeat end date");
        }
        if item.is_saved() {
            if self.title(cx).trim().is_empty() {
                return Some("Name it first");
            }
            if matches!(item.item_type(), ItemType::Event | ItemType::EventTemplate)
                && self.draft.duration.is_none()
            {
                return Some("An event needs a duration");
            }
            None
        } else {
            self.draft
                .blocker(item.item_type(), &self.title(cx), self.step_count())
        }
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        if !self.is_dirty(cx) || self.blocker(cx).is_some() || self.unavailable {
            return;
        }
        let (Some(original), Some(baseline_item)) =
            (self.current_item.as_ref(), self.baseline_item.as_ref())
        else {
            return;
        };
        let typed_title = self.title(cx);
        let typed_content = self.content(cx);
        let title = if typed_title != baseline_item.title() {
            typed_title.trim().to_string()
        } else {
            original.title().to_string()
        };
        let content = if typed_content != normalized_content(baseline_item.content()) {
            typed_content
        } else {
            original.content()
        };
        let Some(updated) = (match original {
            ItemSubject::Live(item) => self
                .draft
                .apply_to(&self.baseline, item, &title, content.clone())
                .map(ItemSubject::Live),
            ItemSubject::Saved(item) => self
                .draft
                .apply_to_saved(&self.baseline, item, &title, content.clone())
                .map(ItemSubject::Saved),
        }) else {
            return;
        };

        self.current_item = Some(updated.clone());
        self.transient = false;
        self.baseline_item = Some(updated.clone());
        self.baseline = self.draft.clone();
        self.title_input
            .update(cx, |input, cx| input.set_text_quietly(title, cx));
        self.notes_input.update(cx, |input, cx| {
            input.set_value(content.unwrap_or_default(), cx)
        });
        AppDatabaseStore::global(cx).update(cx, |store, cx| match updated {
            ItemSubject::Live(AnyItem::Action(action)) => store.upsert_action(action, cx),
            ItemSubject::Live(AnyItem::Event(event)) => store.upsert_event(event, cx),
            ItemSubject::Live(AnyItem::Routine(routine)) => store.upsert_routine(routine, cx),
            ItemSubject::Live(AnyItem::Marker(marker)) => store.upsert_marker(marker, cx),
            ItemSubject::Live(AnyItem::Signal(signal)) => store.upsert_signal(signal, cx),
            ItemSubject::Live(AnyItem::ActionTemplate(template)) => {
                store.update_action_template(template, cx)
            }
            ItemSubject::Live(AnyItem::EventTemplate(template)) => {
                store.update_event_template(template, cx)
            }
            ItemSubject::Saved(SavedItem::Action(template)) => {
                store.update_action_template(template, cx)
            }
            ItemSubject::Saved(SavedItem::Event(template)) => {
                store.update_event_template(template, cx)
            }
        });

        if let Some(pending) = self.pending_item.take() {
            self.load(Some(pending), cx);
        } else {
            cx.notify();
        }
    }

    fn revert(&mut self, cx: &mut Context<Self>) {
        if let Some(pending) = self.pending_item.take() {
            self.load(Some(pending), cx);
            return;
        }
        let was_transient = self.transient;
        let item = if self.unavailable {
            match self.current_item.as_ref().map(ItemSubject::key) {
                Some(ItemSubjectKey::Live(_)) => SelectionManager::selected_items(cx)
                    .into_iter()
                    .last()
                    .map(ItemSubject::Live),
                Some(ItemSubjectKey::Saved(id)) => AppDatabaseStore::global(cx)
                    .read(cx)
                    .get_saved_item(id)
                    .map(ItemSubject::Saved),
                None => None,
            }
        } else {
            self.current_item.clone()
        };
        let keeps_transient = was_transient && item.is_some();
        self.load(item, cx);
        self.transient = keeps_transient;
    }

    fn destructive_items(&self, cx: &App) -> (Vec<AnyItem>, bool) {
        let Some(current) = self.current_item.as_ref().and_then(ItemSubject::as_live) else {
            return (Vec::new(), false);
        };
        let selected = SelectionManager::global(cx).read(cx);
        (
            vec![current.clone()],
            selected.ids().contains(&current.id()),
        )
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let used_selection = match self.current_item.as_ref() {
            Some(ItemSubject::Saved(item)) => {
                bulk::delete_saved(&[item.id()], window, cx);
                SelectionManager::global(cx)
                    .read(cx)
                    .ids()
                    .contains(&item.id())
            }
            Some(ItemSubject::Live(_)) => {
                let (items, used_selection) = self.destructive_items(cx);
                if items.is_empty() {
                    return;
                }
                bulk::delete(&items, window, cx);
                used_selection
            }
            None => return,
        };
        if used_selection {
            SelectionManager::clear_global(cx);
        }
        self.load(None, cx);
        window.dispatch_action(Box::new(CloseOverlay), cx);
    }

    fn complete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (items, used_selection) = self.destructive_items(cx);
        if items.is_empty() {
            return;
        }
        bulk::complete(&items, window, cx);
        if used_selection {
            SelectionManager::clear_global(cx);
        }
        self.load(None, cx);
        window.dispatch_action(Box::new(CloseOverlay), cx);
    }

    fn toggle_schedule_editor(&mut self, editor: ScheduleEditor, cx: &mut Context<Self>) {
        if self.schedule_editor == Some(editor) {
            self.schedule_editor = None;
            cx.notify();
            return;
        }

        self.schedule_editor = Some(editor);
        match editor {
            ScheduleEditor::Date if self.draft.schedule.date.is_none() => {
                self.draft.toggle_date();
                self.date_error = None;
                self.time_error = None;
                self.sync_schedule_inputs(cx);
            }
            ScheduleEditor::Time if !self.draft.schedule.has_time() => {
                self.draft.toggle_time();
                self.date_error = None;
                self.time_error = None;
                self.sync_schedule_inputs(cx);
            }
            ScheduleEditor::RecurrenceEnd
                if self
                    .draft
                    .recurrence
                    .is_some_and(|recurrence| recurrence.end_date.is_none()) =>
            {
                self.draft.toggle_recurrence_end_date();
                self.recurrence_end_error = None;
                self.sync_recurrence_end_input(cx);
            }
            _ => {}
        }
        cx.notify();
    }

    fn date_chip(&self, cx: &Context<Self>) -> CreatorChip {
        CreatorChip::new("item-inspector-date")
            .property_row()
            .disclosure(self.schedule_editor == Some(ScheduleEditor::Date))
            .icon(AppIcon::CalendarPlus)
            .label("Date")
            .value(
                self.draft
                    .schedule
                    .date
                    .map(format_date)
                    .unwrap_or_else(|| "No date".into()),
            )
            .active(self.draft.schedule.is_set())
            .on_click(
                cx.listener(|this, _, _, cx| this.toggle_schedule_editor(ScheduleEditor::Date, cx)),
            )
            .stepper(
                cx.listener(|this, _, _, cx| {
                    this.draft.step_days(-1);
                    this.date_error = None;
                    this.sync_schedule_inputs(cx);
                    cx.notify();
                }),
                cx.listener(|this, _, _, cx| {
                    this.draft.step_days(1);
                    this.date_error = None;
                    this.sync_schedule_inputs(cx);
                    cx.notify();
                }),
            )
    }

    fn time_chip(&self, cx: &Context<Self>) -> CreatorChip {
        CreatorChip::new("item-inspector-time")
            .property_row()
            .disclosure(self.schedule_editor == Some(ScheduleEditor::Time))
            .icon(AppIcon::Clock)
            .label("Time")
            .value(
                self.draft
                    .schedule
                    .time
                    .map(format_time)
                    .unwrap_or_else(|| "Any time".into()),
            )
            .active(self.draft.schedule.has_time())
            .on_click(
                cx.listener(|this, _, _, cx| this.toggle_schedule_editor(ScheduleEditor::Time, cx)),
            )
            .stepper(
                cx.listener(|this, _, _, cx| {
                    this.draft.step_minutes(-1);
                    this.time_error = None;
                    this.sync_schedule_inputs(cx);
                    cx.notify();
                }),
                cx.listener(|this, _, _, cx| {
                    this.draft.step_minutes(1);
                    this.time_error = None;
                    this.sync_schedule_inputs(cx);
                    cx.notify();
                }),
            )
    }

    fn preferred_time_chip(&self, cx: &Context<Self>) -> CreatorChip {
        CreatorChip::new("item-inspector-preferred-time")
            .property_row()
            .disclosure(self.schedule_editor == Some(ScheduleEditor::Time))
            .icon(AppIcon::Clock)
            .label("Preferred time")
            .value(
                self.draft
                    .schedule
                    .time
                    .map(format_time)
                    .unwrap_or_else(|| "Any time".into()),
            )
            .active(self.draft.schedule.time.is_some())
            .on_click(cx.listener(|this, _, _, cx| {
                if this.schedule_editor == Some(ScheduleEditor::Time) {
                    this.schedule_editor = None;
                } else {
                    this.schedule_editor = Some(ScheduleEditor::Time);
                    if this.draft.schedule.time.is_none() {
                        this.draft.schedule.toggle_time();
                        this.draft.schedule.date = None;
                    }
                    this.sync_time_input(cx);
                }
                cx.notify();
            }))
            .stepper(
                cx.listener(|this, _, _, cx| {
                    this.draft.schedule.shift_minutes(-15);
                    this.draft.schedule.date = None;
                    this.sync_time_input(cx);
                    cx.notify();
                }),
                cx.listener(|this, _, _, cx| {
                    this.draft.schedule.shift_minutes(15);
                    this.draft.schedule.date = None;
                    this.sync_time_input(cx);
                    cx.notify();
                }),
            )
    }

    fn duration_chip(&self, cx: &Context<Self>) -> CreatorChip {
        CreatorChip::new("item-inspector-duration")
            .property_row()
            .icon(AppIcon::Timeline)
            .label("Duration")
            .value(
                self.draft
                    .duration
                    .map(format_duration)
                    .unwrap_or_else(|| "No length".into()),
            )
            .active(self.draft.duration.is_some())
            .on_click(cx.listener(|this, _, _, cx| {
                this.draft.toggle_duration();
                cx.notify();
            }))
            .stepper(
                cx.listener(|this, _, _, cx| {
                    this.draft.step_duration(-1);
                    cx.notify();
                }),
                cx.listener(|this, _, _, cx| {
                    this.draft.step_duration(1);
                    cx.notify();
                }),
            )
    }

    fn repeat_chip(&self, cx: &Context<Self>) -> CreatorChip {
        CreatorChip::new("item-inspector-repeat")
            .property_row()
            .icon(AppIcon::Repeat)
            .label("Repeat")
            .value(
                self.draft
                    .recurrence
                    .as_ref()
                    .map(format_recurrence)
                    .unwrap_or_else(|| "Once".into()),
            )
            .active(self.draft.recurrence.is_some())
            .on_click(cx.listener(|this, _, _, cx| {
                this.draft.cycle_recurrence(true);
                this.sync_after_recurrence_change(cx);
                cx.notify();
            }))
            .stepper(
                cx.listener(|this, _, _, cx| {
                    this.draft.cycle_recurrence(false);
                    this.sync_after_recurrence_change(cx);
                    cx.notify();
                }),
                cx.listener(|this, _, _, cx| {
                    this.draft.cycle_recurrence(true);
                    this.sync_after_recurrence_change(cx);
                    cx.notify();
                }),
            )
    }

    fn recurrence_count_chip(&self, cx: &Context<Self>) -> CreatorChip {
        CreatorChip::new("item-inspector-repeat-count")
            .property_row()
            .icon(AppIcon::ListOrdered)
            .label("Repeat count")
            .value(
                self.draft
                    .recurrence
                    .and_then(|recurrence| recurrence.remaining)
                    .map(|remaining| format!("{remaining} more"))
                    .unwrap_or_else(|| "No count".into()),
            )
            .active(
                self.draft
                    .recurrence
                    .is_some_and(|recurrence| recurrence.remaining.is_some()),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.draft.toggle_recurrence_remaining();
                this.recurrence_end_error = None;
                this.sync_recurrence_end_input(cx);
                cx.notify();
            }))
            .stepper(
                cx.listener(|this, _, _, cx| {
                    this.draft.step_recurrence_remaining(-1);
                    this.recurrence_end_error = None;
                    this.sync_recurrence_end_input(cx);
                    cx.notify();
                }),
                cx.listener(|this, _, _, cx| {
                    this.draft.step_recurrence_remaining(1);
                    this.recurrence_end_error = None;
                    this.sync_recurrence_end_input(cx);
                    cx.notify();
                }),
            )
    }

    fn recurrence_end_chip(&self, cx: &Context<Self>) -> CreatorChip {
        CreatorChip::new("item-inspector-repeat-until")
            .property_row()
            .disclosure(self.schedule_editor == Some(ScheduleEditor::RecurrenceEnd))
            .icon(AppIcon::CalendarClock)
            .label("Repeat until")
            .value(
                self.draft
                    .recurrence
                    .and_then(|recurrence| recurrence.end_date)
                    .map(format_date)
                    .unwrap_or_else(|| "No end".into()),
            )
            .active(
                self.draft
                    .recurrence
                    .is_some_and(|recurrence| recurrence.end_date.is_some()),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_schedule_editor(ScheduleEditor::RecurrenceEnd, cx)
            }))
            .stepper(
                cx.listener(|this, _, _, cx| {
                    this.draft.step_recurrence_end_date(-1);
                    this.recurrence_end_error = None;
                    this.sync_recurrence_end_input(cx);
                    cx.notify();
                }),
                cx.listener(|this, _, _, cx| {
                    this.draft.step_recurrence_end_date(1);
                    this.recurrence_end_error = None;
                    this.sync_recurrence_end_input(cx);
                    cx.notify();
                }),
            )
    }

    fn span_chip(&self, cx: &Context<Self>) -> CreatorChip {
        CreatorChip::new("item-inspector-span")
            .property_row()
            .icon(AppIcon::Calendars)
            .label("Span")
            .value(match self.draft.span_days {
                1 => "1 day".to_string(),
                days => format!("{days} days"),
            })
            .active(self.draft.span_days > 1)
            .stepper(
                cx.listener(|this, _, _, cx| {
                    this.draft.step_span(-1);
                    cx.notify();
                }),
                cx.listener(|this, _, _, cx| {
                    this.draft.step_span(1);
                    cx.notify();
                }),
            )
    }

    fn render_date_editor(&self, item_type: ItemType, cx: &Context<Self>) -> gpui::Div {
        let has_date = self.draft.schedule.date.is_some();
        let required = matches!(
            item_type,
            ItemType::Event | ItemType::Marker | ItemType::Signal
        );
        let error = self.date_error.clone().or_else(|| {
            (!has_date && required).then(|| match item_type {
                ItemType::Event => SharedString::from("An event needs a date."),
                ItemType::Marker => SharedString::from("A marker needs a date."),
                ItemType::Signal => SharedString::from("A signal needs a date."),
                _ => unreachable!(),
            })
        });

        schedule_editor_frame(cx)
            .child(
                div()
                    .row()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().min_w_0().child(self.date_input.clone()))
                    .child(
                        Button::new("item-inspector.schedule.toggle-date")
                            .ghost()
                            .xsmall()
                            .label(if has_date { "Clear" } else { "Today" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.draft.toggle_date();
                                this.date_error = None;
                                this.time_error = None;
                                this.sync_schedule_inputs(cx);
                                cx.notify();
                            })),
                    ),
            )
            .when_some(error, |this, error| {
                this.child(
                    Label::new(error)
                        .text_xs()
                        .text_color(cx.theme().colors.danger),
                )
            })
    }

    fn render_recurrence_end_editor(&self, cx: &Context<Self>) -> gpui::Div {
        let has_end = self
            .draft
            .recurrence
            .is_some_and(|recurrence| recurrence.end_date.is_some());

        schedule_editor_frame(cx)
            .child(
                div()
                    .row()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(self.recurrence_end_input.clone()),
                    )
                    .child(
                        Button::new("item-inspector.recurrence.toggle-end-date")
                            .ghost()
                            .xsmall()
                            .label(if has_end { "Clear" } else { "Add" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.draft.toggle_recurrence_end_date();
                                this.recurrence_end_error = None;
                                this.sync_recurrence_end_input(cx);
                                cx.notify();
                            })),
                    ),
            )
            .when_some(self.recurrence_end_error.clone(), |this, error| {
                this.child(
                    Label::new(error)
                        .text_xs()
                        .text_color(cx.theme().colors.danger),
                )
            })
    }

    fn render_time_editor(&self, item_type: ItemType, cx: &Context<Self>) -> gpui::Div {
        let has_time = self.draft.schedule.has_time();
        let required = matches!(item_type, ItemType::Event | ItemType::Signal);
        let error = self.time_error.clone().or_else(|| {
            (!has_time && required).then(|| match item_type {
                ItemType::Event => SharedString::from("An event needs a time."),
                ItemType::Signal => SharedString::from("A signal needs a time."),
                _ => unreachable!(),
            })
        });

        schedule_editor_frame(cx)
            .child(
                div()
                    .row()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().min_w_0().child(self.time_input.clone()))
                    .child(
                        Button::new("item-inspector.schedule.toggle-time")
                            .ghost()
                            .xsmall()
                            .label(if has_time { "Clear" } else { "Add" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.draft.toggle_time();
                                this.time_error = None;
                                this.sync_schedule_inputs(cx);
                                cx.notify();
                            })),
                    ),
            )
            .when_some(error, |this, error| {
                this.child(
                    Label::new(error)
                        .text_xs()
                        .text_color(cx.theme().colors.danger),
                )
            })
    }

    fn render_saved_time_editor(&self, cx: &Context<Self>) -> gpui::Div {
        schedule_editor_frame(cx).child(
            div()
                .row()
                .w_full()
                .items_center()
                .gap_2()
                .child(div().flex_1().min_w_0().child(self.time_input.clone()))
                .child(
                    Button::new("item-inspector.schedule.clear-preferred-time")
                        .ghost()
                        .xsmall()
                        .label("Clear")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.draft.schedule.time = None;
                            this.time_error = None;
                            this.schedule_editor = None;
                            this.sync_time_input(cx);
                            cx.notify();
                        })),
                ),
        )
    }

    fn render_saved_schedule(&self, item_type: ItemType, cx: &Context<Self>) -> gpui::Div {
        let properties = div()
            .column()
            .w_full()
            .gap_1()
            .when(
                item_type == ItemType::Action || item_type == ItemType::ActionTemplate,
                |this| {
                    this.child(
                        div()
                            .column()
                            .w_full()
                            .child(self.preferred_time_chip(cx))
                            .when(self.schedule_editor == Some(ScheduleEditor::Time), |this| {
                                this.child(self.render_saved_time_editor(cx))
                            }),
                    )
                },
            )
            .child(self.duration_chip(cx))
            .child(self.repeat_chip(cx))
            .when(self.draft.recurrence.is_some(), |this| {
                this.child(self.recurrence_count_chip(cx)).child(
                    div()
                        .column()
                        .w_full()
                        .child(self.recurrence_end_chip(cx))
                        .when(
                            self.schedule_editor == Some(ScheduleEditor::RecurrenceEnd),
                            |this| this.child(self.render_recurrence_end_editor(cx)),
                        ),
                )
            });

        div().column().gap_6().child(
            div()
                .column()
                .gap_2()
                .child(section_label("Defaults", cx))
                .child(properties),
        )
    }

    fn render_schedule(&self, item_type: ItemType, cx: &Context<Self>) -> gpui::Div {
        let properties = div()
            .column()
            .w_full()
            .gap_1()
            .child(
                div()
                    .column()
                    .w_full()
                    .child(self.date_chip(cx))
                    .when(self.schedule_editor == Some(ScheduleEditor::Date), |this| {
                        this.child(self.render_date_editor(item_type, cx))
                    }),
            )
            .when(
                matches!(
                    item_type,
                    ItemType::Action | ItemType::Event | ItemType::Routine | ItemType::Signal
                ),
                |this| {
                    this.child(
                        div()
                            .column()
                            .w_full()
                            .child(self.time_chip(cx))
                            .when(self.schedule_editor == Some(ScheduleEditor::Time), |this| {
                                this.child(self.render_time_editor(item_type, cx))
                            }),
                    )
                },
            )
            .when(
                matches!(item_type, ItemType::Action | ItemType::Event),
                |this| this.child(self.duration_chip(cx)),
            )
            .when(item_type == ItemType::Marker, |this| {
                this.child(self.span_chip(cx))
            })
            .child(self.repeat_chip(cx))
            .when(self.draft.recurrence.is_some(), |this| {
                this.child(self.recurrence_count_chip(cx)).child(
                    div()
                        .column()
                        .w_full()
                        .child(self.recurrence_end_chip(cx))
                        .when(
                            self.schedule_editor == Some(ScheduleEditor::RecurrenceEnd),
                            |this| this.child(self.render_recurrence_end_editor(cx)),
                        ),
                )
            });

        div()
            .column()
            .gap_6()
            .child(
                div()
                    .column()
                    .gap_2()
                    .child(section_label("Schedule", cx))
                    .child(properties),
            )
            .when(item_type == ItemType::Action, |this| {
                let queued = self.draft.queued;
                let pinned = self.draft.pinned && self.draft.schedule.has_time();
                let can_pin = self.draft.schedule.has_time();
                this.child(
                    div()
                        .column()
                        .gap_2()
                        .child(section_label("Planning", cx))
                        .child(
                            div()
                                .column()
                                .gap_0p5()
                                .p_1()
                                .rounded(px(cx.theme().radii.control))
                                .bg(cx.theme().colors.raised.alpha(0.36))
                                .child(planning_toggle_row(
                                    AppIcon::ListChecks,
                                    "Queue",
                                    "Show in planning and Focus.",
                                    false,
                                    Switch::new("item-inspector-queued")
                                        .named("Queue")
                                        .control_size(ControlSize::Xs)
                                        .on(queued)
                                        .on_change({
                                            let entity = cx.entity().clone();
                                            move |next, _, cx| {
                                                entity.update(cx, |this, cx| {
                                                    this.draft.queued = next;
                                                    cx.notify();
                                                });
                                            }
                                        }),
                                    cx,
                                ))
                                .child(planning_toggle_row(
                                    AppIcon::Pin,
                                    "Pin time",
                                    if can_pin {
                                        "Keep this action at its scheduled time."
                                    } else {
                                        "Set a time to enable pinning."
                                    },
                                    !can_pin,
                                    Switch::new("item-inspector-pinned")
                                        .named(if can_pin {
                                            "Pin time"
                                        } else {
                                            "Pin time: set a time first"
                                        })
                                        .control_size(ControlSize::Xs)
                                        .on(pinned)
                                        .disabled(!can_pin)
                                        .on_change({
                                            let entity = cx.entity().clone();
                                            move |next, _, cx| {
                                                entity.update(cx, |this, cx| {
                                                    this.draft.pinned = next;
                                                    cx.notify();
                                                });
                                            }
                                        }),
                                    cx,
                                )),
                        ),
                )
            })
    }
}

impl Render for ItemInspector {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(item) = self.current_item.clone() else {
            return div()
                .column()
                .size_full()
                .child(EmptyState::new(
                    Icon::new(AppIcon::SquarePen),
                    "Nothing selected",
                ))
                .into_any_element();
        };

        let item_type = item.item_type();
        let dirty = self.is_dirty(cx);
        let blocker = self.blocker(cx);
        let unavailable = self.unavailable;

        let selection_count = if item.is_saved() {
            1
        } else {
            self.destructive_items(cx).0.len()
        };
        let item_id = item.id();
        let conflict = (!item.is_saved())
            .then(|| {
                AppDatabaseStore::global(cx)
                    .read(cx)
                    .blocked_conflict()
                    .cloned()
            })
            .flatten()
            .filter(|conflict| {
                conflict
                    .error
                    .resource
                    .is_some_and(|resource| resource.id() == item_id)
            });
        let notes_source = self.notes_input.read(cx).snapshot(cx).text.clone();
        let notes_body = if self.notes_editing {
            self.notes_input.clone().into_any_element()
        } else if notes_source.trim().is_empty() {
            div()
                .size_full()
                .semantic_in(
                    cx,
                    NodeSpec::new("item-inspector.notes.empty", Role::Status).text("No notes"),
                )
                .into_any_element()
        } else {
            Markdown::new(
                format!("item-inspector.notes.preview-{item_id}"),
                notes_source,
            )
            .on_event(|event, _window, cx| {
                if let MarkdownEvent::LinkClicked { href } = event {
                    cx.open_url(href.as_ref());
                }
            })
            .into_any_element()
        };
        let notes_overscroll_y = if self.notes_editing {
            self.notes_overscroll.reset();
            px(0.0)
        } else {
            let offset = self.notes_overscroll.advance(cx);
            if self.notes_overscroll.needs_frame(cx) {
                window.request_animation_frame();
            }
            offset
        };
        let notes_editor = self.notes_input.clone();
        let inspector = cx.entity().clone();
        let notes_viewport = div()
            .id("item-inspector-notes-body")
            .flex_1()
            .min_h(NOTES_MIN_HEIGHT)
            .min_w_0()
            .when_else(
                self.notes_editing,
                |this| this.overflow_hidden(),
                |this| {
                    this.overflow_y_scroll().on_scroll_wheel(cx.listener(
                        |view, event, window, cx| {
                            if view.notes_overscroll.handle_scroll(event, window, cx) {
                                cx.notify();
                            }
                        },
                    ))
                },
            )
            .child(div().relative().top(notes_overscroll_y).child(notes_body))
            .on_prepaint(move |bounds, _, cx| {
                let theme = cx.theme();
                let line_height = theme.type_style(TypeScale::Code).line_height;
                let chrome = theme.spacing.xs * 2.0;
                let available = (f32::from(bounds.size.height) - chrome).max(line_height);
                let rows = ((available / line_height).floor() as usize).max(NOTES_MIN_ROWS);
                let changed = inspector.update(cx, |this, _| {
                    if this.notes_rows == rows {
                        false
                    } else {
                        this.notes_rows = rows;
                        true
                    }
                });
                if changed {
                    notes_editor.update(cx, |editor, cx| editor.set_rows(rows, cx));
                }
            });
        let notes_mode = if self.notes_editing {
            Button::new("item-inspector-notes-preview")
                .ghost()
                .xsmall()
                .icon(AppIcon::ScanEye)
                .label("Preview")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.notes_editing = false;
                    cx.notify();
                }))
                .into_any_element()
        } else {
            Button::new("item-inspector-notes-edit")
                .ghost()
                .xsmall()
                .icon(AppIcon::SquarePen)
                .label("Edit")
                .on_click(cx.listener(|this, _, window, cx| this.begin_notes_edit(window, cx)))
                .into_any_element()
        };

        div()
            .id("item-inspector")
            .column()
            .size_full()
            .overflow_hidden()
            .child(
                div()
                    .row()
                    .h(ITEM_EDITOR_HEADER_HEIGHT)
                    .flex_none()
                    .px_4()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().colors.hairline)
                    .bg(cx.theme().colors.raised.alpha(0.18))
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .size_7()
                            .items_center()
                            .justify_center()
                            .rounded(px(cx.theme().radii.control))
                            .bg(cx.theme().colors.raised)
                            .child(item_icon(item_type).size_3p5().text_color(cx.theme().colors.text_muted)),
                    )
                    .child(
                        div()
                            .column()
                            .min_w_0()
                            .flex_1()
                            .child(
                                Label::new(if item.is_saved() {
                                    saved_type_name(item_type)
                                } else {
                                    type_name(item_type)
                                })
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD),
                            )
                            .when(selection_count > 1, |this| {
                                this.child(
                                    Label::new(format!("Editing 1 of {selection_count} selected"))
                                        .text_xs()
                                        .text_color(cx.theme().colors.text_muted),
                                )
                            }),
                    )
                    .when(dirty, |this| {
                        this.child(
                            div()
                                .flex_none()
                                .px_2()
                                .py_1()
                                .rounded_full()
                                .bg(cx.theme().colors.raised)
                                .child(
                                    Label::new(if unavailable {
                                        "Out of date"
                                    } else {
                                        "Unsaved"
                                    })
                                    .text_xs()
                                    .text_color(cx.theme().colors.text_muted)
                                    .font_weight(FontWeight::MEDIUM),
                                ),
                        )
                    })
                    .child(
                        Button::new("item-editor.close")
                            .ghost()
                            .compact()
                            .size_7()
                            .icon(AppIcon::Close)
                            .tooltip(if dirty {
                                "Save or revert changes before closing"
                            } else {
                                "Close editor"
                            })
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(CloseOverlay), cx);
                            }),
                    ),
            )
            .when_some(conflict, |inspector, conflict| {
                let mutation_id = conflict.mutation_id;
                inspector.child(
                    div()
                        .column()
                        .mx_4()
                        .mt_3()
                        .p_3()
                        .gap_2()
                        .rounded(px(cx.theme().radii.control))
                        .border_1()
                        .border_color(cx.theme().colors.warning.opacity(0.45))
                        .bg(cx.theme().colors.warning.opacity(0.08))
                        .child(
                            Label::new("This item changed on another device")
                                .font_weight(FontWeight::SEMIBOLD),
                        )
                        .child(
                            Label::new("Choose a version. Other changes stay queued.")
                                .text_xs()
                                .text_color(cx.theme().colors.text_muted),
                        )
                        .child(
                            div()
                                .row()
                                .justify_end()
                                .gap_2()
                                .child(
                                    Button::new((
                                        "item-inspector.conflict.use-other",
                                        mutation_id.as_u64_pair().1,
                                    ))
                                    .small()
                                    .ghost()
                                    .label("Use other version")
                                    .on_click(
                                        move |_, _, cx| {
                                            AppDatabaseStore::global(cx).update(cx, |store, cx| {
                                                store.accept_server_conflict(mutation_id, cx)
                                            });
                                        },
                                    ),
                                )
                                .child(
                                    Button::new((
                                        "item-inspector.conflict.keep-mine",
                                        mutation_id.as_u64_pair().1,
                                    ))
                                    .small()
                                    .primary()
                                    .label("Keep my changes")
                                    .on_click(
                                        move |_, _, cx| {
                                            AppDatabaseStore::global(cx).update(cx, |store, cx| {
                                                store.keep_local_conflict(mutation_id, cx)
                                            });
                                        },
                                    ),
                                ),
                        ),
                )
            })
            .child(
                div().flex_1().min_h_0().child(
                    ScrollArea::new("item-inspector-scroll")
                        .label("Item details")
                        .vertical()
                        .child(
                            div()
                                .column()
                                .w_full()
                                .min_h_full()
                                .p_4()
                                .gap_6()
                                .child(
                                    div()
                                        .column()
                                        .w_full()
                                        .gap_1()
                                        .child(section_label("Title", cx))
                                        .child(
                                            div()
                                                .w_full()
                                                .px_3()
                                                .py_1()
                                                .rounded(px(cx.theme().radii.control))
                                                .bg(cx.theme().colors.raised.alpha(0.36))
                                                .child(self.title_input.clone()),
                                        ),
                                )
                                .child(if item.is_saved() {
                                    self.render_saved_schedule(item_type, cx)
                                } else {
                                    self.render_schedule(item_type, cx)
                                })
                                .when_some(routine_steps(&item), |this, steps| {
                                    this.child(
                                        div()
                                            .column()
                                            .flex_none()
                                            .gap_2()
                                            .child(section_label("Steps", cx))
                                            .children(steps.into_iter().enumerate().map(
                                                |(ix, step)| {
                                                    div()
                                                        .row()
                                                        .w_full()
                                                        .gap_2()
                                                        .items_start()
                                                        .child(
                                                            Label::new(format!("{}", ix + 1))
                                                                .w_6()
                                                                .text_xs()
                                                                .text_color(
                                                                    cx.theme().colors.text_muted,
                                                                ),
                                                        )
                                                        .child(Label::new(step).min_w_0().text_sm())
                                                },
                                            )),
                                    )
                                })
                                .child(
                                    div()
                                        .column()
                                        .flex_1()
                                        .min_h_0()
                                        .gap_2()
                                        .child(
                                            div()
                                                .row()
                                                .w_full()
                                                .items_center()
                                                .justify_between()
                                                .child(section_label("Notes", cx))
                                                .child(notes_mode),
                                        )
                                        .child(notes_viewport),
                                ),
                        ),
                ),
            )
            .child(
                div()
                    .row()
                    .min_h(px(56.))
                    .flex_none()
                    .px_4()
                    .py_2()
                    .items_center()
                    .justify_between()
                    .border_t_1()
                    .border_color(cx.theme().colors.hairline)
                    .bg(cx.theme().colors.raised.alpha(0.18))
                    .child(
                        div()
                            .row()
                            .gap_1()
                            .when(
                                matches!(item.as_live(), Some(AnyItem::Action(action)) if !action.is_completed()),
                                |this| {
                                    this.child(
                                        Button::new("item-inspector-complete")
                                            .ghost()
                                            .small()
                                            .icon(AppIcon::Check)
                                            .label(if selection_count > 1 {
                                                "Complete selected"
                                            } else {
                                                "Complete"
                                            })
                                            .disabled(dirty)
                                            .tooltip(if dirty {
                                                "Save or revert changes before completing"
                                            } else {
                                                "Complete this action"
                                            })
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.complete(window, cx)
                                            })),
                                    )
                                },
                            )
                            .child(
                                Button::new("item-inspector-delete")
                                    .ghost()
                                    .small()
                                    .icon(AppIcon::Trash)
                                    .label(if selection_count > 1 {
                                        "Delete selected"
                                    } else {
                                        "Delete"
                                    })
                                    .text_color(cx.theme().colors.danger)
                                    .hover(|this| this.bg(cx.theme().colors.danger.alpha(0.1)))
                                    .disabled(dirty)
                                    .tooltip(if dirty {
                                        "Save or revert changes before deleting"
                                    } else if item.is_saved() {
                                        "Delete this saved item"
                                    } else {
                                        "Delete this item"
                                    })
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.delete(window, cx)),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .row()
                            .gap_1()
                            .child(
                                Button::new("item-inspector-revert")
                                    .ghost()
                                    .small()
                                    .label("Revert")
                                    .disabled(!dirty)
                                    .on_click(cx.listener(|this, _, _, cx| this.revert(cx))),
                            )
                            .child(
                                Button::new("item-inspector-save")
                                    .primary()
                                    .small()
                                    .label("Save")
                                    .disabled(!dirty || blocker.is_some() || unavailable)
                                    .tooltip(if unavailable {
                                        "This item is no longer available"
                                    } else {
                                        blocker.unwrap_or("Save changes")
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                            ),
                    ),
            )
            .into_any_element()
    }
}

fn normalized_content(content: Option<String>) -> Option<String> {
    content.filter(|content| !content.trim().is_empty())
}

fn naive_to_time_input(time: NaiveTime) -> TimeOfDay {
    let hour = match time.hour() % 12 {
        0 => 12,
        hour => hour,
    };
    TimeOfDay::new(hour, time.minute()).with_meridiem(usize::from(time.hour() >= 12))
}

fn time_input_to_naive(time: TimeOfDay) -> Option<NaiveTime> {
    if !(1..=12).contains(&time.hour) || time.minute > 59 {
        return None;
    }
    let hour = match time.meridiem? {
        0 => time.hour % 12,
        1 => time.hour % 12 + 12,
        _ => return None,
    };
    NaiveTime::from_hms_opt(hour, time.minute, 0)
}

fn schedule_editor_frame(cx: &App) -> gpui::Div {
    div()
        .column()
        .w_full()
        .gap_1()
        .mt_1()
        .mb_2()
        .pl_7()
        .pr_2()
        .py_2()
        .rounded(px(cx.theme().radii.control))
        .bg(cx.theme().colors.raised.alpha(0.28))
}

fn planning_toggle_row(
    icon: AppIcon,
    label: &'static str,
    description: &'static str,
    disabled: bool,
    control: impl IntoElement,
    cx: &App,
) -> gpui::Div {
    div()
        .row()
        .w_full()
        .min_h(px(38.))
        .items_center()
        .gap_2()
        .px_2()
        .py_1p5()
        .rounded(px(cx.theme().radii.control))
        .when(disabled, |this| this.opacity(0.5))
        .child(
            div().flex_none().w_5().flex().justify_center().child(
                Icon::new(icon)
                    .size_3p5()
                    .text_color(cx.theme().colors.text_muted),
            ),
        )
        .child(
            Label::new(label)
                .id(format!("item-inspector.planning.{label}"))
                .flex_1()
                .min_w_0()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .tooltip(move |_, cx| Tooltip::new(label, description).view(cx))
                .semantic_in(
                    cx,
                    NodeSpec::new(format!("item-inspector.planning.{label}"), Role::Text)
                        .text(label)
                        .description(description),
                ),
        )
        .child(control)
}

fn section_label(label: &'static str, cx: &App) -> gpui::Div {
    Label::new(label)
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().colors.text_muted)
}

fn saved_type_name(item_type: ItemType) -> &'static str {
    match item_type {
        ItemType::Action => "Saved action",
        ItemType::Event => "Saved event",
        ItemType::Routine => "Saved routine",
        ItemType::Marker => "Saved marker",
        ItemType::Signal => "Saved signal",
        ItemType::ActionTemplate => "Saved action",
        ItemType::EventTemplate => "Saved event",
    }
}

fn type_name(item_type: ItemType) -> &'static str {
    match item_type {
        ItemType::Action => "Action",
        ItemType::Event => "Event",
        ItemType::Routine => "Routine",
        ItemType::Marker => "Marker",
        ItemType::Signal => "Signal",
        ItemType::ActionTemplate => "Saved action",
        ItemType::EventTemplate => "Saved event",
    }
}

fn routine_steps(item: &ItemSubject) -> Option<Vec<String>> {
    let Some(AnyItem::Routine(routine)) = item.as_live() else {
        return None;
    };
    Some(
        routine
            .steps
            .iter()
            .map(|step| {
                step.duration
                    .map(|duration| {
                        let now = Local::now();
                        format!(
                            "{} · {}",
                            step.title,
                            super::format_item_duration(duration, now)
                        )
                    })
                    .unwrap_or_else(|| step.title.clone())
            })
            .collect(),
    )
}
