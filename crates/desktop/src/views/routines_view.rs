use std::collections::{HashMap, HashSet};

use chrono::Utc;
use chronoutil::RelativeDuration;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, ElementId, Entity, FocusHandle,
    Focusable as _, FontWeight, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    Pixels, Render, SharedString, StatefulInteractiveElement, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_kit::controls::input::{TextInput, TextInputEvent};
use gpui_kit::foundation::{Sizable as _, StyledExt as _};
use gpui_kit_theme::{ActiveTheme, ControlSize};
use subroutine_core::{AnyItem, Routine, RoutineStep};
use uuid::Uuid;

use crate::{
    AppIcon,
    components::ext::InteractiveElementExt as _,
    components::{
        Button, ButtonVariants, DynamicList, DynamicListCardStates, DynamicListDelegate,
        DynamicListState, EmptyState, ItemCard, Label, MarqueeSelection, MarqueeView,
        SIDEBAR_GUTTER, SIDEBAR_ITEM_GAP, SIDEBAR_ITEM_HEIGHT, SIDEBAR_MIN_WIDTH, SidebarAddButton,
        SidebarPanel, apply_reorder, marquee,
    },
    icons::Icon,
    item_manager::{DiscardDraft, ItemManager},
    selection::{
        DismissExt as _, SelectionOrder, SelectionScope, focus_item, focus_item_extending,
    },
    stores::{AppDatabaseStore, RoutineDataChanged},
};

const ROUTINE_HEIGHT: Pixels = SIDEBAR_ITEM_HEIGHT;
const STEP_HEIGHT: Pixels = px(40.);
const STEP_GAP: Pixels = px(6.);
const EMPTY_STEPS_HEIGHT: Pixels = px(52.);
const STEP_INPUT_HEIGHT: Pixels = px(40.);
const STEP_EDITOR_PADDING: Pixels = px(8.);
const STEP_DURATION_MINUTES: i64 = 5;

fn step_list_height(count: usize) -> Pixels {
    if count == 0 {
        EMPTY_STEPS_HEIGHT
    } else {
        STEP_HEIGHT * count as f32 + STEP_GAP * count.saturating_sub(1) as f32
    }
}

fn routine_editor_height(step_count: usize) -> Pixels {
    px(1.)
        + STEP_EDITOR_PADDING
        + step_list_height(step_count)
        + STEP_GAP
        + STEP_INPUT_HEIGHT
        + STEP_EDITOR_PADDING
}

fn parse_step(raw: &str) -> Option<RoutineStep> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    Some(match parser::parse_routine_step(raw) {
        Ok(parsed) if !parsed.title.trim().is_empty() => {
            let step = RoutineStep::new(parsed.title.trim());
            match parsed.duration {
                Some(duration) => step.with_duration(RelativeDuration::from(duration)),
                None => step,
            }
        }
        Ok(parsed) => {
            let step = RoutineStep::new(raw);
            match parsed.duration {
                Some(duration) => step.with_duration(RelativeDuration::from(duration)),
                None => step,
            }
        }
        Err(_) => RoutineStep::new(raw),
    })
}

fn format_step_duration(duration: RelativeDuration) -> String {
    let now = Utc::now();
    let minutes = (now + duration - now).num_minutes();
    match (minutes / 60, minutes % 60) {
        (0, minutes) => format!("{minutes}m"),
        (hours, 0) => format!("{hours}h"),
        (hours, minutes) => format!("{hours}h {minutes}m"),
    }
}

fn save_steps_later(routine_id: Uuid, steps: Vec<RoutineStep>, cx: &mut App) {
    cx.defer(move |cx| {
        AppDatabaseStore::global(cx).update(cx, |store, cx| {
            store.replace_routine_steps(routine_id, steps, cx);
        });
    });
}

#[derive(Clone)]
struct StepEntry {
    id: Uuid,
    title: String,
    duration: Option<RelativeDuration>,
}

