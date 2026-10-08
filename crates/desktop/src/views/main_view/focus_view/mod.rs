mod carousel;
mod gestures;
mod mode_switch;
mod navigation;
mod notices;
mod reconcile;
mod render;
mod settings;
pub(crate) mod temporal;

use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use chrono::{DateTime, Utc};
use gpui::{App, AsyncApp, Context, FocusHandle, Focusable, Pixels, Size, Window, px, size};
use subroutine_core::{Action, AnyItem, Event, Signal};
use uuid::Uuid;

use crate::{
    components::{ItemCardStates, elastic_overscroll::ElasticOverscroll},
    item_manager::ItemManager,
    selection::{SelectionManager, SelectionOrder, SelectionScope},
    settings::GlobalSettings,
    stores::AppDatabaseStore,
};

use super::tab::{MainViewTab, SelectedMainView};
use gestures::CarouselScrollAxis;
use notices::NoticeSlot;
use reconcile::replacement_index;
use temporal::{EventMoment, SignalMoment};

pub(crate) use notices::event_notice_card;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum FocusMode {
    #[default]
    Action,
    Event,
}

impl FocusMode {
    const ALL: [Self; 2] = [Self::Action, Self::Event];

    const fn id(self) -> &'static str {
        match self {
            Self::Action => "action",
            Self::Event => "event",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Action => "Actions",
            Self::Event => "Events",
        }
    }
}

pub struct FocusView {
    pub(super) focus_handle: FocusHandle,
    settings_button_focus: FocusHandle,
    settings_open: bool,
    mode: FocusMode,
    source_actions: Vec<Action>,
    items: Vec<AnyItem>,
    events: Vec<Event>,
    event_moments: Vec<EventMoment>,
    active_event_id: Option<Uuid>,
    event_focus_handles: HashMap<Uuid, FocusHandle>,
    signals: Vec<Signal>,
    now: DateTime<Utc>,
    event_notice: NoticeSlot<EventMoment>,
    signal_notice: NoticeSlot<SignalMoment>,
    dismissed_event_notice: Option<Uuid>,
    dismissed_signal_notice: Option<Uuid>,
    source_ids: HashSet<Uuid>,
    entering_ids: HashSet<Uuid>,
    active_id: Option<Uuid>,
    item_focus_handles: HashMap<Uuid, FocusHandle>,
    loaded: bool,
    restore_focus: bool,
    carousel_size: Size<Pixels>,
    viewport_size: Size<Pixels>,

    card_states: ItemCardStates,
    notice_states: ItemCardStates,
    notice_card_widths: HashMap<Uuid, Pixels>,
    workspace_generation: u64,
    wheel_active: bool,
    wheel_axis: Option<CarouselScrollAxis>,
    pixel_gesture_travel: f32,
    pixel_gesture_navigated: bool,
    wheel_phase_ended: bool,
    wheel_generation: u64,
    carousel_overscroll: ElasticOverscroll,
    drop_active: bool,
}

