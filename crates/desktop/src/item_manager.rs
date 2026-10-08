mod draft;

pub(crate) use draft::{
    DRAFT_KEY_CONTEXT, DraftAsAction, DraftAsEvent, DraftType, OpenDraftTypeMenu,
};

use std::time::Duration;

use chrono::{DateTime, Utc};
use chronoutil::RelativeDuration;
use gpui::{
    App, AppContext, AsyncApp, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable as _,
    Global, Pixels, SharedString, Subscription, Window,
};
use gpui_kit::controls::input::{TextInput, TextInputEvent};
use gpui_kit::display::badge::Tone;
use gpui_kit::foundation::Sizable as _;
use gpui_kit_theme::ControlSize;

use parser::{self, ParseDraft};
use subroutine_core::{Action, AnyItem, Event, ItemType, PipelineContext, SchedulePoint};
use uuid::Uuid;

use crate::components::timed_toast;
use crate::item_subject::{ItemSubject, ItemSubjectKey, SavedItem};
use crate::selection::FocusHandoff;
use crate::settings::Settings;
use crate::stores::AppDatabaseStore;

const COMPLETE_ANIMATION_DURATION: Duration = Duration::from_millis(340);

const EXIT_ANIMATION_DURATION: Duration = Duration::from_millis(280);

type EditingSession = (
    Entity<TextInput>,
    Vec<std::ops::Range<usize>>,
    Bounds<Pixels>,
    Option<u64>,
);

pub struct DiscardDraft(pub Uuid);
impl EventEmitter<DiscardDraft> for ItemManager {}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DraftView {
    Queue,
    Timeline,
}

pub(crate) struct DraftSubmitted {
    pub view: DraftView,
    pub item: AnyItem,
}
impl EventEmitter<DraftSubmitted> for ItemManager {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditKind {
    Existing,
    Draft,
    FixedTypeDraft,
    ViewDraft {
        view: DraftView,
        cursor: Option<DateTime<Utc>>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditEnd {
    Submit,
    Finish,
}

pub fn abandons(value: &str) -> bool {
    value.trim().is_empty()
}

pub fn discards_draft(is_draft: bool, value: &str) -> bool {
    is_draft && abandons(value)
}

pub fn parsed_spans(parsed: &ParseDraft) -> Vec<std::ops::Range<usize>> {
    parsed
        .highlights
        .iter()
        .filter(|(_, kind)| *kind != parser::HighlightKind::Title)
        .map(|(range, _)| range.clone())
        .collect()
}

fn apply_action_when(action: &mut Action, when: &parser::WhenSpec) {
    match when {
        parser::WhenSpec::DateTime(time) => {
            action.set_queued(true);
            action.set_start(Some(SchedulePoint::DateTime(*time)));
        }
        parser::WhenSpec::NaiveDate(date) => {
            action.set_queued(false);
            action.set_start(Some(SchedulePoint::Date(*date)));
        }
    }
}

pub(crate) fn next_batch_draft(
    item: &AnyItem,
    context: &PipelineContext<'_>,
) -> Option<(AnyItem, Option<DateTime<Utc>>)> {
    let mut cursor = None;
    let start = match item.start() {
        Some(SchedulePoint::DateTime(start)) => {
            let duration = item
                .duration()
                .unwrap_or(context.config.default_action_duration);
            let next = context.quantize_ceil(start + duration);
            cursor = Some(next);
            let duration = matches!(item, AnyItem::Event(_)).then_some(RelativeDuration::hours(1));
            context
                .place_in_batch(next, Action::new("").with_duration(duration))
                .action
                .start
        }
        start => start,
    };
    let draft = match item {
        AnyItem::Action(_) => AnyItem::Action(Action::new("").with_queued(true).with_start(start)),
        AnyItem::Event(_) => AnyItem::Event(Event::new(
            "",
            DateTime::<Utc>::from(start?),
            chrono::Duration::hours(1),
        )),
        _ => return None,
    };
    Some((draft, cursor))
}

pub struct EditingItem {
    pub original: ItemSubject,
    alternate_draft: Option<AnyItem>,
    kind: EditKind,
    pub input: Entity<TextInput>,
    pub parsed: Option<ParseDraft>,
    pub input_bounds: Bounds<Pixels>,
    pub draft_type_bounds: Option<Bounds<Pixels>>,
    highlight_revision: u64,
    laid_out_highlight_revision: u64,
    _change: Option<Subscription>,
}

impl EditingItem {
    fn new(item: ItemSubject, kind: EditKind, window: &mut Window, cx: &mut App) -> Self {
        let title = item.title().to_string();
        let input = cx.new(|cx| {
            TextInput::new(format!("item-title.{}", item.id()), window, cx)
                .text(title.clone())
                .name("Title")
                .bare(true)
                .control_size(ControlSize::Xs)
        });
        window.focus(&input.read(cx).focus_handle(cx), cx);
        let mut editing = Self {
            original: item,
            alternate_draft: None,
            kind,
            input,
            parsed: None,
            input_bounds: Bounds::default(),
            draft_type_bounds: None,
            highlight_revision: 1,
            laid_out_highlight_revision: 0,
            _change: None,
        };
        editing.parsed = editing.parse(&title);
        editing
    }

    pub fn parsed_spans(&self) -> Vec<std::ops::Range<usize>> {
        self.parsed.as_ref().map(parsed_spans).unwrap_or_default()
    }

    pub fn value(&self, cx: &App) -> SharedString {
        self.input.read(cx).value().clone()
    }

    pub fn id(&self) -> Uuid {
        self.original.id()
    }

    pub fn key(&self) -> ItemSubjectKey {
        self.original.key()
    }

    pub fn item_type(&self) -> ItemType {
        self.original.item_type()
    }

    pub fn parse(&self, text: &str) -> Option<ParseDraft> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }

        match self.item_type() {
            ItemType::Action => parser::parse_action(text).ok(),
            ItemType::Event => parser::parse_event(text).ok(),
            ItemType::Routine => parser::parse_routine_step(text).ok(),
            ItemType::Marker => parser::parse_marker(text).ok(),
            ItemType::Signal => parser::parse_signal(text).ok(),
            ItemType::ActionTemplate => parser::parse_action_template(text).ok(),
            ItemType::EventTemplate => parser::parse_event_template(text).ok(),
        }
    }

