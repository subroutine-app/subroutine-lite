mod column;
mod drop;
mod render;

use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use gpui::{App, AsyncApp, Context, FocusHandle, Focusable, Pixels, Point, Size, Window, px, size};
use subroutine_core::{Action, AnyItem, Event};
use uuid::Uuid;

use crate::{
    components::ItemCardStates,
    item_manager::ItemManager,
    selection::{SelectionManager, SelectionOrder, SelectionScope},
    settings::{GlobalSettings, Settings},
    stores::AppDatabaseStore,
};

use super::{
    EventMoment,
    queue_system::{QueueLane, QueueSnapshot},
    tab::{MainViewTab, SelectedMainView},
};
use column::QueueColumn;

const FADE: Duration = Duration::from_millis(160);
const PROMOTE: Duration = Duration::from_millis(360);
const CURRENT_HEIGHT: Pixels = px(124.);

#[derive(Clone)]
enum CurrentItem {
    Candidate(AnyItem),
    Event(EventMoment),
}

impl CurrentItem {
    fn id(&self) -> Uuid {
        match self {
            Self::Candidate(item) => item.id(),
            Self::Event(moment) => moment.notice_id(),
        }
    }

    fn action_id(&self) -> Option<Uuid> {
        match self {
            Self::Candidate(item) => Some(item.id()),
            Self::Event(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct CardPose {
    center: Point<Pixels>,
    size: Size<Pixels>,
}

#[derive(Clone)]
struct PresentedItem {
    item: CurrentItem,
    pose: CardPose,
    opacity: f32,
}

#[derive(Clone, Copy)]
struct PromotionOrigin {
    lane: QueueLane,
    pose: CardPose,
}

enum HandoffPhase {
    Fading(PresentedItem),
    Promoting,
}

struct Handoff {
    phase: HandoffPhase,
    origin: Option<PromotionOrigin>,
    retained_lanes: Vec<QueueLane>,
    started: Instant,
}

impl Handoff {
    fn new(
        outgoing: Option<PresentedItem>,
        origin: Option<PromotionOrigin>,
        retained_lanes: Vec<QueueLane>,
        started: Instant,
    ) -> Self {
        Self {
            phase: outgoing
                .filter(|item| item.opacity > 0.)
                .map_or(HandoffPhase::Promoting, HandoffPhase::Fading),
            origin,
            retained_lanes,
            started,
        }
    }

    fn outgoing(&self) -> Option<&PresentedItem> {
        match &self.phase {
            HandoffPhase::Fading(item) => Some(item),
            HandoffPhase::Promoting => None,
        }
    }

    fn hidden_action(&self, incoming: Option<&CurrentItem>) -> Option<Uuid> {
        match &self.phase {
            HandoffPhase::Fading(outgoing) => outgoing.item.action_id(),
            HandoffPhase::Promoting => incoming.and_then(CurrentItem::action_id),
        }
    }

    fn advance(&mut self, now: Instant) -> bool {
        if matches!(self.phase, HandoffPhase::Fading(_))
            && now.saturating_duration_since(self.started) >= FADE
        {
            self.phase = HandoffPhase::Promoting;
            self.started += FADE;
            true
        } else {
            false
        }
    }

    fn finished(&self, now: Instant, has_incoming: bool) -> bool {
        matches!(self.phase, HandoffPhase::Promoting)
            && (!has_incoming || now.saturating_duration_since(self.started) >= PROMOTE)
    }
}

pub struct QueueView {
    focus_handle: FocusHandle,
    current_focus: FocusHandle,
    actions: Vec<Action>,
    events: Vec<Event>,
    now: DateTime<Utc>,
    snapshot: QueueSnapshot,
    columns: [QueueColumn; 3],
    current: Option<CurrentItem>,
    handoff: Option<Handoff>,
    card_states: ItemCardStates,
    order: SelectionOrder,
    viewport: Size<Pixels>,
    scroll_pending: HashSet<QueueLane>,
    dragging: bool,
    drop_target: Option<QueueLane>,
    workspace_generation: u64,
    loaded: bool,
}

impl QueueView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = AppDatabaseStore::global(cx);
        cx.observe(&store, |view, store, cx| {
            let generation = store.read(cx).workspace_generation();
            if generation != view.workspace_generation {
                view.workspace_generation = generation;
                view.actions.clear();
                view.events.clear();
                view.current = None;
                view.handoff = None;
                view.dragging = false;
                view.drop_target = None;
                view.card_states = ItemCardStates::default();
                view.loaded = false;
                view.reconcile(cx);
            }
        })
        .detach();
        cx.observe(&ItemManager::global(cx), |view, _, cx| view.reconcile(cx))
            .detach();
        cx.observe_global::<GlobalSettings>(|view, cx| view.reconcile(cx))
            .detach();
        cx.spawn(async move |view, cx: &mut AsyncApp| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if view.update(cx, |view, cx| view.reconcile(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();

        Self {
            focus_handle: cx.focus_handle(),
            current_focus: cx.focus_handle(),
            actions: Vec::new(),
            events: Vec::new(),
            now: Utc::now(),
            snapshot: QueueSnapshot {
                candidates: Vec::new(),
                blocker: None,
            },
            columns: QueueLane::ALL.map(|lane| QueueColumn::new(lane, window, cx)),
            current: None,
            handoff: None,
            card_states: ItemCardStates::default(),
            order: SelectionOrder::new(SelectionScope::Queue, []),
            viewport: size(px(900.), px(560.)),
            scroll_pending: QueueLane::ALL.into_iter().collect(),
            dragging: false,
            drop_target: None,
            workspace_generation: store.read(cx).workspace_generation(),
            loaded: false,
        }
    }

    pub fn refresh(&mut self, actions: Vec<Action>, events: Vec<Event>, cx: &mut Context<Self>) {
        self.actions = actions;
        self.events = events;
        self.reconcile(cx);
    }

    fn reconcile(&mut self, cx: &mut Context<Self>) {
        let animation_now = Instant::now();
        self.advance_handoff(animation_now, cx);
        self.now = Utc::now();
        let mut snapshot = QueueSnapshot::new(
            &self.actions,
            &self.events,
            self.now,
            Settings::global(cx).schedule.granularity,
        );
        let manager = ItemManager::global(cx);
        snapshot
            .candidates
            .retain(|item| !manager.read(cx).is_completing(item.id()));
        let desired = snapshot
            .blocker
            .clone()
            .map(CurrentItem::Event)
            .or_else(|| {
                snapshot
                    .candidates
                    .first()
                    .cloned()
                    .map(CurrentItem::Candidate)
            });
        let previous_id = self.current.as_ref().map(CurrentItem::id);
        let desired_id = desired.as_ref().map(CurrentItem::id);
        let blocked = snapshot.blocker.is_some();
        let began_blocking = blocked && self.snapshot.blocker.is_none();
        if previous_id != desired_id {
            self.handoff = if self.loaded && !cx.reduce_motion() {
                let outgoing = self.presented(animation_now, cx);
                let origin = desired.as_ref().and_then(|item| match item {
                    CurrentItem::Candidate(item) => self.column_origin(item.id(), cx),
                    CurrentItem::Event(_) => None,
                });
                if let Some(origin) = origin {
                    self.scroll_pending.insert(origin.lane);
                }
                Some(Handoff::new(
                    outgoing,
                    origin,
                    self.visible_lanes(cx),
                    animation_now,
                ))
            } else {
                None
            };
        }
        let previous_action = self.current.as_ref().and_then(CurrentItem::action_id);
        self.current = desired;
        self.order = SelectionOrder::new(
            SelectionScope::Queue,
            snapshot
                .candidates
                .iter()
                .filter(|_| !blocked)
                .map(AnyItem::id),
        );
        let current_action = self.current.as_ref().and_then(CurrentItem::action_id);
        SelectionManager::global(cx).update(cx, |selection, cx| {
            if !selection.has_selection_in(SelectionScope::Queue) {
                return;
            }
            if blocked {
                if began_blocking {
                    selection.clear(cx);
                }
                return;
            }
            if previous_id != desired_id
                && let Some(previous) = previous_action
            {
                selection.hand_off(SelectionScope::Queue, previous, current_action, cx);
            }
            let stale = selection
                .ids()
                .iter()
                .copied()
                .filter(|id| !self.order.ids().contains(id))
                .collect::<Vec<_>>();
            for id in stale {
                selection.hand_off(SelectionScope::Queue, id, None, cx);
            }
        });
        self.snapshot = snapshot;
        self.update_columns(cx);
        self.retain_card_states();
        self.loaded = true;
        cx.notify();
    }

    fn advance_handoff(&mut self, now: Instant, cx: &mut Context<Self>) {
        let beginning_promotion = self.handoff.as_ref().is_some_and(|motion| {
            matches!(motion.phase, HandoffPhase::Fading(_))
                && now.saturating_duration_since(motion.started) >= FADE
        });
        let origin = beginning_promotion
            .then(|| {
                self.current
                    .as_ref()
                    .and_then(CurrentItem::action_id)
                    .and_then(|id| self.column_origin(id, cx))
            })
            .flatten();
        let Some(motion) = self.handoff.as_mut() else {
            return;
        };
        let advanced = motion.advance(now);
        if advanced {
            motion.origin = origin;
            if let Some(origin) = origin
                && !motion.retained_lanes.contains(&origin.lane)
            {
                motion.retained_lanes.push(origin.lane);
            }
        }
        let finished = cx.reduce_motion() || motion.finished(now, self.current.is_some());
        if finished {
            self.handoff = None;
        }
        if advanced || finished {
            self.update_columns(cx);
            self.retain_card_states();
        }
    }

    fn update_columns(&mut self, cx: &mut Context<Self>) {
        let hidden = self.handoff.as_ref().map_or_else(
            || self.current.as_ref().and_then(CurrentItem::action_id),
            |motion| motion.hidden_action(self.current.as_ref()),
        );
        for column in &mut self.columns {
            let items = self
                .snapshot
                .candidates
                .iter()
                .rev()
                .filter(|item| {
                    Some(item.id()) != hidden
                        && QueueLane::for_item(item, self.now) == Some(column.lane)
                })
                .cloned()
                .collect();
            let previous_priority = column.priority_id(cx);
            if let Some(id) = hidden {
                column.suppress_departure(id, cx);
            }
            column.update(
                items,
                self.order.clone(),
                self.snapshot.blocker.is_some(),
                self.workspace_generation,
                cx,
            );
            if previous_priority != column.priority_id(cx) {
                self.scroll_pending.insert(column.lane);
            }
        }
    }

    fn retain_card_states(&self) {
        self.card_states.retain(
            self.current.iter().map(CurrentItem::id).chain(
                self.handoff
                    .iter()
                    .filter_map(|motion| motion.outgoing().map(|shown| shown.item.id())),
            ),
        );
    }

    pub(super) fn clear_drop_target(&mut self, cx: &mut Context<Self>) {
        if self.dragging || self.drop_target.is_some() {
            self.dragging = false;
            self.drop_target = None;
            cx.notify();
        }
    }

    fn visible_lanes(&self, cx: &App) -> Vec<QueueLane> {
        self.columns
            .iter()
            .filter(|column| {
                (self.dragging && column.lane != QueueLane::Missed)
                    || column.len(cx) > 0
                    || self
                        .handoff
                        .as_ref()
                        .is_some_and(|motion| motion.retained_lanes.contains(&column.lane))
            })
            .map(|column| column.lane)
            .collect()
    }

    fn next_candidate(&self) -> Option<&AnyItem> {
        if self.snapshot.blocker.is_some() {
            return None;
        }
        let current = self.current.as_ref().and_then(CurrentItem::action_id);
        self.snapshot
            .candidates
            .iter()
            .find(|item| Some(item.id()) != current)
    }

    fn next_lane(&self) -> Option<QueueLane> {
        if self.snapshot.blocker.is_some() {
            return None;
        }
        self.handoff
            .as_ref()
            .and_then(|motion| {
                motion
                    .origin
                    .map(|origin| origin.lane)
                    .or_else(|| match self.current.as_ref()? {
                        CurrentItem::Candidate(item) => QueueLane::for_item(item, self.now),
                        CurrentItem::Event(_) => None,
                    })
            })
            .or_else(|| {
                self.next_candidate()
                    .and_then(|item| QueueLane::for_item(item, self.now))
            })
    }

    fn layout(&self, count: usize) -> render::QueueLayout {
        let height = self.current.as_ref().map_or(CURRENT_HEIGHT, |item| {
            self.card_states.height(item.id(), CURRENT_HEIGHT)
        });
        render::QueueLayout::new(self.viewport, count, height)
    }

    fn column_origin(&self, id: Uuid, cx: &App) -> Option<PromotionOrigin> {
        let item = self
            .snapshot
            .candidates
            .iter()
            .find(|item| item.id() == id)?;
        let lane = QueueLane::for_item(item, self.now)?;
        let column = self.columns.iter().find(|column| column.lane == lane)?;
        if column.len(cx) == 0 {
            return None;
        }
        let lanes = self.visible_lanes(cx);
        let index = lanes.iter().position(|candidate| *candidate == lane)?;
        Some(PromotionOrigin {
            lane,
            pose: self.layout(lanes.len()).source_card(index),
        })
    }

    fn bottom_lanes(&self, cx: &App) -> Vec<QueueLane> {
        self.columns
            .iter()
            .filter(|column| column.is_at_bottom(cx))
            .map(|column| column.lane)
            .collect()
    }
}

impl Focusable for QueueView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl MainViewTab for QueueView {
    const TAB: SelectedMainView = SelectedMainView::Queue;

    fn scope() -> Option<SelectionScope> {
        Some(SelectionScope::Queue)
    }

    fn go_to_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.scroll_pending.extend(QueueLane::ALL);
        if let Some(id) = self.current.as_ref().and_then(CurrentItem::action_id) {
            crate::selection::focus_item(
                SelectionScope::Queue,
                id,
                &self.current_focus,
                window,
                cx,
            );
        }
        cx.notify();
    }
}
