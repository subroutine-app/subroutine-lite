use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    time::Instant,
};

use gpui::{App, Pixels, Window, px};
use gpui_kit::motion::Transition;
use uuid::Uuid;

use crate::components::transition;

#[derive(Clone, Default)]
pub struct ItemCardStates(Rc<RefCell<HashMap<Uuid, ItemCardDetails>>>);

impl ItemCardStates {
    pub fn get(&self, id: Uuid) -> ItemCardDetails {
        self.0.borrow_mut().entry(id).or_default().clone()
    }

    pub fn height(&self, id: Uuid, collapsed: Pixels) -> Pixels {
        self.0.borrow().get(&id).map_or(collapsed, |details| {
            let state = details.0.borrow();
            let desired = state.height.as_ref().map_or(collapsed, Transition::value);
            collapsed + (desired - collapsed).max(px(0.0)) * state.progress()
        })
    }

    pub fn is_open(&self, id: Uuid) -> bool {
        self.0
            .borrow()
            .get(&id)
            .is_some_and(ItemCardDetails::is_open)
    }

    pub fn collapse_all(&self) -> bool {
        let mut changed = false;
        for details in self.0.borrow().values() {
            changed |= details.collapse();
        }
        changed
    }

    pub fn animate(&self, window: &mut Window, cx: &mut App) {
        let now = Instant::now();
        for details in self.0.borrow().values() {
            if details.0.borrow_mut().advance(now, cx.reduce_motion()) {
                window.request_animation_frame();
            }
        }
    }

    pub fn pause(&self) {
        for details in self.0.borrow().values() {
            details.0.borrow_mut().last_frame = None;
        }
    }

    pub fn retain(&self, ids: impl IntoIterator<Item = Uuid>) {
        let ids: HashSet<_> = ids.into_iter().collect();
        self.0.borrow_mut().retain(|id, _| ids.contains(id));
    }
}

#[derive(Clone, Default)]
pub struct ItemCardDetails(Rc<RefCell<DetailsState>>);

struct DetailsState {
    expanded: bool,
    expansion: Transition<f32>,
    height: Option<Transition<Pixels>>,
    last_frame: Option<Instant>,
}

impl Default for DetailsState {
    fn default() -> Self {
        Self {
            expanded: false,
            expansion: Transition::new(0.0, transition::QUICK),
            height: None,
            last_frame: None,
        }
    }
}

impl DetailsState {
    fn advance(&mut self, now: Instant, reduce_motion: bool) -> bool {
        let Some(height) = self.height.as_mut() else {
            self.last_frame = None;
            return false;
        };
        if reduce_motion {
            self.expansion.snap(self.expansion.target());
            height.snap(height.target());
        } else if let Some(previous) = self.last_frame {
            let delta = now.saturating_duration_since(previous);
            self.expansion.advance(delta);
            height.advance(delta);
        }
        let moving = self.expansion.is_animating() || height.is_animating();
        self.last_frame = moving.then_some(now);
        moving
    }

    fn progress(&self) -> f32 {
        self.expansion.value().clamp(0.0, 1.0)
    }
}

impl ItemCardDetails {
    pub(super) fn expanded(&self) -> bool {
        self.0.borrow().expanded
    }

    pub(super) fn is_open(&self) -> bool {
        let state = self.0.borrow();
        state.expanded || state.progress() > 0.0
    }

    pub(crate) fn progress(&self) -> f32 {
        self.0.borrow().progress()
    }

    pub(super) fn toggle(&self) {
        let mut state = self.0.borrow_mut();
        state.expanded = !state.expanded;
        let target = if state.expanded { 1.0 } else { 0.0 };
        state.expansion.set(target);
        state.last_frame = None;
    }

    pub(crate) fn collapse(&self) -> bool {
        let mut state = self.0.borrow_mut();
        if !state.expanded {
            return false;
        }
        state.expanded = false;
        state.expansion.set(0.0);
        state.last_frame = None;
        true
    }

    pub(super) fn reset(&self) -> bool {
        let changed = self.is_open();
        let mut state = self.0.borrow_mut();
        state.expanded = false;
        state.expansion.snap(0.0);
        state.last_frame = None;
        changed
    }

    pub(super) fn measure(&self, height: Pixels) -> bool {
        let mut state = self.0.borrow_mut();
        let expanded = state.expanded;
        let Some(measured) = state.height.as_mut() else {
            state.height = Some(Transition::new(height, transition::QUICK));
            return true;
        };
        if (measured.target() - height).abs() <= px(1.0) {
            return false;
        }
        if expanded {
            measured.set(height);
        } else {
            measured.snap(height);
        }
        true
    }
}