    pub fn abandons(&self, value: &str) -> bool {
        abandons(value)
    }

    pub fn is_draft(&self) -> bool {
        self.kind != EditKind::Existing
    }

    pub fn discards_draft(&self, value: &str) -> bool {
        discards_draft(self.is_draft(), value)
    }
}

pub struct ItemManager {
    pub(super) editing_item: Option<EditingItem>,
    last_focus: Option<FocusHandle>,
    completing: std::collections::HashSet<Uuid>,
    exiting: std::collections::HashSet<Uuid>,
}

struct GlobalItemManager(Entity<ItemManager>);
impl Global for GlobalItemManager {}

impl ItemManager {
    fn new(_cx: &mut App) -> Self {
        Self {
            editing_item: None,
            last_focus: None,
            completing: std::collections::HashSet::new(),
            exiting: std::collections::HashSet::new(),
        }
    }

    pub fn return_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(focus) = self.last_focus.take() {
            window.focus(&focus, cx);
        }
    }

    pub fn initialize_global(cx: &mut App) -> Entity<Self> {
        if cx.has_global::<GlobalItemManager>() {
            return cx.global::<GlobalItemManager>().0.clone();
        }
        let store = cx.new(|cx| Self::new(cx));
        cx.set_global(GlobalItemManager(store.clone()));
        store
    }

    pub fn global(cx: &App) -> Entity<Self> {
        cx.global::<GlobalItemManager>().0.clone()
    }

    pub(super) fn begin_edit(
        &mut self,
        item: &AnyItem,
        is_draft: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = if is_draft {
            EditKind::Draft
        } else {
            EditKind::Existing
        };
        self.begin_subject_edit(ItemSubject::Live(item.clone()), kind, window, cx);
    }

    pub(super) fn begin_fixed_type_draft(
        &mut self,
        item: &AnyItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_subject_edit(
            ItemSubject::Live(item.clone()),
            EditKind::FixedTypeDraft,
            window,
            cx,
        );
    }

