use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    time::Duration,
};

use chrono::{DateTime, Local, NaiveDate, Utc};
use gpui::{
    AnyElement, App, AppContext as _, AsyncApp, Context, ElementId, Entity, FocusHandle,
    InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Pixels, Render, Styled, Window,
    div,
};
use gpui_kit::foundation::StyledExt as _;
use subroutine_core::{ActionTemplate, AnyItem, EventTemplate};
use uuid::Uuid;

use crate::{
    components::{
        DraggedItems, DynamicList, DynamicListCardStates, DynamicListDelegate, DynamicListState,
        EmptyState, ItemCard, MarqueeSelection, MarqueeView, SIDEBAR_GUTTER, SIDEBAR_ITEM_GAP,
        SIDEBAR_ITEM_HEIGHT, marquee,
    },
    icons::Icon,
    item_manager::ItemManager,
    item_subject::SavedItem,
    selection::{
        DismissExt as _, SelectionManager, SelectionOrder, SelectionScope, focus_item,
        focus_item_extending,
    },
    stores::{AppDatabaseStore, DataChanged},
};

const RESULT_HEIGHT: Pixels = SIDEBAR_ITEM_HEIGHT;
const RESULT_GAP: Pixels = SIDEBAR_ITEM_GAP;

const RECENT_COMPLETION_WINDOW_DAYS: i64 = 7;
pub(crate) const RECENT_COMPLETION_LABEL: &str = "Recently completed (last 7 days)";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SourceKind {
    Unqueued,
    Overdue,
    RecentlyCompleted,
    Completed,
    Action,
    Event,
    Routine,
    Marker,
    Signal,
    SavedAction,
    SavedEvent,
}