impl FocusView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let store = AppDatabaseStore::global(cx);
        cx.observe(&store, |view, store, cx| {
            view.sync_workspace(store.read(cx).workspace_generation(), cx);
        })
        .detach();

        cx.observe(&ItemManager::global(cx), |view, manager, cx| {
            let previous_active = view.active_id;
            let previous_index = view.active_index();
            let completing: HashSet<Uuid> = {
                let manager = manager.read(cx);
                view.items
                    .iter()
                    .filter(|item| manager.is_completing(item.id()))
                    .map(AnyItem::id)
                    .collect()
            };

            if let Some(index) =
                previous_index.filter(|index| completing.contains(&view.items[*index].id()))
            {
                let replacement = ((index + 1)..view.items.len())
                    .find(|candidate| !completing.contains(&view.items[*candidate].id()))
                    .or_else(|| {
                        (0..index)
                            .rev()
                            .find(|candidate| !completing.contains(&view.items[*candidate].id()))
                    })
                    .map(|index| view.items[index].id());
                if replacement != previous_active {
                    let leaving = view.items[index].id();
                    view.active_id = replacement;
                    if view.mode == FocusMode::Action {
                        view.reset_wheel();
                    }
                    SelectionManager::global(cx).update(cx, |selection, cx| {
                        selection.hand_off(SelectionScope::Focus, leaving, replacement, cx)
                    });
                }
            }

            view.items.retain(|item| {
                view.source_ids.contains(&item.id()) || completing.contains(&item.id())
            });
            let visible_ids: HashSet<_> = view.items.iter().map(AnyItem::id).collect();
            view.item_focus_handles
                .retain(|id, _| visible_ids.contains(id));
            if let Some(leaving) = previous_active.filter(|id| !visible_ids.contains(id)) {
                view.active_id = replacement_index(previous_index, view.items.len())
                    .and_then(|index| view.items.get(index))
                    .map(AnyItem::id);
                if view.mode == FocusMode::Action {
                    view.reset_wheel();
                }
                SelectionManager::global(cx).update(cx, |selection, cx| {
                    selection.hand_off(SelectionScope::Focus, leaving, view.active_id, cx)
                });
                view.restore_focus = true;
            }
            cx.notify();
        })
        .detach();

        cx.observe(&SelectionManager::global(cx), |view, selection, cx| {
            if view.mode != FocusMode::Action {
                return;
            }
            let selected = {
                let selection = selection.read(cx);
                if selection.has_selection_in(SelectionScope::Focus) {
                    selection.ids().to_vec()
                } else {
                    Vec::new()
                }
            };
            let target = view
                .active_id
                .filter(|active| selected.contains(active))
                .or_else(|| {
                    selected
                        .iter()
                        .rev()
                        .copied()
                        .find(|id| view.items.iter().any(|item| item.id() == *id))
                });
            if let Some(id) = target
                && view.active_id != Some(id)
            {
                view.active_id = Some(id);
                view.reset_wheel();
                cx.notify();
            }
        })
        .detach();

        cx.observe_global::<GlobalSettings>(|view, cx| {
            view.reconcile_items(cx);
        })
        .detach();

        cx.spawn(async move |view, cx: &mut AsyncApp| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if view
                    .update(cx, |view, cx| {
                        view.now = Utc::now();
                        view.reconcile_items(cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        Self {
            focus_handle: cx.focus_handle(),
            settings_button_focus: cx.focus_handle(),
            settings_open: false,
            mode: FocusMode::default(),
            source_actions: Vec::new(),
            items: Vec::new(),
            events: Vec::new(),
            event_moments: Vec::new(),
            active_event_id: None,
            event_focus_handles: HashMap::new(),
            signals: Vec::new(),
            now: Utc::now(),
            event_notice: NoticeSlot::default(),
            signal_notice: NoticeSlot::default(),
            dismissed_event_notice: None,
            dismissed_signal_notice: None,
            source_ids: HashSet::new(),
            entering_ids: HashSet::new(),
            active_id: None,
            item_focus_handles: HashMap::new(),
            loaded: false,
            restore_focus: false,
            carousel_size: size(px(640.), px(260.)),
            viewport_size: size(px(640.), px(400.)),

            card_states: ItemCardStates::default(),
            notice_states: ItemCardStates::default(),
            notice_card_widths: HashMap::new(),
            workspace_generation: store.read(cx).workspace_generation(),
            wheel_active: false,
            wheel_axis: None,
            pixel_gesture_travel: 0.0,
            pixel_gesture_navigated: false,
            wheel_phase_ended: false,
            wheel_generation: 0,
            carousel_overscroll: ElasticOverscroll::default(),
            drop_active: false,
        }
    }

    fn sync_workspace(&mut self, generation: u64, cx: &mut Context<Self>) {
        if self.workspace_generation == generation {
            return;
        }
        self.workspace_generation = generation;
        self.card_states = ItemCardStates::default();
        self.notice_states = ItemCardStates::default();
        self.notice_card_widths.clear();
        self.event_notice = NoticeSlot::default();
        self.signal_notice = NoticeSlot::default();
        self.dismissed_event_notice = None;
        self.dismissed_signal_notice = None;
        cx.notify();
    }

    pub fn refresh_temporal(
        &mut self,
        events: Vec<Event>,
        signals: Vec<Signal>,
        cx: &mut Context<Self>,
    ) {
        self.events = events;
        self.signals = signals;
        cx.notify();
    }

    pub(super) fn set_mode(&mut self, mode: FocusMode, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }

        self.mode = mode;
        self.reset_wheel();
        self.carousel_overscroll.reset();
        self.drop_active = false;
        self.restore_focus = false;
        match mode {
            FocusMode::Event => {
                SelectionManager::clear_global(cx);
                let empty = SelectionOrder::new(SelectionScope::Focus, std::iter::empty());
                SelectionManager::report_order(&empty, cx);
            }
            FocusMode::Action => {
                if let Some(active) = self.active_id {
                    SelectionManager::global(cx).update(cx, |selection, cx| {
                        selection.select_only(SelectionScope::Focus, active, cx)
                    });
                }
            }
        }
        cx.notify();
    }

    pub(super) fn is_action_mode(&self) -> bool {
        self.mode == FocusMode::Action
    }

    pub fn refresh_actions(&mut self, actions: Vec<Action>, cx: &mut Context<Self>) {
        self.source_actions = actions;
        self.reconcile_items(cx);
    }
}

impl Focusable for FocusView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl MainViewTab for FocusView {
    const TAB: SelectedMainView = SelectedMainView::Focus;

    fn scope() -> Option<SelectionScope> {
        Some(SelectionScope::Focus)
    }

    fn go_to_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let first = self.navigable_ids(cx).first().copied();
        if let Some(first) = first {
            self.activate(first, window, cx);
        }
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.settings_open {
            return false;
        }
        self.close_settings(window, cx);
        true
    }
}