    pub(super) fn begin_view_draft(
        &mut self,
        item: &AnyItem,
        view: DraftView,
        cursor: Option<DateTime<Utc>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_subject_edit(
            ItemSubject::Live(item.clone()),
            EditKind::ViewDraft { view, cursor },
            window,
            cx,
        );
    }

    pub(super) fn begin_saved_edit(
        &mut self,
        item: &SavedItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_subject_edit(
            ItemSubject::Saved(item.clone()),
            EditKind::Existing,
            window,
            cx,
        );
    }

    fn begin_subject_edit(
        &mut self,
        item: ItemSubject,
        kind: EditKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editing_item.as_ref().is_some_and(|editing| {
            matches!(editing.kind, EditKind::ViewDraft { .. }) && editing.key() != item.key()
        }) {
            self.commit_open_edit(window, cx);
            if self.is_editing() {
                return;
            }
        }
        if self.last_focus.is_none() {
            self.last_focus = window.focused(cx);
        }
        if let Some(previous) = self.editing_item.take()
            && previous.is_draft()
            && previous.key() != item.key()
        {
            cx.emit(DiscardDraft(previous.id()));
        }
        let mut editing = EditingItem::new(item, kind, window, cx);
        let input = editing.input.clone();
        editing._change = Some(cx.subscribe_in(
            &input,
            window,
            |this, _, event: &TextInputEvent, _window, cx| {
                if let TextInputEvent::Change(text) = event
                    && let Some(editing) = this.editing_item.as_mut()
                {
                    editing.parsed = editing.parse(text);
                    editing.highlight_revision = editing.highlight_revision.wrapping_add(1);
                    cx.notify();
                }
            },
        ));
        self.editing_item = Some(editing);
        cx.notify();
    }

    pub(super) fn is_being_edited(&self, id: Uuid) -> bool {
        self.is_subject_being_edited(ItemSubjectKey::Live(id))
    }

    pub(super) fn is_subject_being_edited(&self, key: ItemSubjectKey) -> bool {
        self.editing_item
            .as_ref()
            .is_some_and(|editing_item| editing_item.key() == key)
    }

    pub(super) fn is_draft(&self, id: Uuid) -> bool {
        self.editing_item.as_ref().is_some_and(|editing_item| {
            editing_item.key() == ItemSubjectKey::Live(id) && editing_item.is_draft()
        })
    }

    pub(super) fn is_editing(&self) -> bool {
        self.editing_item.is_some()
    }

    pub(super) fn editing_session(&self, key: ItemSubjectKey) -> Option<EditingSession> {
        self.editing_item
            .as_ref()
            .filter(|editing| editing.key() == key)
            .map(|editing| {
                (
                    editing.input.clone(),
                    editing.parsed_spans(),
                    editing.input_bounds,
                    (editing.highlight_revision != editing.laid_out_highlight_revision)
                        .then_some(editing.highlight_revision),
                )
            })
    }

    pub(super) fn report_input_bounds(&mut self, key: ItemSubjectKey, bounds: Bounds<Pixels>) {
        if let Some(editing) = self.editing_item.as_mut()
            && editing.key() == key
        {
            editing.input_bounds = bounds;
        }
    }

    pub(super) fn acknowledge_highlight_layout(
        &mut self,
        key: ItemSubjectKey,
        revision: u64,
        cx: &mut Context<Self>,
    ) {
        if let Some(editing) = self.editing_item.as_mut()
            && editing.key() == key
            && editing.highlight_revision == revision
            && editing.laid_out_highlight_revision != revision
        {
            editing.laid_out_highlight_revision = revision;
            cx.notify();
        }
    }