impl StepEntry {
    fn new(step: RoutineStep) -> Self {
        Self {
            id: Uuid::now_v7(),
            title: step.title,
            duration: step.duration,
        }
    }

    fn to_step(&self) -> RoutineStep {
        let step = RoutineStep::new(self.title.clone());
        match self.duration {
            Some(duration) => step.with_duration(duration),
            None => step,
        }
    }
}

struct StepEdit {
    id: Uuid,
    input: Entity<TextInput>,
    _subscription: Subscription,
}

struct RoutineStepDelegate {
    routine_id: Uuid,
    steps: Vec<StepEntry>,
    editing: Option<StepEdit>,
}

impl Clone for RoutineStepDelegate {
    fn clone(&self) -> Self {
        Self {
            routine_id: self.routine_id,
            steps: self.steps.clone(),
            editing: None,
        }
    }
}

impl RoutineStepDelegate {
    fn new(routine: &Routine) -> Self {
        Self {
            routine_id: routine.id,
            steps: routine.steps.iter().cloned().map(StepEntry::new).collect(),
            editing: None,
        }
    }

    fn to_steps(&self) -> Vec<RoutineStep> {
        self.steps.iter().map(StepEntry::to_step).collect()
    }

    fn sync(&mut self, routine: &Routine) -> bool {
        let unchanged =
            self.steps.len() == routine.steps.len()
                && self.steps.iter().zip(&routine.steps).all(|(entry, step)| {
                    entry.title == step.title && entry.duration == step.duration
                });
        if unchanged {
            return false;
        }

        self.editing = None;
        self.steps = routine
            .steps
            .iter()
            .cloned()
            .enumerate()
            .map(|(ix, step)| StepEntry {
                id: self
                    .steps
                    .get(ix)
                    .map(|entry| entry.id)
                    .unwrap_or_else(Uuid::now_v7),
                title: step.title,
                duration: step.duration,
            })
            .collect();
        if self
            .editing
            .as_ref()
            .is_some_and(|edit| !self.steps.iter().any(|step| step.id == edit.id))
        {
            self.editing = None;
        }
        true
    }

    fn remove(&mut self, id: Uuid) {
        self.steps.retain(|step| step.id != id);
    }

    fn adjust_duration(&mut self, id: Uuid, delta: i64) {
        let Some(step) = self.steps.iter_mut().find(|step| step.id == id) else {
            return;
        };
        let now = Utc::now();
        let current = step
            .duration
            .map(|duration| (now + duration - now).num_minutes())
            .unwrap_or(0);
        let minutes = current + delta * STEP_DURATION_MINUTES;
        step.duration = (minutes > 0).then(|| RelativeDuration::minutes(minutes));
    }
}

impl DynamicListState<RoutineStepDelegate> {
    fn persist_steps(&self, cx: &mut Context<Self>) {
        save_steps_later(self.delegate().routine_id, self.delegate().to_steps(), cx);
    }

