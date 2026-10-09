use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::components::{menu::MenuBuilder, timed_toast};
use chrono::{Duration as ChronoDuration, Local};
use gpui::{
    App, AppContext, Bounds, ClipboardItem, Context, Entity, FocusHandle, Global,
    InteractiveElement, Modifiers, MouseDownEvent, Pixels, Point, Window, actions, point,
};
use gpui_kit::{display::badge::Tone, overlay::toast};
use subroutine_core::{Action, AnyItem, Routine, RoutineStep};
use uuid::Uuid;

use crate::{
    item_manager::ItemManager,
    keys::{CONTEXT_MENU_KEYS, key},
    stores::{AppDatabaseStore, UndoTransaction},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SelectionScope {
    Home,
    Timeline,
    Calendar,
    Queue,
    Focus,
    Unqueued,
    Routines,
    SavedItems,
    Search,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionModifier {
    Platform,
    Shift,
    Control,
    Command,
    Alt,
}

impl SelectionModifier {
    pub const ALL: [SelectionModifier; 5] = [
        SelectionModifier::Platform,
        SelectionModifier::Shift,
        SelectionModifier::Control,
        SelectionModifier::Command,
        SelectionModifier::Alt,
    ];

    pub fn is_held(&self, modifiers: &Modifiers) -> bool {
        match self {
            SelectionModifier::Platform => modifiers.secondary(),
            SelectionModifier::Shift => modifiers.shift,
            SelectionModifier::Control => modifiers.control,
            SelectionModifier::Command => modifiers.platform,
            SelectionModifier::Alt => modifiers.alt,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            SelectionModifier::Platform => "platform",
            SelectionModifier::Shift => "shift",
            SelectionModifier::Control => "control",
            SelectionModifier::Command => "command",
            SelectionModifier::Alt => "alt",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|modifier| modifier.id() == id)
    }

    pub fn label(&self) -> &'static str {
        match self {
            SelectionModifier::Platform if cfg!(target_os = "macos") => "Cmd (platform)",
            SelectionModifier::Platform => "Ctrl (platform)",
            SelectionModifier::Shift => "Shift",
            SelectionModifier::Control => "Control",
            SelectionModifier::Command if cfg!(target_os = "macos") => "Command",
            SelectionModifier::Command => "Super",
            SelectionModifier::Alt if cfg!(target_os = "macos") => "Option",
            SelectionModifier::Alt => "Alt",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.label() == label)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionGesture {
    Replace,
    Toggle,
    Range,
    ExtendRange,
}

#[derive(Debug, Clone, Copy)]
pub struct SelectionConfig {
    pub toggle: SelectionModifier,
    pub range: SelectionModifier,
}

impl Default for SelectionConfig {
    fn default() -> Self {
        Self {
            toggle: SelectionModifier::Platform,
            range: SelectionModifier::Shift,
        }
    }
}

impl SelectionConfig {
    pub fn gesture(&self, modifiers: &Modifiers) -> SelectionGesture {
        match (
            self.range.is_held(modifiers),
            self.toggle.is_held(modifiers),
        ) {
            (true, true) => SelectionGesture::ExtendRange,
            (true, false) => SelectionGesture::Range,
            (false, true) => SelectionGesture::Toggle,
            (false, false) => SelectionGesture::Replace,
        }
    }

    pub fn is_modified(&self, modifiers: &Modifiers) -> bool {
        self.gesture(modifiers) != SelectionGesture::Replace
    }
}

#[derive(Clone)]
pub struct SelectionOrder {
    pub scope: SelectionScope,
    items: Rc<Vec<Uuid>>,
}

impl SelectionOrder {
    pub fn new(scope: SelectionScope, items: impl IntoIterator<Item = Uuid>) -> Self {
        Self {
            scope,
            items: Rc::new(items.into_iter().collect()),
        }
    }

    pub fn ids(&self) -> &[Uuid] {
        &self.items
    }
}

pub struct SelectionManager {
    scope: Option<SelectionScope>,
    last_scope: Option<SelectionScope>,
    orders: HashMap<SelectionScope, Rc<Vec<Uuid>>>,
    selected: Vec<Uuid>,
    anchor: Option<Uuid>,
    press_claimed: bool,
    painting: Vec<CardRect>,
    painted: Vec<CardRect>,
}

struct CardRect {
    scope: SelectionScope,
    id: Uuid,
    bounds: Bounds<Pixels>,
}

struct GlobalSelectionManager(Entity<SelectionManager>);
impl Global for GlobalSelectionManager {}

impl SelectionManager {
    fn new() -> Self {
        Self {
            scope: None,
            last_scope: None,
            orders: HashMap::new(),
            selected: Vec::new(),
            anchor: None,
            press_claimed: false,
            painting: Vec::new(),
            painted: Vec::new(),
        }
    }

    pub fn initialize_global(cx: &mut App) -> Entity<Self> {
        if cx.has_global::<GlobalSelectionManager>() {
            return cx.global::<GlobalSelectionManager>().0.clone();
        }
        let manager = cx.new(|_| Self::new());
        cx.set_global(GlobalSelectionManager(manager.clone()));
        manager
    }

    pub fn global(cx: &App) -> Entity<Self> {
        cx.global::<GlobalSelectionManager>().0.clone()
    }

    pub fn count(&self) -> usize {
        self.selected.len()
    }

    pub fn ids(&self) -> &[Uuid] {
        &self.selected
    }

    pub fn scope(&self) -> Option<SelectionScope> {
        self.scope
    }

    pub fn press_is_claimed(&self) -> bool {
        self.press_claimed
    }

    pub fn has_selection_in(&self, scope: SelectionScope) -> bool {
        self.scope == Some(scope) && !self.selected.is_empty()
    }

    pub fn is_selected_in(&self, scope: SelectionScope, id: Uuid) -> bool {
        self.scope == Some(scope) && self.selected.contains(&id)
    }

    pub fn context_menu_targets_selection(&self, scope: SelectionScope, id: Uuid) -> bool {
        self.count() > 1 && self.is_selected_in(scope, id)
    }

    pub fn prepare_context_menu(scope: SelectionScope, id: Uuid, cx: &mut App) {
        let manager = Self::global(cx);
        if manager.read(cx).is_selected_in(scope, id) {
            return;
        }
        manager.update(cx, |selection, cx| selection.select_only(scope, id, cx));
    }

    pub fn select_for_inspection(scope: SelectionScope, id: Uuid, cx: &mut App) {
        Self::global(cx).update(cx, |selection, cx| selection.select_only(scope, id, cx));
    }

    pub fn selected_items(cx: &App) -> Vec<AnyItem> {
        let selection = Self::global(cx);
        let selection = selection.read(cx);
        let ids = selection.selected.clone();
        let scope = selection.scope;
        let store = AppDatabaseStore::global(cx);
        let store = store.read(cx);
        ids.into_iter()
            .filter_map(|id| match scope {
                Some(SelectionScope::SavedItems) => store.get_saved_item(id).map(AnyItem::from),
                Some(SelectionScope::Search) => store.get_item(id),
                _ => store.get_item(id).filter(|item| !item.is_template()),
            })
            .collect()
    }

    pub fn all_in_last_scope(&self) -> Option<(SelectionScope, Vec<Uuid>)> {
        let scope = self.last_scope?;
        let order = self.orders.get(&scope)?;
        (!order.is_empty()).then(|| (scope, order.as_ref().clone()))
    }

    pub fn report_card(scope: SelectionScope, id: Uuid, bounds: Bounds<Pixels>, cx: &mut App) {
        Self::global(cx).update(cx, |selection, _| {
            selection.painting.push(CardRect { scope, id, bounds });
        });
    }

    pub fn occlude_cards(scope: SelectionScope, bounds: Bounds<Pixels>, cx: &mut App) {
        if bounds.is_empty() {
            return;
        }
        Self::global(cx).update(cx, |selection, _| {
            selection.painting = std::mem::take(&mut selection.painting)
                .into_iter()
                .flat_map(|card| {
                    let portions = if card.scope == scope {
                        subtract_bounds(card.bounds, bounds)
                    } else {
                        vec![card.bounds]
                    };
                    portions
                        .into_iter()
                        .map(move |bounds| CardRect { bounds, ..card })
                })
                .collect();
        });
    }

    pub fn report_order(order: &SelectionOrder, cx: &mut App) {
        Self::global(cx).update(cx, |selection, _| {
            let current = selection.orders.get(&order.scope);
            if current.is_some_and(|existing| Rc::ptr_eq(existing, &order.items)) {
                return;
            }
            selection.orders.insert(order.scope, order.items.clone());
        });
    }

    pub fn settle_geometry(cx: &mut App) {
        Self::global(cx).update(cx, |selection, _| {
            selection.painted = std::mem::take(&mut selection.painting);
        });
    }

    pub fn card_at(&self, scope: SelectionScope, position: Point<Pixels>) -> Option<Uuid> {
        self.painted
            .iter()
            .rev()
            .find(|card| {
                card.scope == scope && !card.bounds.is_empty() && card.bounds.contains(&position)
            })
            .map(|card| card.id)
    }

    pub(crate) fn card_bounds(
        &self,
        scope: SelectionScope,
    ) -> impl Iterator<Item = (Uuid, Bounds<Pixels>)> + '_ {
        self.painted
            .iter()
            .filter(move |card| card.scope == scope)
            .map(|card| (card.id, card.bounds))
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        if self.selected.is_empty() && self.scope.is_none() {
            return;
        }
        self.scope = None;
        self.selected.clear();
        self.anchor = None;
        self.changed(cx);
    }

    pub fn clear_global(cx: &mut App) {
        Self::global(cx).update(cx, |selection, cx| selection.clear(cx));
    }

    pub fn track_background_clicks(window: &mut Window) {
        window.on_mouse_event(|_: &MouseDownEvent, phase, _, cx| {
            if phase.capture() {
                Self::begin_press(cx);
            } else {
                Self::clear_if_unclaimed(cx);
            }
        });
    }

    pub fn begin_press(cx: &mut App) {
        Self::global(cx).update(cx, |selection, _| selection.press_claimed = false);
    }

    pub fn claim_press(cx: &mut App) {
        Self::global(cx).update(cx, |selection, _| selection.press_claimed = true);
    }

    pub fn clear_if_unclaimed(cx: &mut App) {
        if !Self::global(cx).read(cx).press_claimed {
            Self::clear_global(cx);
        }
    }

    pub fn select_only(&mut self, scope: SelectionScope, id: Uuid, cx: &mut Context<Self>) {
        let unchanged = self.scope == Some(scope) && self.selected.as_slice() == [id];
        self.last_scope = Some(scope);
        self.scope = Some(scope);
        self.selected = vec![id];
        self.anchor = Some(id);
        if !unchanged {
            self.changed(cx);
        }
    }

    pub fn toggle(&mut self, scope: SelectionScope, id: Uuid, cx: &mut Context<Self>) {
        self.enter(scope);

        match self.selected.iter().position(|other| *other == id) {
            Some(pos) => {
                self.selected.remove(pos);
            }
            None => self.selected.push(id),
        }

        self.anchor = Some(id);

        if self.selected.is_empty() {
            self.scope = None;
        }
        self.changed(cx);
    }

    pub fn select_range(
        &mut self,
        order: &SelectionOrder,
        id: Uuid,
        extend: bool,
        cx: &mut Context<Self>,
    ) {
        let scope = order.scope;
        self.enter(scope);

        let span = self
            .anchor
            .and_then(|anchor| span_between(order.ids(), anchor, id));
        let Some(span) = span else {
            self.select_only(scope, id, cx);
            return;
        };

        if extend {
            for id in span {
                if !self.selected.contains(&id) {
                    self.selected.push(id);
                }
            }
        } else {
            self.selected = span;
        }
        self.changed(cx);
    }

    pub fn select_group(
        &mut self,
        order: &SelectionOrder,
        members: &[Uuid],
        gesture: SelectionGesture,
        cx: &mut Context<Self>,
    ) {
        if members.is_empty() {
            return;
        }
        self.enter(order.scope);
        let (selected, anchor) =
            group_selection(order.ids(), &self.selected, self.anchor, members, gesture);
        self.selected = selected;
        self.anchor = anchor;
        if self.selected.is_empty() {
            self.scope = None;
        }
        self.changed(cx);
    }

    pub fn select_keyboard_range(
        &mut self,
        order: &SelectionOrder,
        origin: Uuid,
        target: Uuid,
        cx: &mut Context<Self>,
    ) {
        self.enter(order.scope);
        let (selected, anchor) =
            keyboard_range_selection(order.ids(), &self.selected, self.anchor, origin, target);
        let changed = self.selected != selected;
        self.selected = selected;
        self.anchor = Some(anchor);
        if changed {
            self.changed(cx);
        }
    }

    pub fn select_many(&mut self, scope: SelectionScope, ids: Vec<Uuid>, cx: &mut Context<Self>) {
        if self.scope == Some(scope) && self.selected == ids {
            return;
        }
        self.anchor = ids.last().copied();
        self.last_scope = Some(scope);
        self.scope = (!ids.is_empty()).then_some(scope);
        self.selected = ids;
        self.changed(cx);
    }

    pub fn hand_off(
        &mut self,
        scope: SelectionScope,
        leaving: Uuid,
        arriving: Option<Uuid>,
        cx: &mut Context<Self>,
    ) {
        if !self.is_selected_in(scope, leaving) {
            return;
        }
        if self.selected.len() > 1 {
            self.selected.retain(|id| *id != leaving);
            if self.anchor == Some(leaving) {
                self.anchor = None;
            }
            self.changed(cx);
            return;
        }
        match arriving {
            Some(next) => self.select_only(scope, next, cx),
            None => self.clear(cx),
        }
    }

    fn enter(&mut self, scope: SelectionScope) {
        self.last_scope = Some(scope);
        if self.scope != Some(scope) {
            self.scope = Some(scope);
            self.selected.clear();
            self.anchor = None;
        }
    }

    fn changed(&self, cx: &mut Context<Self>) {
        cx.notify();
        cx.refresh_windows();
    }
}

fn subtract_bounds(bounds: Bounds<Pixels>, occluder: Bounds<Pixels>) -> Vec<Bounds<Pixels>> {
    let overlap = bounds.intersect(&occluder);
    if overlap.is_empty() {
        return vec![bounds];
    }
    let mut portions: Vec<_> = [
        Bounds::from_corners(bounds.origin, point(bounds.right(), overlap.top())),
        Bounds::from_corners(
            point(bounds.left(), overlap.bottom()),
            bounds.bottom_right(),
        ),
        Bounds::from_corners(
            point(bounds.left(), overlap.top()),
            point(overlap.left(), overlap.bottom()),
        ),
        Bounds::from_corners(
            point(overlap.right(), overlap.top()),
            point(bounds.right(), overlap.bottom()),
        ),
    ]
    .into_iter()
    .filter(|portion| !portion.is_empty())
    .collect();
    if portions.is_empty() {
        portions.push(Bounds::new(bounds.origin, Default::default()));
    }
    portions
}

fn group_selection(
    order: &[Uuid],
    selected: &[Uuid],
    anchor: Option<Uuid>,
    members: &[Uuid],
    gesture: SelectionGesture,
) -> (Vec<Uuid>, Option<Uuid>) {
    let Some(&last) = members.last() else {
        return (selected.to_vec(), anchor);
    };
    let mut next = match gesture {
        SelectionGesture::Replace | SelectionGesture::Range => members.to_vec(),
        SelectionGesture::Toggle if members.iter().all(|id| selected.contains(id)) => selected
            .iter()
            .filter(|id| !members.contains(id))
            .copied()
            .collect(),
        SelectionGesture::Toggle | SelectionGesture::ExtendRange => {
            selected.iter().chain(members).copied().collect()
        }
    };
    let anchor = match gesture {
        SelectionGesture::Replace | SelectionGesture::Toggle => Some(last),
        SelectionGesture::Range | SelectionGesture::ExtendRange => {
            if let Some(start) = anchor.and_then(|anchor| order.iter().position(|id| *id == anchor))
            {
                let (low, high) = order
                    .iter()
                    .enumerate()
                    .filter(|(_, id)| members.contains(id))
                    .fold((start, start), |(low, high), (position, _)| {
                        (low.min(position), high.max(position))
                    });
                next.extend_from_slice(&order[low..=high]);
            }
            next.sort_by_cached_key(|id| {
                order
                    .iter()
                    .position(|other| other == id)
                    .unwrap_or(usize::MAX)
            });
            anchor.filter(|id| order.contains(id)).or(Some(last))
        }
    };
    let mut seen = HashSet::new();
    next.retain(|id| seen.insert(*id));
    (next, anchor)
}

fn keyboard_range_selection(
    order: &[Uuid],
    selected: &[Uuid],
    anchor: Option<Uuid>,
    origin: Uuid,
    target: Uuid,
) -> (Vec<Uuid>, Uuid) {
    let anchor = anchor
        .filter(|anchor| order.contains(anchor) && selected.contains(&origin))
        .unwrap_or(origin);
    match span_between(order, anchor, target) {
        Some(selected) => (selected, anchor),
        None => (vec![target], target),
    }
}

fn span_between(order: &[Uuid], from: Uuid, to: Uuid) -> Option<Vec<Uuid>> {
    let start = order.iter().position(|id| *id == from)?;
    let end = order.iter().position(|id| *id == to)?;
    let (low, high) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    Some(order[low..=high].to_vec())
}

pub fn focus_item(
    scope: SelectionScope,
    id: Uuid,
    handle: &FocusHandle,
    window: &mut Window,
    cx: &mut App,
) {
    handle.focus(window, cx);
    SelectionManager::global(cx).update(cx, |selection, cx| {
        selection.select_only(scope, id, cx);
    });
}

pub fn focus_item_extending(
    order: &SelectionOrder,
    origin: Uuid,
    target: Uuid,
    handle: &FocusHandle,
    window: &mut Window,
    cx: &mut App,
) {
    handle.focus(window, cx);
    SelectionManager::global(cx).update(cx, |selection, cx| {
        selection.select_keyboard_range(order, origin, target, cx);
    });
}

#[derive(Clone)]
pub struct FocusHandoff {
    scope: Option<SelectionScope>,
    leaving: Uuid,
    arriving: Option<(Uuid, FocusHandle)>,
}

impl FocusHandoff {
    pub fn new(
        scope: Option<SelectionScope>,
        leaving: Uuid,
        arriving: Option<(Uuid, FocusHandle)>,
    ) -> Self {
        Self {
            scope,
            leaving,
            arriving,
        }
    }

    pub fn take(&self, window: &mut Window, cx: &mut App) {
        if let Some((_, handle)) = &self.arriving {
            handle.focus(window, cx);
        }
        let Some(scope) = self.scope else {
            return;
        };
        let leaving = self.leaving;
        let arriving = self.arriving.as_ref().map(|(id, _)| *id);
        SelectionManager::global(cx).update(cx, |selection, cx| {
            selection.hand_off(scope, leaving, arriving, cx);
        });
    }
}

actions!(selection, [Dismiss, OpenItemContextMenu]);

pub const ITEM_CARD_KEY_CONTEXT: &str = "ItemCard";

pub const VIEW_KEY_CONTEXT: &str = "AppView";

pub fn dismiss_view(
    scope: Option<SelectionScope>,
    home: Option<&FocusHandle>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let manager = ItemManager::global(cx);
    if manager.read(cx).is_editing() {
        manager.update(cx, |manager, cx| manager.discard_edit(window, cx));
        return true;
    }

    let dropped = scope.is_some_and(|scope| {
        SelectionManager::global(cx).update(cx, |selection, cx| {
            let held = selection.has_selection_in(scope);
            if held {
                selection.clear(cx);
            }
            held
        })
    });
    let moved = match home {
        Some(home) if !home.is_focused(window) => {
            home.focus(window, cx);
            true
        }
        None if window.focused(cx).is_some() => {
            window.blur();
            true
        }
        _ => false,
    };

    dropped || moved
}

pub trait DismissExt: Sized {
    fn on_dismiss(self, scope: SelectionScope, home: Option<FocusHandle>) -> Self;
}

impl<E: InteractiveElement> DismissExt for E {
    fn on_dismiss(self, scope: SelectionScope, home: Option<FocusHandle>) -> Self {
        self.key_context(VIEW_KEY_CONTEXT)
            .on_action(move |_: &Dismiss, window, cx| {
                if !dismiss_view(Some(scope), home.as_ref(), window, cx) {
                    cx.propagate();
                }
            })
    }
}

actions!(
    selection,
    [
        CompleteSelected,
        ToggleQueuedSelected,
        TogglePinnedSelected,
        DeleteSelected,
        CopySelected,
        CutSelected,
        PasteItems,
        DuplicateSelected,
        SelectAllItems,
    ]
);

pub const KEY_CONTEXT: &str = "Items";

pub fn init(cx: &mut App) {
    cx.bind_keys([
        key("escape", Dismiss, Some(VIEW_KEY_CONTEXT)),
        key(
            CONTEXT_MENU_KEYS[0],
            OpenItemContextMenu,
            Some(ITEM_CARD_KEY_CONTEXT),
        ),
        key(
            CONTEXT_MENU_KEYS[1],
            OpenItemContextMenu,
            Some(ITEM_CARD_KEY_CONTEXT),
        ),
        key("cmd-enter", CompleteSelected, Some(KEY_CONTEXT)),
        key("cmd-shift-enter", ToggleQueuedSelected, Some(KEY_CONTEXT)),
        key("cmd-p", TogglePinnedSelected, Some(KEY_CONTEXT)),
        key("delete", DeleteSelected, Some(KEY_CONTEXT)),
        key("backspace", DeleteSelected, Some(KEY_CONTEXT)),
        key("cmd-c", CopySelected, Some(KEY_CONTEXT)),
        key("cmd-x", CutSelected, Some(KEY_CONTEXT)),
        key("cmd-v", PasteItems, Some(KEY_CONTEXT)),
        key("cmd-d", DuplicateSelected, Some(KEY_CONTEXT)),
        key("cmd-a", SelectAllItems, Some(KEY_CONTEXT)),
    ]);
}

#[derive(Default)]
struct ItemClipboard {
    items: Vec<AnyItem>,
    scope: Option<SelectionScope>,
}

impl Global for ItemClipboard {}

pub fn complete_selected(window: &mut Window, cx: &mut App) {
    let items = SelectionManager::selected_items(cx);
    if items.is_empty() {
        return;
    }
    if bulk::complete(&items, window, cx) {
        SelectionManager::clear_global(cx);
    }
}

pub fn toggle_queued_selected(cx: &mut App) {
    let items = SelectionManager::selected_items(cx);
    if items.is_empty() {
        return;
    }
    let has_unqueued = items.iter().any(
        |item| matches!(item, AnyItem::Action(action) if !action.is_completed() && !action.queued),
    );
    if has_unqueued {
        bulk::queue(&items, cx);
    } else {
        bulk::unqueue(&items, cx);
    }
}

pub fn toggle_pinned_selected(cx: &mut App) {
    let items = SelectionManager::selected_items(cx);
    if items.is_empty() {
        return;
    }
    let has_unpinned = items.iter().any(
        |item| matches!(item, AnyItem::Action(action) if !action.is_completed() && action.start.is_some() && !action.pinned),
    );
    bulk::set_pinned(&items, has_unpinned, cx);
}

pub fn delete_selected(window: &mut Window, cx: &mut App) {
    let manager = SelectionManager::global(cx);
    if manager.read(cx).scope() == Some(SelectionScope::SavedItems) {
        let ids = manager.read(cx).ids().to_vec();
        if ids.is_empty() {
            return;
        }
        if bulk::delete_saved(&ids, window, cx) {
            SelectionManager::clear_global(cx);
        }
        return;
    }

    let items = SelectionManager::selected_items(cx);
    if items.is_empty() {
        return;
    }
    if bulk::delete(&items, window, cx) {
        SelectionManager::clear_global(cx);
    }
}

pub fn copy_selected(cx: &mut App) {
    let items = SelectionManager::selected_items(cx);
    if items.is_empty() {
        return;
    }
    let summary = items
        .iter()
        .map(describe_item)
        .collect::<Vec<_>>()
        .join("\n");
    cx.write_to_clipboard(ClipboardItem::new_string(summary));

    let scope = SelectionManager::global(cx).read(cx).scope;
    cx.set_global(ItemClipboard { items, scope });
}

pub fn cut_selected(window: &mut Window, cx: &mut App) {
    copy_selected(cx);
    delete_selected(window, cx);
}

pub fn can_paste_items(cx: &App) -> bool {
    cx.try_global::<ItemClipboard>()
        .is_some_and(|clipboard| !clipboard.items.is_empty())
}

pub fn paste_items(cx: &mut App) {
    let Some(clipboard) = cx.try_global::<ItemClipboard>() else {
        return;
    };
    if clipboard.items.is_empty() {
        return;
    }
    let items = clipboard.items.clone();
    let scope = clipboard.scope;
    let created = bulk::duplicate(&items, cx);
    select_created(created, scope, cx);
}

pub fn duplicate_selected(cx: &mut App) {
    let items = SelectionManager::selected_items(cx);
    if items.is_empty() {
        return;
    }
    let scope = SelectionManager::global(cx).read(cx).scope;
    let created = bulk::duplicate(&items, cx);
    select_created(created, scope, cx);
}

pub fn can_select_all_items(cx: &App) -> bool {
    SelectionManager::global(cx)
        .read(cx)
        .all_in_last_scope()
        .is_some_and(|(_, ids)| !ids.is_empty())
}

pub fn select_all_items(cx: &mut App) {
    let manager = SelectionManager::global(cx);
    let Some((scope, ids)) = manager.read(cx).all_in_last_scope() else {
        return;
    };
    manager.update(cx, |selection, cx| selection.select_many(scope, ids, cx));
}

fn select_created(created: Vec<AnyItem>, scope: Option<SelectionScope>, cx: &mut App) {
    if created.is_empty() {
        return;
    }
    let Some(scope) = scope else {
        return;
    };
    let ids: Vec<Uuid> = created.iter().map(|item| item.id()).collect();
    SelectionManager::global(cx).update(cx, |selection, cx| {
        selection.select_many(scope, ids, cx);
    });
}

fn describe_item(item: &AnyItem) -> String {
    match item.start_datetime() {
        Some(start) => format!(
            "{} — {}",
            item.title(),
            start.format("%a %b %-d, %-I:%M %p")
        ),
        None => item.title().to_string(),
    }
}

pub mod bulk {
    use super::*;

    fn actions(items: &[AnyItem]) -> impl Iterator<Item = &Action> {
        items.iter().filter_map(|item| match item {
            AnyItem::Action(action) => Some(action),
            _ => None,
        })
    }

    fn show_deletion_undo(
        toast_id: &'static str,
        noun: &'static str,
        transaction: UndoTransaction,
        affected: usize,
        window: &mut Window,
        cx: &mut App,
    ) {
        let message = if affected == 1 {
            format!("Deleted 1 {noun}")
        } else {
            format!("Deleted {affected} {noun}s")
        };
        toast::push(
            window,
            cx,
            timed_toast(toast_id, message)
                .tone(Tone::Warning)
                .action("Undo", move |window, cx| {
                    let undone = AppDatabaseStore::global(cx)
                        .update(cx, |store, cx| store.undo_transaction(transaction, cx));
                    if matches!(undone, Ok(false)) {
                        toast::push(
                            window,
                            cx,
                            timed_toast(
                                "items.undo-unavailable",
                                "That deletion is no longer the latest change and was not undone.",
                            )
                            .tone(Tone::Warning),
                        );
                    }
                }),
        );
    }

    pub fn delete(items: &[AnyItem], window: &mut Window, cx: &mut App) -> bool {
        let outcome = AppDatabaseStore::global(cx)
            .update(cx, |store, cx| store.delete_items(items.to_vec(), cx));
        let Ok(Some((transaction, affected))) = outcome else {
            return false;
        };
        show_deletion_undo("items.deleted", "item", transaction, affected, window, cx);
        true
    }

    pub fn delete_saved(ids: &[Uuid], window: &mut Window, cx: &mut App) -> bool {
        let outcome =
            AppDatabaseStore::global(cx).update(cx, |store, cx| store.delete_saved_items(ids, cx));
        let Ok(Some((transaction, affected))) = outcome else {
            return false;
        };
        show_deletion_undo(
            "saved-items.deleted",
            "saved item",
            transaction,
            affected,
            window,
            cx,
        );
        true
    }

    pub fn convert_events_to_markers(events: &[subroutine_core::Event], cx: &mut App) {
        AppDatabaseStore::global(cx).update(cx, |store, cx| {
            let _ = store.convert_events_to_markers(events, cx);
        });
    }

    pub fn copy_of(item: &AnyItem) -> AnyItem {
        match item.clone() {
            AnyItem::Action(mut action) => {
                action.id = Uuid::now_v7();
                action.recurrence_id = action.id;
                action.completion = None;
                action.source_provider = None;
                action.source_external_id = None;
                AnyItem::Action(action)
            }
            AnyItem::Event(mut event) => {
                event.id = Uuid::now_v7();
                event.lineage_id = event.id;
                event.source_provider = None;
                event.source_external_id = None;
                AnyItem::Event(event)
            }
            AnyItem::Routine(mut routine) => {
                routine.id = Uuid::now_v7();
                AnyItem::Routine(routine)
            }
            AnyItem::Marker(mut marker) => {
                marker.id = Uuid::now_v7();
                marker.lineage_id = marker.id;
                marker.source_provider = None;
                marker.source_external_id = None;
                AnyItem::Marker(marker)
            }
            AnyItem::Signal(mut signal) => {
                signal.id = Uuid::now_v7();
                signal.lineage_id = signal.id;
                AnyItem::Signal(signal)
            }
            AnyItem::ActionTemplate(mut template) => {
                template.id = Uuid::now_v7();
                template.sort_order = i64::MAX;
                AnyItem::ActionTemplate(template)
            }
            AnyItem::EventTemplate(mut template) => {
                template.id = Uuid::now_v7();
                template.lineage_id = Uuid::now_v7();
                template.sort_order = i64::MAX;
                template.source_provider = None;
                template.source_external_id = None;
                AnyItem::EventTemplate(template)
            }
        }
    }

    pub fn duplicate(items: &[AnyItem], cx: &mut App) -> Vec<AnyItem> {
        let copies: Vec<AnyItem> = items.iter().map(copy_of).collect();
        let result = AppDatabaseStore::global(cx)
            .update(cx, |store, cx| store.create_items(copies.clone(), cx));
        if result.is_ok() { copies } else { Vec::new() }
    }

    pub fn complete(items: &[AnyItem], window: &mut Window, cx: &mut App) -> bool {
        let ids: Vec<Uuid> = actions(items)
            .filter(|action| !action.is_completed())
            .map(|action| action.id)
            .collect();
        let outcome =
            AppDatabaseStore::global(cx).update(cx, |store, cx| store.complete_actions(&ids, cx));
        let Ok(Some((transaction, affected))) = outcome else {
            return false;
        };
        let message = if affected == 1 {
            "Completed 1 action".to_owned()
        } else {
            format!("Completed {affected} actions")
        };
        toast::push(
            window,
            cx,
            timed_toast("items.completed", message)
                .tone(Tone::Success)
                .action("Undo", move |window, cx| {
                    let undone = AppDatabaseStore::global(cx)
                        .update(cx, |store, cx| store.undo_transaction(transaction, cx));
                    if matches!(undone, Ok(false)) {
                        toast::push(
                            window,
                            cx,
                            timed_toast(
                                "items.undo-unavailable",
                                "That completion is no longer the latest change and was not undone.",
                            )
                            .tone(Tone::Warning),
                        );
                    }
                }),
        );
        true
    }

    pub fn uncomplete(items: &[AnyItem], cx: &mut App) {
        let changed = actions(items)
            .filter(|action| action.is_completed())
            .cloned()
            .map(|action| AnyItem::Action(subroutine_core::ops::actions::uncomplete(action).value))
            .collect();
        AppDatabaseStore::global(cx).update(cx, |store, cx| {
            let _ = store.update_items(changed, cx);
        });
    }

    pub fn queue(items: &[AnyItem], cx: &mut App) {
        let ids: Vec<Uuid> = actions(items)
            .filter(|action| !action.is_completed() && !action.queued)
            .map(|action| action.id)
            .collect();
        AppDatabaseStore::global(cx).update(cx, |store, cx| {
            let _ = store.queue_actions(&ids, cx);
        });
    }

    pub fn unqueue(items: &[AnyItem], cx: &mut App) {
        let changed = actions(items)
            .filter(|action| !action.is_completed() && action.queued)
            .cloned()
            .map(|mut action| {
                action.set_queued(false);
                action.set_start(None);
                action.set_pinned(false);
                AnyItem::Action(action)
            })
            .collect();
        AppDatabaseStore::global(cx).update(cx, |store, cx| {
            let _ = store.update_items(changed, cx);
        });
    }

    pub fn clear_durations(items: &[AnyItem], cx: &mut App) {
        let changed = actions(items)
            .filter(|action| !action.is_completed() && action.duration.is_some())
            .cloned()
            .map(|mut action| {
                action.duration = None;
                AnyItem::Action(action)
            })
            .collect();
        AppDatabaseStore::global(cx).update(cx, |store, cx| {
            let _ = store.update_items(changed, cx);
        });
    }

    pub fn set_pinned(items: &[AnyItem], pinned: bool, cx: &mut App) {
        let changed: Vec<Action> = actions(items)
            .filter(|action| {
                !action.is_completed() && action.start.is_some() && action.pinned != pinned
            })
            .cloned()
            .map(|mut action| {
                action.pinned = pinned;
                action
            })
            .collect();
        AppDatabaseStore::global(cx).update(cx, |store, cx| {
            let _ = store.update_items(changed.into_iter().map(AnyItem::Action).collect(), cx);
        });
    }

    pub fn save_as_templates(items: &[AnyItem], cx: &mut App) {
        let templates = actions(items)
            .filter(|action| action.template_id.is_none())
            .map(|action| AnyItem::ActionTemplate(action.as_template()))
            .collect();
        AppDatabaseStore::global(cx).update(cx, |store, cx| {
            let _ = store.create_items(templates, cx);
        });
    }

    pub fn save_as_routine(items: &[AnyItem], cx: &mut App) {
        let mut ordered: Vec<&Action> = actions(items).collect();
        ordered.sort_by_key(|action| action.start.map(|start| start.timestamp()));

        let steps: Vec<RoutineStep> = ordered
            .iter()
            .map(|action| {
                let step = RoutineStep::new(action.title.clone());
                match action.duration {
                    Some(duration) => step.with_duration(duration),
                    None => step,
                }
            })
            .collect();

        let Some(first) = ordered.first() else {
            return;
        };
        let routine = Routine::new(first.title.clone()).with_steps(steps);
        AppDatabaseStore::global(cx).update(cx, |store, cx| {
            let _ = store.upsert_routine(routine, cx);
        });
    }

    pub fn shift(items: &[AnyItem], delta: ChronoDuration, cx: &mut App) {
        if delta.is_zero() {
            return;
        }
        let items = items.to_vec();
        AppDatabaseStore::global(cx).update(cx, |store, cx| {
            let mut changed = Vec::new();
            for item in items {
                match item {
                    AnyItem::Action(action) => {
                        if let Some(action) = shifted_action(action, delta) {
                            changed.push(AnyItem::Action(action));
                        }
                    }
                    AnyItem::Event(mut event) => {
                        event.start += delta;
                        changed.push(AnyItem::Event(event));
                    }
                    AnyItem::Signal(mut signal) => {
                        signal.datetime += delta;
                        changed.push(AnyItem::Signal(signal));
                    }
                    AnyItem::Marker(mut marker) => {
                        let days = ChronoDuration::days(delta.num_days());
                        if days.is_zero() {
                            continue;
                        }
                        marker.date += days;
                        marker.end_date = marker.end_date.map(|end| end + days);
                        changed.push(AnyItem::Marker(marker));
                    }
                    AnyItem::Routine(_)
                    | AnyItem::ActionTemplate(_)
                    | AnyItem::EventTemplate(_) => {}
                }
            }
            let _ = store.update_items(changed, cx);
        });
    }

    fn shifted_action(mut action: Action, delta: ChronoDuration) -> Option<Action> {
        if action.is_completed() {
            return None;
        }
        action.start = Some(action.start? + delta);
        Some(action)
    }

    pub fn move_to_now(items: &[AnyItem], cx: &mut App) {
        let Some(earliest) = items.iter().filter_map(|item| item.start_datetime()).min() else {
            return;
        };
        shift(items, Local::now() - earliest, cx);
    }
}

pub fn selection_context_menu(items: Vec<AnyItem>) -> MenuBuilder {
    let count = items.len();
    let actions: Vec<&Action> = items
        .iter()
        .filter_map(|item| match item {
            AnyItem::Action(action) => Some(action),
            _ => None,
        })
        .collect();

    let events: Vec<_> = items
        .iter()
        .filter_map(|item| match item {
            AnyItem::Event(event) => Some(event.clone()),
            _ => None,
        })
        .collect();

    let incomplete = actions.iter().filter(|a| !a.is_completed()).count();
    let completed = actions.iter().filter(|a| a.is_completed()).count();
    let unqueued = actions
        .iter()
        .filter(|a| !a.is_completed() && !a.queued)
        .count();
    let queued = actions
        .iter()
        .filter(|a| !a.is_completed() && a.queued)
        .count();
    let timed = actions
        .iter()
        .filter(|a| !a.is_completed() && a.duration.is_some())
        .count();
    let untemplated = actions.iter().filter(|a| a.template_id.is_none()).count();
    let scheduled = items
        .iter()
        .filter(|item| !item.is_completed() && item.start().is_some())
        .count();
    let placed = actions
        .iter()
        .filter(|a| !a.is_completed() && a.start.is_some());
    let to_pin = placed.clone().filter(|a| !a.pinned).count();
    let to_unpin = placed.filter(|a| a.pinned).count();
    let action_count = actions.len();
    let event_count = events.len();

    MenuBuilder::new()
        .label(format!("{count} items selected"))
        .separator()
        .when(incomplete > 0, |menu| {
            let items = items.clone();
            menu.item_with_keybinding(
                format!("Complete {incomplete}"),
                CompleteSelected,
                move |window, cx| {
                    if bulk::complete(&items, window, cx) {
                        SelectionManager::clear_global(cx);
                    }
                },
            )
        })
        .when(completed > 0, |menu| {
            let items = items.clone();
            menu.item(format!("Mark {completed} incomplete"), move |_, cx| {
                bulk::uncomplete(&items, cx);
                SelectionManager::clear_global(cx);
            })
        })
        .when(unqueued > 0, |menu| {
            let items = items.clone();
            menu.item_with_keybinding(
                format!("Queue {unqueued}"),
                ToggleQueuedSelected,
                move |_, cx| bulk::queue(&items, cx),
            )
        })
        .when(queued > 0, |menu| {
            let items = items.clone();
            if unqueued == 0 {
                menu.item_with_keybinding(
                    format!("Unqueue {queued}"),
                    ToggleQueuedSelected,
                    move |_, cx| bulk::unqueue(&items, cx),
                )
            } else {
                menu.item(format!("Unqueue {queued}"), move |_, cx| {
                    bulk::unqueue(&items, cx)
                })
            }
        })
        .when(scheduled > 0, |menu| {
            let items = items.clone();
            menu.separator().submenu("Reschedule", move |mut menu| {
                for (label, delta) in RESCHEDULE_STEPS {
                    let items = items.clone();
                    menu = menu.item(*label, move |_, cx| bulk::shift(&items, delta(), cx));
                }
                let items = items.clone();
                menu.separator()
                    .item("Start now", move |_, cx| bulk::move_to_now(&items, cx))
            })
        })
        .when(event_count > 0, |menu| {
            menu.item(
                if event_count == 1 {
                    "Convert event to date marker".to_owned()
                } else {
                    format!("Convert {event_count} events to date markers")
                },
                move |_, cx| {
                    bulk::convert_events_to_markers(&events, cx);
                    SelectionManager::clear_global(cx);
                },
            )
        })
        .when(timed > 0, |menu| {
            let items = items.clone();
            menu.item(format!("Remove {timed} durations"), move |_, cx| {
                bulk::clear_durations(&items, cx)
            })
        })
        .when(to_pin > 0, |menu| {
            let items = items.clone();
            menu.item_with_keybinding(
                format!("Pin {to_pin} to their times"),
                TogglePinnedSelected,
                move |_, cx| bulk::set_pinned(&items, true, cx),
            )
        })
        .when(to_unpin > 0, |menu| {
            let items = items.clone();
            if to_pin == 0 {
                menu.item_with_keybinding(
                    format!("Unpin {to_unpin}"),
                    TogglePinnedSelected,
                    move |_, cx| bulk::set_pinned(&items, false, cx),
                )
            } else {
                menu.item(format!("Unpin {to_unpin}"), move |_, cx| {
                    bulk::set_pinned(&items, false, cx)
                })
            }
        })
        .when(action_count > 1, |menu| {
            let items = items.clone();
            menu.separator().item(
                format!("Save {action_count} actions as a routine"),
                move |_, cx| {
                    bulk::save_as_routine(&items, cx);
                    SelectionManager::clear_global(cx);
                },
            )
        })
        .when(untemplated > 0, |menu| {
            let items = items.clone();
            menu.item(format!("Save {untemplated} for reuse"), move |_, cx| {
                bulk::save_as_templates(&items, cx)
            })
        })
        .separator()
        .item_with_keybinding(format!("Copy {count} items"), CopySelected, |_, cx| {
            copy_selected(cx)
        })
        .item_with_keybinding(
            format!("Duplicate {count} items"),
            DuplicateSelected,
            |_, cx| duplicate_selected(cx),
        )
        .separator()
        .item_with_keybinding(format!("Delete {count} items"), DeleteSelected, {
            let items = items.clone();
            move |window, cx| {
                if bulk::delete(&items, window, cx) {
                    SelectionManager::clear_global(cx);
                }
            }
        })
        .item("Clear selection", |_, cx| {
            SelectionManager::clear_global(cx)
        })
}

type RescheduleStep = (&'static str, fn() -> ChronoDuration);

const RESCHEDULE_STEPS: &[RescheduleStep] = &[
    ("15 minutes later", || ChronoDuration::minutes(15)),
    ("1 hour later", || ChronoDuration::hours(1)),
    ("1 day later", || ChronoDuration::days(1)),
    ("1 week later", || ChronoDuration::weeks(1)),
    ("1 hour earlier", || ChronoDuration::hours(-1)),
    ("1 day earlier", || ChronoDuration::days(-1)),
];
