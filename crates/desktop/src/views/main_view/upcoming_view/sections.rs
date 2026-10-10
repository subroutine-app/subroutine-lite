use std::collections::{BTreeSet, HashMap};

use chrono::NaiveDate;
use gpui::{App, Window};
use gpui_kit::motion::Transition;
use subroutine_core::AnyItem;

use crate::components::transition;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum UpcomingSection {
    Missed,
    Unscheduled,
    Day(NaiveDate),
}

impl From<Option<NaiveDate>> for UpcomingSection {
    fn from(date: Option<NaiveDate>) -> Self {
        date.map_or(Self::Unscheduled, Self::Day)
    }
}

impl UpcomingSection {
    pub fn for_item(item: &AnyItem, today: NaiveDate) -> Self {
        if matches!(item, AnyItem::Action(action) if action.queued && !action.is_completed())
            && item.start_date().is_some_and(|date| date < today)
        {
            Self::Missed
        } else {
            item.start_date().into()
        }
    }
}

struct SectionState {
    expanded: bool,
    reveal: Transition<f32>,
}

impl Default for SectionState {
    fn default() -> Self {
        Self {
            expanded: false,
            reveal: Transition::new(0., transition::QUICK),
        }
    }
}

#[derive(Default)]
pub(super) struct UpcomingSections {
    states: HashMap<UpcomingSection, SectionState>,
    revealed_dates: BTreeSet<NaiveDate>,
}

impl UpcomingSections {
    pub fn is_expanded(&self, section: UpcomingSection) -> bool {
        self.states
            .get(&section)
            .is_some_and(|state| state.expanded)
    }

    pub fn set_expanded(&mut self, section: UpcomingSection, expanded: bool) {
        let state = self.states.entry(section).or_default();
        state.expanded = expanded;
        state.reveal.set(if expanded { 1. } else { 0. });
    }

    pub fn reveal(&mut self, section: UpcomingSection) {
        if let UpcomingSection::Day(date) = section {
            self.revealed_dates.insert(date);
        }
        self.set_expanded(section, true);
    }

    pub fn revealed_dates(&self) -> &BTreeSet<NaiveDate> {
        &self.revealed_dates
    }

    pub fn collapse_all(&mut self) {
        for state in self.states.values_mut() {
            state.expanded = false;
            state.reveal.set(0.);
        }
    }

    pub fn animate(&mut self, window: &mut Window, cx: &mut App) {
        for state in self.states.values_mut() {
            state.reveal.animate(window, cx);
        }
    }

    pub fn is_animating(&self) -> bool {
        self.states
            .values()
            .any(|state| state.reveal.is_animating())
    }

    pub fn layout_state(&self, section: UpcomingSection) -> (bool, f32) {
        self.states
            .get(&section)
            .map_or((false, 0.), |state| (state.expanded, state.reveal.value()))
    }
}