    fn begin_step_edit(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .delegate()
            .editing
            .as_ref()
            .is_some_and(|edit| edit.id == id)
        {
            return;
        }
        let Some(title) = self
            .delegate()
            .steps
            .iter()
            .find(|step| step.id == id)
            .map(|step| step.title.clone())
        else {
            return;
        };

        let input = cx.new(|cx| {
            TextInput::new(format!("routine-step.{id}"), window, cx)
                .text(title)
                .name("Step title")
                .bare(true)
                .control_size(ControlSize::Xs)
        });
        let subscription = cx.subscribe_in(
            &input,
            window,
            |list, _, event: &TextInputEvent, window, cx| match event {
                TextInputEvent::Submit | TextInputEvent::Blur => {
                    list.commit_step_edit(window, cx);
                }
                TextInputEvent::Cancel => {
                    list.delegate_mut().editing = None;
                    cx.notify();
                }
                _ => {}
            },
        );
        let focus = input.read(cx).focus_handle(cx);
        self.delegate_mut().editing = Some(StepEdit {
            id,
            input,
            _subscription: subscription,
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    fn commit_step_edit(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.delegate_mut().editing.take() else {
            return;
        };
        let value = edit.input.read(cx).value().to_string();
        let Some(parsed) = parse_step(&value) else {
            cx.notify();
            return;
        };

        self.update_items(cx, |delegate, _| {
            if let Some(step) = delegate.steps.iter_mut().find(|step| step.id == edit.id) {
                step.title = parsed.title;
                if parsed.duration.is_some() {
                    step.duration = parsed.duration;
                }
            }
        });
        self.persist_steps(cx);
    }
}

impl DynamicListDelegate for RoutineStepDelegate {
    type Item = AnyElement;

    fn items_count(&self, _cx: &App) -> usize {
        self.steps.len()
    }

    fn item_id(&self, ix: usize, _cx: &App) -> ElementId {
        self.steps
            .get(ix)
            .map(|step| ElementId::Uuid(step.id))
            .unwrap_or_else(|| ElementId::Integer(ix as u64))
    }

    fn item_height(&self, _ix: usize, _cx: &App) -> Pixels {
        STEP_HEIGHT
    }

    fn move_item(
        &mut self,
        from: usize,
        to: usize,
        _window: &mut Window,
        cx: &mut Context<DynamicListState<Self>>,
    ) {
        apply_reorder(&mut self.steps, from, to);
        save_steps_later(self.routine_id, self.to_steps(), cx);
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
        let editing_input = self
            .editing
            .as_ref()
            .filter(|edit| edit.id == id)
            .map(|edit| edit.input.clone());
        let duration = entry
            .duration
            .map(format_step_duration)
            .unwrap_or_else(|| "—".into());

        let title = match editing_input {
            Some(input) => div().flex_1().min_w_0().child(input).into_any_element(),
            None => div()
                .id(("routine-step-title", key))
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .cursor_text()
                .child(
                    Label::new(entry.title)
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .truncate(),
                )
                .on_click(cx.listener(move |list, _, window, cx| {
                    list.begin_step_edit(id, window, cx);
                }))
                .into_any_element(),
        };

        Some(
            div()
                .row()
                .size_full()
                .items_center()
                .pl_1p5()
                .pr_1()
                .gap_1()
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
                .child(title)
                .child(
                    div()
                        .row()
                        .items_center()
                        .flex_none()
                        .child(
                            Button::new(("routine-step-shorter", key))
                                .ghost()
                                .size_6()
                                .tooltip("Shorten by 5 minutes")
                                .child(Icon::new(AppIcon::Minus).size_3())
                                .on_click(cx.listener(move |list, _, _window, cx| {
                                    list.update_items(cx, |delegate, _| {
                                        delegate.adjust_duration(id, -1)
                                    });
                                    list.persist_steps(cx);
                                })),
                        )
                        .child(
                            Label::new(duration)
                                .w_10()
                                .text_xs()
                                .text_center()
                                .text_color(cx.theme().colors.text_muted),
                        )
                        .child(
                            Button::new(("routine-step-longer", key))
                                .ghost()
                                .size_6()
                                .tooltip("Add 5 minutes")
                                .child(Icon::new(AppIcon::Plus).size_3())
                                .on_click(cx.listener(move |list, _, _window, cx| {
                                    list.update_items(cx, |delegate, _| {
                                        delegate.adjust_duration(id, 1)
                                    });
                                    list.persist_steps(cx);
                                })),
                        ),
                )
                .child(
                    Button::new(("routine-step-remove", key))
                        .ghost()
                        .size_6()
                        .tooltip("Remove step")
                        .child(Icon::new(AppIcon::Close).size_3())
                        .on_click(cx.listener(move |list, _, _window, cx| {
                            list.update_items(cx, |delegate, _| delegate.remove(id));
                            list.persist_steps(cx);
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

struct RoutineStepsEditor {
    routine_id: Uuid,
    list: Entity<DynamicListState<RoutineStepDelegate>>,
    input: Entity<TextInput>,
    _subscriptions: Vec<Subscription>,
}

impl RoutineStepsEditor {
    fn new(routine: Routine, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let routine_id = routine.id;
        let list = cx.new(|cx| {
            DynamicListState::new(RoutineStepDelegate::new(&routine), window, cx)
                .gap(STEP_GAP)
                .scrollbar_visible(false)
                .elastic_overscroll(false)
        });
        let input = cx.new(|cx| {
            TextInput::new(format!("routine-step-new.{routine_id}"), window, cx)
                .placeholder("Add a step, then press Enter")
                .name("New step")
                .bare(true)
                .control_size(ControlSize::Xs)
        });
        let subscription = cx.subscribe_in(
            &input,
            window,
            |editor, _, event: &TextInputEvent, window, cx| {
                if matches!(event, TextInputEvent::Submit) {
                    editor.submit_step(window, cx);
                }
            },
        );

        Self {
            routine_id,
            list,
            input,
            _subscriptions: vec![subscription],
        }
    }

    fn sync(&mut self, routine: &Routine, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| {
            let changed = list.delegate_mut().sync(routine);
            if changed {
                list.refresh(cx);
            }
        });
    }

    fn submit_step(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let value = self.input.read(cx).value().to_string();
        let Some(step) = parse_step(&value) else {
            return;
        };

        let steps = self.list.update(cx, |list, cx| {
            list.update_items(cx, |delegate, _| {
                delegate.steps.push(StepEntry::new(step));
            });
            let last = list.delegate().steps.len().saturating_sub(1);
            list.scroll_item_into_view(last, cx);
            list.delegate().to_steps()
        });
        save_steps_later(self.routine_id, steps, cx);
        self.input
            .update(cx, |input, cx| input.set_text_quietly("", cx));
        cx.notify();
    }
}

impl Render for RoutineStepsEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.list.read(cx).delegate().steps.len();
        let list_height = step_list_height(count);
        let key = self.routine_id.as_u64_pair().1;

        div()
            .column()
            .w_full()
            .flex_1()
            .min_h_0()
            .block_mouse_except_scroll()
            .pt(STEP_EDITOR_PADDING)
            .pb(STEP_EDITOR_PADDING)
            .gap(STEP_GAP)
            .border_t_1()
            .border_color(cx.theme().colors.hairline.alpha(0.6))
            .child(
                div()
                    .h(list_height)
                    .w_full()
                    .flex_none()
                    .child(DynamicList::new(&self.list).size_full()),
            )
            .child(
                div()
                    .row()
                    .h(STEP_INPUT_HEIGHT)
                    .w_full()
                    .flex_none()
                    .items_center()
                    .px_2()
                    .gap_2()
                    .rounded(px(cx.theme().radii.control))
                    .border_1()
                    .border_color(cx.theme().colors.hairline)
                    .bg(cx.theme().colors.raised.alpha(0.45))
                    .child(
                        Icon::new(AppIcon::ListPlus)
                            .size_4()
                            .text_color(cx.theme().colors.text_muted),
                    )
                    .child(div().flex_1().min_w_0().child(self.input.clone()))
                    .child(
                        Button::new(("routine-add-step", key))
                            .ghost()
                            .size_6()
                            .tooltip("Add step")
                            .child(Icon::new(AppIcon::Plus).size_3())
                            .on_click(cx.listener(|editor, _, window, cx| {
                                editor.submit_step(window, cx);
                            })),
                    ),
            )
    }
}

#[derive(Clone)]
struct RoutinesDelegate {
    routines: Vec<Routine>,
    drafts: Vec<Routine>,
    query: String,
    items: Vec<Routine>,
    cards: DynamicListCardStates,
    expanded: HashSet<Uuid>,
    editors: HashMap<Uuid, Entity<RoutineStepsEditor>>,
    focus_handles: HashMap<Uuid, FocusHandle>,
    order: SelectionOrder,
}

impl RoutinesDelegate {
    fn new(cx: &mut App) -> Self {
        let mut this = Self {
            routines: Vec::new(),
            drafts: Vec::new(),
            query: String::new(),
            items: Vec::new(),
            cards: DynamicListCardStates::default(),
            expanded: HashSet::new(),
            editors: HashMap::new(),
            focus_handles: HashMap::new(),
            order: SelectionOrder::new(SelectionScope::Routines, []),
        };
        this.reload(cx);
        this
    }

    fn reload(&mut self, cx: &mut App) {
        let store = AppDatabaseStore::global(cx);
        if self
            .cards
            .set_generation(store.read(cx).workspace_generation())
        {
            self.drafts.clear();
            self.expanded.clear();
            self.editors.clear();
            self.focus_handles.clear();
        }
        self.routines = store.read(cx).routines().to_vec();
        self.drafts
            .retain(|draft| !self.routines.iter().any(|routine| routine.id == draft.id));
        self.rebuild(cx);

        for routine in &self.routines {
            if let Some(editor) = self.editors.get(&routine.id) {
                editor.update(cx, |editor, cx| editor.sync(routine, cx));
            }
        }
    }

    fn rebuild(&mut self, cx: &mut App) {
        self.items = self.routines.clone();
        self.items.extend(self.drafts.iter().cloned());
        let tokens = self
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect::<Vec<_>>();
        if !tokens.is_empty() {
            self.items.retain(|routine| {
                let mut text = format!(
                    "{} {}",
                    routine.title.to_lowercase(),
                    routine
                        .content
                        .as_deref()
                        .unwrap_or_default()
                        .to_lowercase()
                );
                for step in &routine.steps {
                    text.push(' ');
                    text.push_str(&step.title.to_lowercase());
                }
                tokens.iter().all(|token| text.contains(token))
            });
        }
        self.order = SelectionOrder::new(
            SelectionScope::Routines,
            self.items.iter().map(|routine| routine.id),
        );

        let current: HashSet<_> = self.items.iter().map(|routine| routine.id).collect();
        self.cards.retain(self.items.iter().filter_map(|routine| {
            routine
                .content
                .as_deref()
                .filter(|content| !content.trim().is_empty())
                .map(|_| routine.id)
        }));
        self.expanded.retain(|id| current.contains(id));
        self.editors.retain(|id, _| current.contains(id));
        self.focus_handles.retain(|id, _| current.contains(id));
        for id in current {
            self.focus_handles
                .entry(id)
                .or_insert_with(|| cx.focus_handle());
        }
    }

    fn push_draft(&mut self, routine: Routine, cx: &mut App) {
        self.drafts.push(routine);
        self.rebuild(cx);
    }

    fn discard_draft(&mut self, id: Uuid, cx: &mut App) {
        self.drafts.retain(|routine| routine.id != id);
        self.rebuild(cx);
    }

    fn index_of(&self, id: Uuid) -> Option<usize> {
        self.items.iter().position(|routine| routine.id == id)
    }

    fn focus_handle_at(&self, ix: usize) -> Option<FocusHandle> {
        let routine = self.items.get(ix)?;
        self.focus_handles.get(&routine.id).cloned()
    }

    fn toggle_expanded(
        &mut self,
        id: Uuid,
        window: &mut Window,
        cx: &mut Context<DynamicListState<Self>>,
    ) {
        if !self.expanded.insert(id) {
            self.expanded.remove(&id);
            return;
        }
        if self.editors.contains_key(&id) {
            return;
        }
        let Some(routine) = self.items.iter().find(|routine| routine.id == id).cloned() else {
            return;
        };
        let editor = cx.new(|cx| RoutineStepsEditor::new(routine, window, cx));
        self.editors.insert(id, editor);
    }
}

impl DynamicListDelegate for RoutinesDelegate {
    type Item = AnyElement;

    fn items_count(&self, _cx: &App) -> usize {
        self.items.len()
    }

    fn item_id(&self, ix: usize, _cx: &App) -> ElementId {
        self.items
            .get(ix)
            .map(|routine| ElementId::Uuid(routine.id))
            .unwrap_or_else(|| ElementId::Integer(ix as u64))
    }

    fn layout_epoch(&self) -> u64 {
        self.cards.generation()
    }

    fn prepare_layout(&mut self, window: &mut Window, cx: &mut App) {
        self.cards.animate(ROUTINE_HEIGHT, window, cx);
    }

    fn pause_layout(&mut self) {
        self.cards.pause();
    }

    fn item_height(&self, ix: usize, _cx: &App) -> Pixels {
        self.items.get(ix).map_or(ROUTINE_HEIGHT, |routine| {
            self.cards.height(routine.id, ROUTINE_HEIGHT)
                + if self.expanded.contains(&routine.id) {
                    routine_editor_height(routine.steps.len())
                } else {
                    px(0.)
                }
        })
    }

    fn can_drag(&self, _ix: usize, _cx: &App) -> bool {
        false
    }

    fn move_item(
        &mut self,
        from: usize,
        to: usize,
        _window: &mut Window,
        cx: &mut Context<DynamicListState<Self>>,
    ) {
        if self.routines.len() < 2 || from >= self.routines.len() {
            return;
        }
        let to = to.min(self.routines.len() - 1);
        apply_reorder(&mut self.routines, from, to);
        self.rebuild(cx);
        let order = self.routines.iter().map(|routine| routine.id).collect();
        cx.defer(move |cx| {
            AppDatabaseStore::global(cx).update(cx, |store, cx| {
                store.reorder_routines(order, cx);
            });
        });
    }

    fn render_item(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<DynamicListState<Self>>,
    ) -> Option<Self::Item> {
        let routine = self.items.get(ix)?.clone();
        let routine_id = routine.id;
        let key = routine_id.as_u64_pair().1;
        let expanded = self.expanded.contains(&routine_id);
        let editor = self.editors.get(&routine_id).cloned();
        let meta: Option<SharedString> = Some(match routine.steps.len() {
            0 => "No steps".into(),
            1 => "1 step".into(),
            count => format!("{count} steps").into(),
        });
        let handle = self
            .focus_handles
            .get(&routine_id)
            .cloned()
            .unwrap_or_else(|| cx.focus_handle());
        let item = AnyItem::Routine(routine);
        let (is_editing, is_draft) = {
            let manager = ItemManager::global(cx);
            let manager = manager.read(cx);
            (
                manager.is_being_edited(routine_id),
                manager.is_draft(routine_id),
            )
        };
        let reorder_handle = (!expanded && !is_draft).then(|| {
            DynamicListState::<Self>::reorder_handle(
                ix,
                ElementId::Uuid(routine_id),
                div()
                    .flex()
                    .size_6()
                    .items_center()
                    .justify_center()
                    .text_color(cx.theme().colors.text_muted)
                    .child(Icon::new(AppIcon::ListOrdered).size_3()),
                cx,
            )
        });

        let trailing = div()
            .row()
            .items_center()
            .gap_1()
            .children(reorder_handle)
            .child(
                Button::new(("routine-start", key))
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(AppIcon::Play))
                    .tooltip("Start this routine now")
                    .text_color(cx.theme().colors.text_muted)
                    .block_mouse_except_scroll()
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        AppDatabaseStore::global(cx).update(cx, |store, cx| {
                            store.instantiate_routine(routine_id, None, cx);
                        });
                    }),
            )
            .child(
                Button::new(("routine-expand", key))
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(if expanded {
                        AppIcon::ListChevronsDownUp
                    } else {
                        AppIcon::ChevronRight
                    }))
                    .tooltip(if expanded { "Hide steps" } else { "Show steps" })
                    .text_color(cx.theme().colors.text_muted)
                    .block_mouse_except_scroll()
                    .on_click(cx.listener(move |list, _, window, cx| {
                        cx.stop_propagation();
                        list.update_items(cx, |delegate, cx| {
                            delegate.toggle_expanded(routine_id, window, cx)
                        });
                    })),
            );

        Some(
            div()
                .column()
                .size_full()
                .px(SIDEBAR_GUTTER)
                .child(
                    ItemCard::new(&item, meta, window, cx)
                        .details(self.cards.get(routine_id))
                        .schedule_navigation()
                        .with_focus_handle(handle)
                        .selectable(self.order.clone())
                        .w_full()
                        .h(self.cards.height(routine_id, ROUTINE_HEIGHT))
                        .flex_none()
                        .border(false)
                        .trailing(trailing)
                        .draggable(!is_editing && !is_draft, None)
                        .block_mouse_except_scroll()
                        .on_key_down(cx.listener(move |list, event: &KeyDownEvent, window, cx| {
                            if event.is_held {
                                return;
                            }
                            let key = event.keystroke.key.as_str();
                            let extend_selection =
                                event.keystroke.modifiers.shift && matches!(key, "up" | "down");
                            match key {
                                "up" | "k" if !is_editing => {
                                    cx.stop_propagation();
                                    focus_sibling(
                                        list,
                                        routine_id,
                                        -1,
                                        extend_selection,
                                        window,
                                        cx,
                                    );
                                }
                                "down" | "j" if !is_editing => {
                                    cx.stop_propagation();
                                    focus_sibling(
                                        list,
                                        routine_id,
                                        1,
                                        extend_selection,
                                        window,
                                        cx,
                                    );
                                }
                                "enter" if !is_editing => {
                                    cx.stop_propagation();
                                    ItemManager::global(cx).update(cx, |manager, cx| {
                                        manager.begin_edit(&item, false, window, cx);
                                    });
                                    cx.notify();
                                }
                                _ => {}
                            }
                        })),
                )
                .when(expanded, |this| this.children(editor))
                .into_any_element(),
        )
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<DynamicListState<Self>>,
    ) -> impl IntoElement {
        if self.query.trim().is_empty() {
            EmptyState::new(Icon::new(AppIcon::Repeat), "No routines")
        } else {
            EmptyState::new(Icon::new(AppIcon::Repeat), "No matches")
        }
    }
}

fn focus_sibling(
    list: &mut DynamicListState<RoutinesDelegate>,
    id: Uuid,
    offset: isize,
    extend_selection: bool,
    window: &mut Window,
    cx: &mut Context<DynamicListState<RoutinesDelegate>>,
) {
    let delegate = list.delegate();
    let Some(pos) = delegate.index_of(id) else {
        return;
    };
    let Some(next) = pos
        .checked_add_signed(offset)
        .filter(|next| *next < delegate.items.len())
    else {
        return;
    };
    let Some(handle) = delegate.focus_handle_at(next) else {
        return;
    };
    let Some(next_id) = delegate.items.get(next).map(|routine| routine.id) else {
        return;
    };
    let order = delegate.order.clone();

    if extend_selection {
        focus_item_extending(&order, id, next_id, &handle, window, cx);
    } else {
        focus_item(SelectionScope::Routines, next_id, &handle, window, cx);
    }
    list.scroll_item_into_view(next, cx);
}

pub struct RoutinesView {
    list: Entity<DynamicListState<RoutinesDelegate>>,
    focus_handle: FocusHandle,
    marquee: MarqueeSelection,
}

impl RoutinesView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let list = cx.new(|cx| {
            DynamicListState::new(RoutinesDelegate::new(cx), window, cx)
                .gap(SIDEBAR_ITEM_GAP)
                .content_inset_top(crate::views::LIBRARY_HEADER_HEIGHT + SIDEBAR_ITEM_GAP)
                .scrollbar_visible(false)
        });

        let db_store = AppDatabaseStore::global(cx);
        cx.subscribe(&db_store, |view, _, _: &RoutineDataChanged, cx| {
            view.list.update(cx, |list, cx| {
                list.update_items(cx, |delegate, cx| delegate.reload(cx));
            });
        })
        .detach();

        let item_manager = ItemManager::global(cx);
        cx.observe(&item_manager, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&item_manager, |view, _, event: &DiscardDraft, cx| {
            view.list.update(cx, |list, cx| {
                list.update_items(cx, |delegate, cx| delegate.discard_draft(event.0, cx));
            });
        })
        .detach();

        Self {
            list,
            focus_handle: cx.focus_handle(),
            marquee: MarqueeSelection::new(SelectionScope::Routines),
        }
    }

    pub(crate) fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    pub(crate) fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| {
            list.update_items(cx, |delegate, cx| {
                if delegate.query != query {
                    delegate.query = query.to_owned();
                    delegate.rebuild(cx);
                }
            });
        });
        cx.notify();
    }

    fn add_draft_routine(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let routine = Routine::new("");
        let id = routine.id;
        let item = AnyItem::Routine(routine.clone());
        self.list.update(cx, |list, cx| {
            list.update_items(cx, |delegate, cx| delegate.push_draft(routine, cx));
            if let Some(ix) = list.delegate().index_of(id) {
                list.scroll_item_into_view(ix, cx);
            }
        });
        ItemManager::global(cx).update(cx, |manager, cx| {
            manager.begin_edit(&item, true, window, cx);
        });
        cx.notify();
    }
}