    pub(super) fn commit_open_edit(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Uuid> {
        let (value, discarded_draft) = self.editing_item.as_ref().map(|editing| {
            let value = editing.value(cx);
            let discarded = editing
                .discards_draft(value.as_ref())
                .then_some(editing.id());
            (value, discarded)
        })?;
        self.commit_edit(value, EditEnd::Finish, window, cx);
        discarded_draft
    }

    pub(super) fn submit_open_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editing) = &self.editing_item {
            self.commit_edit(editing.value(cx), EditEnd::Submit, window, cx);
        }
    }

    pub(super) fn is_completing(&self, id: Uuid) -> bool {
        self.completing.contains(&id) || self.exiting.contains(&id)
    }

    pub(super) fn discard_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(item) = self.editing_item.take() {
            if item.is_draft() {
                cx.emit(DiscardDraft(item.id()));
            }
            self.return_focus(window, cx);
            cx.notify();
        }
    }

    fn commit_edit(
        &mut self,
        value: SharedString,
        end: EditEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let store = AppDatabaseStore::global(cx);
        let Some(editing) = self.editing_item.take() else {
            return;
        };

        let input_value = value.to_string();
        let draft = editing.parse(&input_value);

        if editing.abandons(&input_value) {
            if editing.discards_draft(&input_value) {
                cx.emit(DiscardDraft(editing.id()));
            }
            self.return_focus(window, cx);
            cx.notify();
            return;
        }

        let raw = input_value.trim().to_string();
        let settings = Settings::global(cx);
        let draft_view = match editing.kind {
            EditKind::ViewDraft { view, .. } => Some(view),
            _ => None,
        };
        let batching = end == EditEnd::Submit
            && match draft_view {
                Some(DraftView::Queue) => settings.queue_batch_mode,
                Some(DraftView::Timeline) => settings.timeline_batch_mode,
                None => false,
            };
        let cursor = match editing.kind {
            EditKind::ViewDraft { cursor, .. }
                if draft.as_ref().is_none_or(|draft| draft.when.is_none()) =>
            {
                cursor
            }
            _ => None,
        };
        let mut submitted = None;

        let committed = match &editing.original {
            ItemSubject::Live(AnyItem::Action(original)) => {
                let updated = match draft {
                    Some(ref d) => {
                        let mut a = original.clone();
                        a.title = d.title.clone();
                        if let Some(ref when) = d.when {
                            apply_action_when(&mut a, when);
                        }
                        if let Some(dur) = d.duration {
                            a.duration = Some(dur.into());
                        }
                        if let Some(ref rec) = d.recurrence
                            && let Some(rule) = parser::recurrence_to_rule(Some(rec))
                        {
                            a.recurrence = Some(rule);
                        }
                        if let Some(ref content) = d.content {
                            a.content = Some(content.clone());
                        }
                        a
                    }
                    None => {
                        let mut a = original.clone();
                        a.title = raw.clone();
                        a
                    }
                };
                if draft_view.is_some() {
                    let mut updated = updated;
                    if draft_view == Some(DraftView::Queue) {
                        updated.set_queued(true);
                    }
                    let items =
                        if batching && let Some(SchedulePoint::DateTime(start)) = updated.start {
                            let start = cursor.unwrap_or(start);
                            updated.set_start(Some(SchedulePoint::DateTime(start)));
                            let placement = store
                                .read(cx)
                                .pipeline(&settings)
                                .place_in_batch(start, updated);
                            let items = placement.moved().cloned().map(AnyItem::Action).collect();
                            updated = placement.action;
                            items
                        } else {
                            vec![AnyItem::Action(updated.clone())]
                        };
                    submitted = Some(AnyItem::Action(updated));
                    store.update(cx, |store, cx| store.try_create_items(items, cx))
                } else {
                    store.update(cx, |store, cx| store.upsert_action(updated, cx));
                    true
                }
            }
            ItemSubject::Live(AnyItem::Event(original)) => {
                let updated = match draft {
                    Some(ref d) => {
                        let mut e = original.clone();
                        e.title = d.title.clone();
                        if let Some(parser::WhenSpec::DateTime(dt)) = d.when {
                            e.start = dt;
                        }
                        if let Some(dur) = d.duration {
                            e.duration = dur.into();
                        }
                        if let Some(ref rec) = d.recurrence
                            && let Some(rule) = parser::recurrence_to_rule(Some(rec))
                        {
                            e.recurrence = Some(rule);
                        }
                        if let Some(ref content) = d.content {
                            e.content = Some(content.clone());
                        }
                        e
                    }
                    None => {
                        let mut e = original.clone();
                        e.title = raw.clone();
                        e
                    }
                };
                if draft_view.is_some() {
                    let mut updated = updated;
                    let mut items = Vec::new();
                    if batching {
                        let start = cursor.unwrap_or(updated.start);
                        let probe = Action::new("")
                            .with_start(Some(SchedulePoint::DateTime(start)))
                            .with_duration(Some(updated.duration));
                        let placement = store
                            .read(cx)
                            .pipeline(&settings)
                            .place_in_batch(start, probe);
                        if let Some(SchedulePoint::DateTime(start)) = placement.action.start {
                            updated.start = start;
                        }
                        items.extend(placement.displaced.into_iter().map(AnyItem::Action));
                    }
                    let item = AnyItem::Event(updated);
                    submitted = Some(item.clone());
                    items.push(item);
                    store.update(cx, |store, cx| store.try_create_items(items, cx))
                } else {
                    store.update(cx, |store, cx| store.upsert_event(updated, cx));
                    true
                }
            }
            ItemSubject::Live(AnyItem::Routine(original)) => {
                let mut routine = original.clone();
                routine.title = match draft {
                    Some(ref d) => d.title.clone(),
                    None => raw.clone(),
                };
                if let Some(ref d) = draft {
                    if let Some(ref rec) = d.recurrence
                        && let Some(rule) = parser::recurrence_to_rule(Some(rec))
                    {
                        routine.recurrence = Some(rule);
                    }
                    if let Some(ref content) = d.content {
                        routine.content = Some(content.clone());
                    }
                }
                store.update(cx, |store, cx| store.upsert_routine(routine, cx));
                true
            }
            ItemSubject::Live(AnyItem::Marker(original)) => {
                let updated = match draft {
                    Some(ref d) => {
                        let mut marker = original.clone();
                        marker.title = d.title.clone();
                        if let Some(ref when) = d.when {
                            use parser::WhenSpec;
                            match when {
                                WhenSpec::NaiveDate(date) => marker.set_date(*date),
                                WhenSpec::DateTime(dt) => marker.set_date(dt.naive_local().date()),
                            }
                        }
                        if let Some(duration) = d.duration {
                            let end_date = marker.date + duration - chrono::Duration::days(1);
                            if end_date <= marker.date {
                                marker.set_end_date(None);
                            } else {
                                marker.set_end_date(Some(end_date));
                            }
                        }
                        if let Some(ref rec) = d.recurrence
                            && let Some(rule) = parser::recurrence_to_rule(Some(rec))
                        {
                            marker.recurrence = Some(rule);
                        }
                        if let Some(ref content) = d.content {
                            marker.content = Some(content.clone());
                        }
                        marker
                    }
                    None => {
                        let mut marker = original.clone();
                        marker.title = raw.clone();
                        marker
                    }
                };
                store.update(cx, |store, cx| store.upsert_marker(updated, cx));
                true
            }
            ItemSubject::Live(AnyItem::Signal(original)) => {
                let updated = match draft {
                    Some(ref d) => {
                        let mut signal = original.clone();
                        signal.title = d.title.clone();
                        if let Some(parser::WhenSpec::DateTime(dt)) = d.when {
                            signal.datetime = dt;
                        }
                        if let Some(ref rec) = d.recurrence
                            && let Some(rule) = parser::recurrence_to_rule(Some(rec))
                        {
                            signal.recurrence = Some(rule);
                        }
                        if let Some(ref content) = d.content {
                            signal.content = Some(content.clone());
                        }
                        signal
                    }
                    None => {
                        let mut signal = original.clone();
                        signal.title = raw.clone();
                        signal
                    }
                };
                store.update(cx, |store, cx| store.upsert_signal(updated, cx));
                true
            }
            ItemSubject::Saved(SavedItem::Action(original))
            | ItemSubject::Live(AnyItem::ActionTemplate(original)) => {
                let mut template = original.clone();
                template.title = draft.as_ref().map_or(raw, |draft| draft.title.clone());
                if let Some(draft) = draft {
                    if let Some(time) = draft.naive_time {
                        template.naive_time = Some(time);
                    }
                    if let Some(duration) = draft.duration {
                        template.duration = Some(duration.into());
                    }
                    if let Some(recurrence) = parser::recurrence_to_rule(draft.recurrence.as_ref())
                    {
                        template.recurrence = Some(
                            recurrence
                                .with_end_date(draft.recurrence_end_date)
                                .with_remaining(draft.recurrence_remaining),
                        );
                    }
                    if let Some(content) = draft.content {
                        template.content = Some(content);
                    }
                }
                store.update(cx, |store, cx| {
                    if editing.is_draft() {
                        store.try_create_items(vec![AnyItem::ActionTemplate(template)], cx)
                    } else {
                        store.update_action_template(template, cx);
                        true
                    }
                })
            }
            ItemSubject::Saved(SavedItem::Event(original))
            | ItemSubject::Live(AnyItem::EventTemplate(original)) => {
                let mut template = original.clone();
                template.title = draft.as_ref().map_or(raw, |draft| draft.title.clone());
                if let Some(draft) = draft {
                    if let Some(duration) = draft.duration {
                        template.duration = duration.into();
                    }
                    if let Some(recurrence) = parser::recurrence_to_rule(draft.recurrence.as_ref())
                    {
                        template.recurrence = Some(
                            recurrence
                                .with_end_date(draft.recurrence_end_date)
                                .with_remaining(draft.recurrence_remaining),
                        );
                    }
                    if let Some(content) = draft.content {
                        template.content = Some(content);
                    }
                }
                store.update(cx, |store, cx| {
                    if editing.is_draft() {
                        store.try_create_items(vec![AnyItem::EventTemplate(template)], cx)
                    } else {
                        store.update_event_template(template, cx);
                        true
                    }
                })
            }
        };

        if !committed {
            cx.focus_view(&editing.input, window);
            self.editing_item = Some(editing);
            gpui_kit::overlay::toast::push(
                window,
                cx,
                timed_toast(
                    "item.save-failed",
                    "Couldn’t save this item. Your draft is still open.",
                )
                .tone(Tone::Warning),
            );
            cx.notify();
            return;
        }

        self.return_focus(window, cx);
        if batching && let (Some(view), Some(item)) = (draft_view, submitted) {
            cx.emit(DraftSubmitted { view, item });
        }
        cx.notify();
    }

    pub(super) fn begin_complete_action(
        &mut self,
        action: Action,
        handoff: Option<FocusHandoff>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(handoff) = handoff {
            handoff.take(window, cx);
        }
        self.completing.insert(action.id);
        let action_id = action.id;
        let transaction = AppDatabaseStore::global(cx)
            .update(cx, |store, cx| store.complete_action(action_id, cx));
        let Some(transaction) = transaction else {
            self.completing.remove(&action_id);
            gpui_kit::overlay::toast::push(
                window,
                cx,
                timed_toast(
                    "item.completion-failed",
                    format!("Couldn’t complete {}", action.title),
                )
                .tone(Tone::Warning),
            );
            cx.notify();
            return;
        };

        let title = action.title;
        let message = format!("Completed {title}");
        gpui_kit::overlay::toast::push(
            window,
            cx,
            timed_toast("item.completed", message)
                .tone(Tone::Success)
                .action("Undo", move |window, cx| {
                    let undone = AppDatabaseStore::global(cx)
                        .update(cx, |store, cx| store.undo_transaction(transaction, cx));
                    if !undone {
                        gpui_kit::overlay::toast::push(
                            window,
                            cx,
                            timed_toast(
                                "item.undo-unavailable",
                                "That completion is no longer the latest change and was not undone.",
                            )
                            .tone(Tone::Warning),
                        );
                    }
                }),
        );
        cx.notify();

        cx.spawn(async move |this, cx: &mut AsyncApp| {
            cx.background_executor()
                .timer(COMPLETE_ANIMATION_DURATION)
                .await;
            let _ = this.update(cx, |view, cx| {
                view.completing.remove(&action_id);
                view.exiting.insert(action_id);
                cx.notify();
            });

            cx.background_executor()
                .timer(EXIT_ANIMATION_DURATION)
                .await;
            let _ = this.update(cx, |view, cx| {
                view.exiting.remove(&action_id);
                cx.notify();
            });
        })
        .detach();
    }
}
