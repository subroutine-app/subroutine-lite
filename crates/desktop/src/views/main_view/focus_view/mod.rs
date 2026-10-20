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
use gpui::{App, AsyncApp, Context, FocusHandle, Focusable, Pixels, Size, Window};
use subroutine_core::{Action, AnyItem, Event, Signal};
use uuid::Uuid;

use crate::{
    components::{ItemCardStates, elastic_overscroll::ElasticOverscroll},
    item_manager::{DiscardDraft, DraftView, ItemManager},
    selection::{SelectionManager, SelectionOrder, SelectionScope},
    settings::GlobalSettings,
    stores::AppDatabaseStore,
    views::ToggleFocusMode,
};

use super::tab::{MainViewTab, SelectedMainView};
use gestures::CarouselScrollAxis;
use notices::NoticeSlot;

use temporal::{EventMoment, SignalMoment};

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
    requested_mode: Option<FocusMode>,
    inline_card_id: Option<Uuid>,
    source_actions: Vec<Action>,
    items: Vec<AnyItem>,
    queue_blocker: Option<EventMoment>,
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

    viewport_size: Size<Pixels>,
    // Keep motion identity when the measured layout adds or removes the scroll wrapper.
    carousel_motion: HashMap<Uuid, carousel::CarouselMotion>,

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
            cx.notify();
        })
        .detach();

        cx.observe(&ItemManager::global(cx), |view, _, cx| {
            view.reconcile_items(cx);
        })
        .detach();

        cx.subscribe(
            &ItemManager::global(cx),
            |view, _, event: &DiscardDraft, cx| {
                if view.inline_card_id == Some(event.0) {
                    view.inline_card_id = None;
                    cx.notify();
                }
            },
        )
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
                .filter(|active| {
                    selected.contains(active)
                        && !ItemManager::global(cx).read(cx).is_completing(*active)
                })
                .or_else(|| {
                    selected.iter().rev().copied().find(|id| {
                        view.items.iter().any(|item| item.id() == *id)
                            && !ItemManager::global(cx).read(cx).is_completing(*id)
                    })
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
            requested_mode: None,
            inline_card_id: None,
            source_actions: Vec::new(),
            items: Vec::new(),
            queue_blocker: None,
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
            viewport_size: Size::default(),
            carousel_motion: HashMap::new(),

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
        self.requested_mode = None;
        self.inline_card_id = None;
        self.card_states = ItemCardStates::default();
        self.carousel_motion.clear();
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
        self.reconcile_items(cx);
    }

    pub(super) fn set_mode(&mut self, mode: FocusMode, cx: &mut Context<Self>) {
        self.requested_mode = (self.mode != mode).then_some(mode);
        cx.notify();
    }

    fn resolve_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mode) = self.requested_mode.take() else {
            return;
        };
        let manager = ItemManager::global(cx);
        manager.update(cx, |manager, cx| {
            manager.commit_open_edit(window, cx);
        });
        if manager.read(cx).is_editing() {
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

    fn has_draft(&self, cx: &App) -> bool {
        ItemManager::global(cx)
            .read(cx)
            .view_draft(DraftView::Focus)
            .is_some()
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

    fn dismissed_idle(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        window.dispatch_action(Box::new(ToggleFocusMode), cx);
        true
    }
}