fn routines_context_menu(view: Entity<RoutinesView>) -> crate::components::menu::MenuBuilder {
    crate::components::menu::MenuBuilder::new()
        .label("Routines")
        .item("New routine", move |window, cx| {
            view.update(cx, |view, cx| view.add_draft_routine(window, cx));
        })
}

impl MarqueeView for RoutinesView {
    fn marquee(&self) -> &MarqueeSelection {
        &self.marquee
    }

    fn marquee_mut(&mut self) -> &mut MarqueeSelection {
        &mut self.marquee
    }

    fn marquee_focus(&self, _cx: &App) -> Option<FocusHandle> {
        Some(self.focus_handle.clone())
    }
}

impl Render for RoutinesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.list.update(cx, |list, cx| {
            list.scroll_marquee(&mut self.marquee, window, cx);
        });
        let entity = cx.entity();
        let body = SidebarPanel::new("routines-backdrop")
            .footer(
                SidebarAddButton::new("new-routine", "New routine")
                    .tooltip("Add a routine")
                    .on_click({
                        let entity = entity.clone();
                        move |_event, window, cx| {
                            entity.update(cx, |view, cx| {
                                view.add_draft_routine(window, cx);
                            });
                        }
                    }),
            )
            .on_double_click({
                let entity = entity.clone();
                move |_event: &ClickEvent, window, cx| {
                    entity.update(cx, |view, cx| {
                        view.add_draft_routine(window, cx);
                    });
                }
            })
            .on_aux_click(cx.listener(move |_view, event: &ClickEvent, window, cx| {
                if event.is_right_click() {
                    crate::components::menu::open_context_menu(
                        routines_context_menu(cx.entity().clone()),
                        event.position(),
                        window,
                        cx,
                    );
                    cx.notify();
                }
            }))
            .child(
                div()
                    .size_full()
                    .min_w(SIDEBAR_MIN_WIDTH)
                    .child(DynamicList::new(&self.list).size_full()),
            );

        marquee(body, self, cx)
            .track_focus(&self.focus_handle)
            .on_dismiss(SelectionScope::Routines, Some(self.focus_handle.clone()))
    }
}