impl SourceKind {
    pub(crate) const ALL: [Self; 11] = [
        Self::Unqueued,
        Self::Overdue,
        Self::RecentlyCompleted,
        Self::Completed,
        Self::Action,
        Self::Event,
        Self::Routine,
        Self::Marker,
        Self::Signal,
        Self::SavedAction,
        Self::SavedEvent,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Unqueued => "Unqueued actions",
            Self::Overdue => "Overdue actions",
            Self::RecentlyCompleted => RECENT_COMPLETION_LABEL,
            Self::Completed => "All completed actions",
            Self::Action => "Actions",
            Self::Event => "Events",
            Self::Routine => "Routines",
            Self::Marker => "Markers",
            Self::Signal => "Signals",
            Self::SavedAction => "Saved actions",
            Self::SavedEvent => "Saved events",
        }
    }

    pub(crate) fn group(self) -> &'static str {
        match self {
            Self::Unqueued | Self::Overdue | Self::RecentlyCompleted | Self::Completed => {
                "Smart lists"
            }
            Self::SavedAction | Self::SavedEvent => "Saved",
            _ => "Live",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SourceSort {
    #[default]
    TimeAscending,
    TimeDescending,
    TitleAscending,
    TitleDescending,
    ItemType,
    BestMatch,
}

impl SourceSort {
    pub(crate) const ALL: [Self; 6] = [
        Self::TimeAscending,
        Self::TimeDescending,
        Self::TitleAscending,
        Self::TitleDescending,
        Self::ItemType,
        Self::BestMatch,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::TimeAscending => "Time: earliest first",
            Self::TimeDescending => "Time: latest first",
            Self::TitleAscending => "Title: A–Z",
            Self::TitleDescending => "Title: Z–A",
            Self::ItemType => "Item type",
            Self::BestMatch => "Best match",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct MatchScore {
    phrase_rank: u8,
    token_rank: usize,
}

#[derive(Clone)]
enum SourceEntry {
    Live(AnyItem),
    SavedAction(ActionTemplate),
    SavedEvent(EventTemplate),
}

impl SourceEntry {
    fn id(&self) -> Uuid {
        match self {
            Self::Live(item) => item.id(),
            Self::SavedAction(template) => template.id,
            Self::SavedEvent(template) => template.id,
        }
    }

    fn title(&self) -> &str {
        match self {
            Self::Live(item) => item.title(),
            Self::SavedAction(template) => &template.title,
            Self::SavedEvent(template) => &template.title,
        }
    }

    fn content(&self) -> Option<&str> {
        match self {
            Self::Live(item) => match item {
                AnyItem::Action(item) => item.content.as_deref(),
                AnyItem::Event(item) => item.content.as_deref(),
                AnyItem::Routine(item) => item.content.as_deref(),
                AnyItem::Marker(item) => item.content.as_deref(),
                AnyItem::Signal(item) => item.content.as_deref(),
                AnyItem::ActionTemplate(item) => item.content.as_deref(),
                AnyItem::EventTemplate(item) => item.content.as_deref(),
            },
            Self::SavedAction(template) => template.content.as_deref(),
            Self::SavedEvent(template) => template.content.as_deref(),
        }
    }

    fn kind(&self) -> SourceKind {
        match self {
            Self::Live(AnyItem::Action(_)) => SourceKind::Action,
            Self::Live(AnyItem::Event(_)) => SourceKind::Event,
            Self::Live(AnyItem::Routine(_)) => SourceKind::Routine,
            Self::Live(AnyItem::Marker(_)) => SourceKind::Marker,
            Self::Live(AnyItem::Signal(_)) => SourceKind::Signal,
            Self::Live(AnyItem::ActionTemplate(_)) => SourceKind::SavedAction,
            Self::Live(AnyItem::EventTemplate(_)) => SourceKind::SavedEvent,
            Self::SavedAction(_) => SourceKind::SavedAction,
            Self::SavedEvent(_) => SourceKind::SavedEvent,
        }
    }

    fn routine_steps(&self) -> Vec<String> {
        match self {
            Self::Live(AnyItem::Routine(routine)) => routine
                .steps
                .iter()
                .map(|step| normalize(&step.title))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn materialized_item(&self) -> AnyItem {
        match self {
            Self::Live(item) => item.clone(),
            Self::SavedAction(template) => AnyItem::Action(template.clone().build()),
            Self::SavedEvent(template) => AnyItem::Event(template.clone().build(Utc::now())),
        }
    }

    fn is_saved(&self) -> bool {
        matches!(self, Self::SavedAction(_) | Self::SavedEvent(_))
    }
}

struct SearchText {
    title: String,
    content: Option<String>,
    routine_steps: Vec<String>,
}

impl SearchText {
    fn from_entry(entry: &SourceEntry) -> Self {
        Self {
            title: normalize(entry.title()),
            content: entry.content().map(normalize),
            routine_steps: entry.routine_steps(),
        }
    }

    fn token_rank(&self, token: &str) -> Option<usize> {
        if self.title.starts_with(token) {
            Some(0)
        } else if self.title.contains(token) {
            Some(1)
        } else if self
            .content
            .as_deref()
            .is_some_and(|content| content.contains(token))
        {
            Some(2)
        } else if self.routine_steps.iter().any(|step| step.contains(token)) {
            Some(3)
        } else {
            None
        }
    }
}

fn normalize(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn match_score(entry: &SourceEntry, query: &str) -> Option<MatchScore> {
    let query = normalize(query);
    if query.is_empty() {
        return Some(MatchScore {
            phrase_rank: 0,
            token_rank: 0,
        });
    }

    let text = SearchText::from_entry(entry);
    let mut token_rank = 0;
    for token in query.split_whitespace() {
        token_rank += text.token_rank(token)?;
    }

    let phrase_rank = if text.title == query {
        0
    } else if text.title.starts_with(&query) {
        1
    } else if text.title.contains(&query) {
        2
    } else if text
        .content
        .as_deref()
        .is_some_and(|content| content.contains(&query))
    {
        3
    } else if text.routine_steps.iter().any(|step| step.contains(&query)) {
        4
    } else {
        5
    };

    Some(MatchScore {
        phrase_rank,
        token_rank,
    })
}

fn kind_rank(kind: SourceKind) -> u8 {
    SourceKind::ALL
        .iter()
        .position(|candidate| *candidate == kind)
        .unwrap_or(SourceKind::ALL.len()) as u8
}

fn search_selection_ids(entries: &[SourceEntry]) -> Vec<Uuid> {
    entries.iter().map(SourceEntry::id).collect()
}

fn source_drag_payload(entries: &[&SourceEntry], primary_id: Uuid) -> Option<DraggedItems> {
    let mut primary = None;
    let mut items = Vec::new();
    let mut saved_item_ids = Vec::new();
    let mut materialized_item_ids = Vec::new();
    for entry in entries {
        let item = entry.materialized_item();
        if entry.id() == primary_id {
            primary = Some(item.clone());
        }
        if entry.is_saved() {
            saved_item_ids.push(entry.id());
            materialized_item_ids.push(item.id());
        }
        items.push(item);
    }
    Some(DraggedItems {
        primary: primary?,
        source_anchor: None,
        items,
        saved_item_ids: (!saved_item_ids.is_empty()).then_some(saved_item_ids),
        materialized_item_ids,
    })
}

fn live_kind_label(item: &AnyItem) -> &'static str {
    match item {
        AnyItem::Action(_) => "Action",
        AnyItem::Event(_) => "Event",
        AnyItem::Routine(_) => "Routine",
        AnyItem::Marker(_) => "Marker",
        AnyItem::Signal(_) => "Signal",
        AnyItem::ActionTemplate(_) => "Saved action",
        AnyItem::EventTemplate(_) => "Saved event",
    }
}

fn occurs_today_or_later(entry: &SourceEntry, today: NaiveDate) -> bool {
    match entry {
        SourceEntry::Live(AnyItem::Event(event)) => {
            event.end_time().with_timezone(&Local).date_naive() >= today
        }
        SourceEntry::Live(AnyItem::Marker(marker)) => {
            marker.end_date.unwrap_or(marker.date) >= today
        }
        SourceEntry::Live(AnyItem::Signal(signal)) => {
            signal.datetime.with_timezone(&Local).date_naive() >= today
        }
        _ => false,
    }
}

fn is_recent_completion(action: &subroutine_core::Action, now: DateTime<Utc>) -> bool {
    let Some(completed_at) = action.completion else {
        return false;
    };
    let cutoff = now - chrono::Duration::days(RECENT_COMPLETION_WINDOW_DAYS);
    completed_at >= cutoff && completed_at <= now
}

fn matches_filter(
    entry: &SourceEntry,
    filters: &[SourceKind],
    today: NaiveDate,
    now: DateTime<Utc>,
) -> bool {
    let completed_action = match entry {
        SourceEntry::Live(AnyItem::Action(action)) if action.is_completed() => Some(action),
        _ => None,
    };
    if filters.is_empty() {
        return completed_action.is_none();
    }
    if let Some(action) = completed_action {
        return filters.contains(&SourceKind::Completed)
            || (filters.contains(&SourceKind::RecentlyCompleted)
                && is_recent_completion(action, now));
    }

    let overdue = filters.contains(&SourceKind::Overdue)
        && matches!(entry, SourceEntry::Live(AnyItem::Action(action)) if action.is_overdue(now));
    let temporal_kind = matches!(
        entry.kind(),
        SourceKind::Event | SourceKind::Marker | SourceKind::Signal
    );

    overdue
        || (filters.contains(&entry.kind())
            && (!temporal_kind || occurs_today_or_later(entry, today)))
        || (filters.contains(&SourceKind::Unqueued)
            && matches!(entry, SourceEntry::Live(AnyItem::Action(action)) if !action.queued))
}

struct RankedEntry {
    score: MatchScore,
    title: String,
    kind: u8,
    time: Option<DateTime<Utc>>,
    id: Uuid,
    entry: SourceEntry,
}

fn entry_time(entry: &SourceEntry) -> Option<DateTime<Utc>> {
    match entry {
        SourceEntry::Live(item) => item
            .start_datetime()
            .map(|datetime| datetime.with_timezone(&Utc)),
        SourceEntry::SavedAction(_) | SourceEntry::SavedEvent(_) => None,
    }
}

fn completion_time(entry: &SourceEntry) -> Option<DateTime<Utc>> {
    match entry {
        SourceEntry::Live(AnyItem::Action(action)) => action.completion,
        _ => None,
    }
}

fn compare_time(
    left: Option<DateTime<Utc>>,
    right: Option<DateTime<Utc>>,
    descending: bool,
) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) if descending => right.cmp(&left),
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_ranked(left: &RankedEntry, right: &RankedEntry, sort: SourceSort) -> Ordering {
    let stable = || {
        left.title
            .cmp(&right.title)
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.id.cmp(&right.id))
    };

    match sort {
        SourceSort::TimeAscending => compare_time(left.time, right.time, false).then_with(stable),
        SourceSort::TimeDescending => compare_time(left.time, right.time, true).then_with(stable),
        SourceSort::TitleAscending => stable(),
        SourceSort::TitleDescending => right
            .title
            .cmp(&left.title)
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.id.cmp(&right.id)),
        SourceSort::ItemType => left
            .kind
            .cmp(&right.kind)
            .then_with(|| compare_time(left.time, right.time, false))
            .then_with(|| left.title.cmp(&right.title))
            .then_with(|| left.id.cmp(&right.id)),
        SourceSort::BestMatch => left.score.cmp(&right.score).then_with(stable),
    }
}

fn matching_entries_sorted_at(
    entries: &[SourceEntry],
    filters: &[SourceKind],
    query: &str,
    sort: SourceSort,
    today: NaiveDate,
    now: DateTime<Utc>,
) -> Vec<SourceEntry> {
    let recent_completion_route = matches!(filters, [SourceKind::RecentlyCompleted]);
    let effective_sort = if recent_completion_route {
        SourceSort::TimeDescending
    } else {
        sort
    };
    let mut matches: Vec<_> = entries
        .iter()
        .filter(|entry| matches_filter(entry, filters, today, now))
        .filter_map(|entry| {
            match_score(entry, query).map(|score| RankedEntry {
                score,
                title: normalize(entry.title()),
                kind: kind_rank(entry.kind()),
                time: if recent_completion_route {
                    completion_time(entry)
                } else {
                    entry_time(entry)
                },
                id: entry.id(),
                entry: entry.clone(),
            })
        })
        .collect();

    matches.sort_by(|left, right| compare_ranked(left, right, effective_sort));
    matches.into_iter().map(|ranked| ranked.entry).collect()
}

fn search_is_explicit(filters: &[SourceKind], query: &str) -> bool {
    !filters.is_empty() || !normalize(query).is_empty()
}

fn matching_entries_with_sort(
    entries: &[SourceEntry],
    filters: &[SourceKind],
    query: &str,
    sort: SourceSort,
) -> Vec<SourceEntry> {
    let now = Utc::now();
    matching_entries_sorted_at(
        entries,
        filters,
        query,
        sort,
        now.with_timezone(&Local).date_naive(),
        now,
    )
}

fn source_entries(store: &AppDatabaseStore) -> Vec<SourceEntry> {
    store
        .all_items()
        .into_iter()
        .map(SourceEntry::Live)
        .chain(
            store
                .action_templates()
                .into_iter()
                .map(SourceEntry::SavedAction),
        )
        .chain(
            store
                .event_templates()
                .into_iter()
                .map(SourceEntry::SavedEvent),
        )
        .collect()
}

#[derive(Clone)]
struct SearchDelegate {
    all_entries: Vec<SourceEntry>,
    filters: Vec<SourceKind>,
    sort: SourceSort,
    query: String,
    results: Vec<SourceEntry>,
    cards: DynamicListCardStates,
    focus_handles: HashMap<Uuid, FocusHandle>,
    order: SelectionOrder,
}

impl SearchDelegate {
    fn new(all_entries: Vec<SourceEntry>, cx: &mut App) -> Self {
        let mut this = Self {
            all_entries,
            filters: Vec::new(),
            sort: SourceSort::default(),
            query: String::new(),
            results: Vec::new(),
            cards: DynamicListCardStates::default(),
            focus_handles: HashMap::new(),
            order: SelectionOrder::new(SelectionScope::Search, []),
        };
        this.rebuild(cx);
        this
    }

    fn set_query(&mut self, query: String, cx: &mut App) {
        if self.query == query {
            return;
        }
        self.query = query;
        self.rebuild(cx);
    }

    fn set_filters(&mut self, filters: Vec<SourceKind>, cx: &mut App) {
        if self.filters == filters {
            return;
        }
        self.filters = filters;
        self.rebuild(cx);
    }

    fn set_sort(&mut self, sort: SourceSort, cx: &mut App) {
        if self.sort == sort {
            return;
        }
        self.sort = sort;
        self.rebuild(cx);
    }

    fn reload(&mut self, all_entries: Vec<SourceEntry>, cx: &mut App) {
        self.all_entries = all_entries;
        self.rebuild(cx);
    }

    fn rebuild(&mut self, cx: &mut App) {
        if self
            .cards
            .set_generation(AppDatabaseStore::global(cx).read(cx).workspace_generation())
        {
            self.focus_handles.clear();
        }
        self.results = if search_is_explicit(&self.filters, &self.query) {
            matching_entries_with_sort(&self.all_entries, &self.filters, &self.query, self.sort)
        } else {
            Vec::new()
        };
        self.order =
            SelectionOrder::new(SelectionScope::Search, search_selection_ids(&self.results));

        let current: HashSet<_> = self.results.iter().map(SourceEntry::id).collect();
        self.cards.retain(self.results.iter().filter_map(|entry| {
            entry
                .content()
                .filter(|content| !content.trim().is_empty())
                .map(|_| entry.id())
        }));
        self.focus_handles.retain(|id, _| current.contains(id));
        for id in current {
            self.focus_handles
                .entry(id)
                .or_insert_with(|| cx.focus_handle());
        }
    }

    fn index_of(&self, id: Uuid) -> Option<usize> {
        self.results.iter().position(|entry| entry.id() == id)
    }

    fn focus_target(&self, ix: usize) -> Option<(SelectionScope, Uuid, FocusHandle)> {
        let entry = self.results.get(ix)?;
        Some((
            SelectionScope::Search,
            entry.id(),
            self.focus_handles.get(&entry.id())?.clone(),
        ))
    }

    fn visible_ids(&self) -> HashSet<Uuid> {
        self.results.iter().map(SourceEntry::id).collect()
    }

    fn selected_ids_for(&self, primary_id: Uuid, cx: &App) -> Vec<Uuid> {
        let selection = SelectionManager::global(cx);
        let selection = selection.read(cx);
        if selection.is_selected_in(SelectionScope::Search, primary_id) {
            let selected: Vec<_> = self
                .order
                .ids()
                .iter()
                .copied()
                .filter(|id| selection.ids().contains(id))
                .collect();
            if !selected.is_empty() {
                return selected;
            }
        }
        vec![primary_id]
    }

    fn entry(&self, id: Uuid) -> Option<&SourceEntry> {
        self.results.iter().find(|entry| entry.id() == id)
    }

    fn drag_payload(&self, primary_id: Uuid, cx: &App) -> Option<DraggedItems> {
        let ids = self.selected_ids_for(primary_id, cx);
        let entries: Vec<_> = ids.iter().filter_map(|id| self.entry(*id)).collect();
        source_drag_payload(&entries, primary_id)
    }

    fn selected_saved_items(&self, primary_id: Uuid, cx: &App) -> Vec<AnyItem> {
        self.selected_ids_for(primary_id, cx)
            .into_iter()
            .filter_map(|id| {
                self.entry(id)
                    .filter(|entry| entry.is_saved())
                    .map(SourceEntry::materialized_item)
            })
            .collect()
    }
}

impl DynamicListDelegate for SearchDelegate {
    type Item = AnyElement;

    fn items_count(&self, _cx: &App) -> usize {
        self.results.len()
    }

    fn item_id(&self, ix: usize, _cx: &App) -> ElementId {
        self.results
            .get(ix)
            .map(SourceEntry::id)
            .map(ElementId::Uuid)
            .unwrap_or_else(|| ElementId::Integer(ix as u64))
    }

    fn layout_epoch(&self) -> u64 {
        self.cards.generation()
    }

    fn prepare_layout(&mut self, window: &mut Window, cx: &mut App) {
        self.cards.animate(RESULT_HEIGHT, window, cx);
    }

    fn pause_layout(&mut self) {
        self.cards.pause();
    }

    fn item_height(&self, ix: usize, _cx: &App) -> Pixels {
        self.results.get(ix).map_or(RESULT_HEIGHT, |entry| {
            self.cards.height(entry.id(), RESULT_HEIGHT)
        })
    }

    fn can_drag(&self, _ix: usize, _cx: &App) -> bool {
        false
    }

    fn move_item(
        &mut self,
        _from: usize,
        _to: usize,
        _window: &mut Window,
        _cx: &mut Context<DynamicListState<Self>>,
    ) {
    }

    fn render_item(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<DynamicListState<Self>>,
    ) -> Option<Self::Item> {
        let entry = self.results.get(ix)?.clone();
        let id = entry.id();
        let focus_handle = self.focus_handles.get(&id)?.clone();

        match entry {
            SourceEntry::Live(item) => {
                let order = self.order.clone();
                let drag_payload = self.drag_payload(id, cx)?;
                let meta = Some(live_kind_label(&item).into());
                Some(
                    div()
                        .size_full()
                        .px(SIDEBAR_GUTTER)
                        .child(
                            ItemCard::new_with_id(
                                ("source-result", item.id_u64()),
                                &item,
                                meta,
                                window,
                                cx,
                            )
                            .details(self.cards.get(id))
                            .schedule_navigation()
                            .with_focus_handle(focus_handle)
                            .selectable(order)
                            .size_full()
                            .border(false)
                            .drag_payload(drag_payload)
                            .block_mouse_except_scroll()
                            .on_key_down(cx.listener(
                                move |list, event: &KeyDownEvent, window, cx| {
                                    if event.is_held
                                        || ItemManager::global(cx).read(cx).is_being_edited(id)
                                    {
                                        return;
                                    }
                                    let key = event.keystroke.key.as_str();
                                    let offset = match key {
                                        "up" | "k" => -1,
                                        "down" | "j" => 1,
                                        _ => return,
                                    };
                                    let extend_selection = event.keystroke.modifiers.shift
                                        && matches!(key, "up" | "down");
                                    cx.stop_propagation();
                                    focus_sibling(list, id, offset, extend_selection, window, cx);
                                },
                            )),
                        )
                        .into_any_element(),
                )
            }
            saved @ (SourceEntry::SavedAction(_) | SourceEntry::SavedEvent(_)) => {
                let saved_item = match &saved {
                    SourceEntry::SavedAction(template) => SavedItem::Action(template.clone()),
                    SourceEntry::SavedEvent(template) => SavedItem::Event(template.clone()),
                    SourceEntry::Live(_) => unreachable!(),
                };
                let saved_kind = match saved {
                    SourceEntry::SavedAction(_) => "Saved action",
                    SourceEntry::SavedEvent(_) => "Saved event",
                    SourceEntry::Live(_) => unreachable!(),
                };
                let drag_payload = self.drag_payload(id, cx)?;
                let order = self.order.clone();
                let keyboard_items = self.selected_saved_items(id, cx);
                let item: AnyItem = saved_item.into();

                Some(
                    div()
                        .size_full()
                        .px(SIDEBAR_GUTTER)
                        .on_key_down(cx.listener(move |list, event: &KeyDownEvent, window, cx| {
                            if event.is_held {
                                return;
                            }
                            let key = event.keystroke.key.as_str();
                            let extend_selection =
                                event.keystroke.modifiers.shift && matches!(key, "up" | "down");
                            match key {
                                "up" | "k" => {
                                    cx.stop_propagation();
                                    focus_sibling(list, id, -1, extend_selection, window, cx);
                                }
                                "down" | "j" => {
                                    cx.stop_propagation();
                                    focus_sibling(list, id, 1, extend_selection, window, cx);
                                }
                                "enter" => {
                                    cx.stop_propagation();
                                    AppDatabaseStore::global(cx).update(cx, |store, cx| {
                                        store.create_items(keyboard_items.clone(), cx);
                                    });
                                }
                                _ => {}
                            }
                        }))
                        .child(
                            ItemCard::new_with_id(
                                ("source-saved-result", id.as_u64_pair().1),
                                &item,
                                Some(saved_kind.into()),
                                window,
                                cx,
                            )
                            .details(self.cards.get(id))
                            .focus_handle(focus_handle)
                            .selectable(order)
                            .drag_payload(drag_payload)
                            .title_only(true)
                            .border(false)
                            .size_full(),
                        )
                        .into_any_element(),
                )
            }
        }
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<DynamicListState<Self>>,
    ) -> impl IntoElement {
        let title = match (normalize(&self.query).is_empty(), self.filters.as_slice()) {
            (true, []) => "Search your items",
            (true, [SourceKind::RecentlyCompleted]) => "No completions in the last 7 days",
            (true, [_]) => "Nothing matches this filter",
            (true, _) => "Nothing matches these filters",
            (false, _) => "No matches",
        };
        EmptyState::new(Icon::from_path("icons/regular/magnifying-glass.svg"), title)
    }
}

fn focus_sibling(
    list: &mut DynamicListState<SearchDelegate>,
    id: Uuid,
    offset: isize,
    extend_selection: bool,
    window: &mut Window,
    cx: &mut Context<DynamicListState<SearchDelegate>>,
) {
    let delegate = list.delegate();
    let target = if extend_selection {
        let scope = SelectionScope::Search;
        let order = &delegate.order;
        let Some(next_id) = order
            .ids()
            .iter()
            .position(|candidate| *candidate == id)
            .and_then(|position| position.checked_add_signed(offset))
            .filter(|next| *next < order.ids().len())
            .map(|next| order.ids()[next])
        else {
            return;
        };
        let Some(next) = delegate.index_of(next_id) else {
            return;
        };
        let Some((_, _, handle)) = delegate.focus_target(next) else {
            return;
        };
        Some((next, scope, next_id, handle, Some(order.clone())))
    } else {
        let Some(next) = delegate
            .index_of(id)
            .and_then(|position| position.checked_add_signed(offset))
            .filter(|next| *next < delegate.results.len())
        else {
            return;
        };
        let Some((scope, next_id, handle)) = delegate.focus_target(next) else {
            return;
        };
        Some((next, scope, next_id, handle, None))
    };
    let Some((next, scope, next_id, handle, order)) = target else {
        return;
    };

    if let Some(order) = order {
        focus_item_extending(&order, id, next_id, &handle, window, cx);
    } else {
        focus_item(scope, next_id, &handle, window, cx);
    }
    list.scroll_item_into_view(next, cx);
}

pub struct SearchView {
    list: Entity<DynamicListState<SearchDelegate>>,
    focus_handle: FocusHandle,
    marquee: MarqueeSelection,
}

impl SearchView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = AppDatabaseStore::global(cx);
        let entries = source_entries(store.read(cx));
        let list = cx.new(|cx| {
            DynamicListState::new(SearchDelegate::new(entries, cx), window, cx)
                .gap(RESULT_GAP)
                .content_inset_top(crate::views::SEARCH_HEADER_HEIGHT + RESULT_GAP)
                .scrollbar_visible(false)
        });

        cx.subscribe(&store, |view, store, _: &DataChanged, cx| {
            let entries = source_entries(store.read(cx));
            view.list.update(cx, |list, cx| {
                list.update_items(cx, |delegate, cx| delegate.reload(entries, cx));
            });
            view.reconcile_selection(cx);
            cx.notify();
        })
        .detach();

        let item_manager = ItemManager::global(cx);
        cx.observe(&item_manager, |_, _, cx| cx.notify()).detach();

        cx.spawn(async move |view, cx: &mut AsyncApp| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(60))
                    .await;
                if view
                    .update(cx, |view, cx| {
                        view.list.update(cx, |list, cx| {
                            list.update_items(cx, |delegate, cx| delegate.rebuild(cx));
                        });
                        view.reconcile_selection(cx);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        Self {
            list,
            focus_handle: cx.focus_handle(),
            marquee: MarqueeSelection::new(SelectionScope::Search),
        }
    }

    pub(crate) fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    pub(crate) fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| {
            list.update_items(cx, |delegate, cx| delegate.set_query(query.to_owned(), cx));
        });
        self.reconcile_selection(cx);
        cx.notify();
    }

    pub(crate) fn set_filters(&mut self, filters: Vec<SourceKind>, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| {
            list.update_items(cx, |delegate, cx| delegate.set_filters(filters, cx));
        });
        self.reconcile_selection(cx);
        cx.notify();
    }

    pub(crate) fn set_sort(&mut self, sort: SourceSort, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| {
            list.update_items(cx, |delegate, cx| delegate.set_sort(sort, cx));
        });
        self.reconcile_selection(cx);
        cx.notify();
    }

    pub(crate) fn focus_first_result(&self, window: &mut Window, cx: &mut App) {
        let target = self.list.read(cx).delegate().focus_target(0);
        let Some((scope, id, handle)) = target else {
            return;
        };
        focus_item(scope, id, &handle, window, cx);
        self.list
            .update(cx, |list, cx| list.scroll_item_into_view(0, cx));
    }

    fn reconcile_selection(&self, cx: &mut App) {
        let manager = SelectionManager::global(cx);
        let scope = manager.read(cx).scope();
        if scope != Some(SelectionScope::Search) {
            return;
        }

        let visible = self.list.read(cx).delegate().visible_ids();
        let retained = manager
            .read(cx)
            .ids()
            .iter()
            .copied()
            .filter(|id| visible.contains(id))
            .collect();
        manager.update(cx, |selection, cx| {
            selection.select_many(SelectionScope::Search, retained, cx)
        });
    }
}

impl MarqueeView for SearchView {
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

impl Render for SearchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.list.update(cx, |list, cx| {
            list.scroll_marquee(&mut self.marquee, window, cx);
        });
        let body = div().column().size_full().overflow_hidden().child(
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(DynamicList::new(&self.list).size_full()),
        );

        marquee(body, self, cx)
            .track_focus(&self.focus_handle)
            .on_dismiss(SelectionScope::Search, Some(self.focus_handle.clone()))
    }
}
