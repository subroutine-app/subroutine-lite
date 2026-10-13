use std::collections::{BTreeSet, HashMap, HashSet};

use chrono::NaiveDate;
use gpui::{
    App, Context, IntoElement, MouseExitEvent, MouseMoveEvent, Pixels, Point, Styled, Window,
    canvas, px,
};
use gpui_kit::motion::Transition;
use subroutine_core::AnyItem;

use crate::{
    components::transition,
    views::{LIST_VIEW_MAX_WIDTH, LIST_VIEW_MIN_WIDTH},
};

use super::{
    UpcomingView,
    agenda::{Agenda, UpcomingRow},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum UpcomingSection {
    Missed,
    Day(NaiveDate),
}

impl UpcomingSection {
    pub fn for_item(item: &AnyItem, today: NaiveDate) -> Option<Self> {
        let date = item.start_date()?;
        if matches!(item, AnyItem::Action(action) if action.queued && !action.is_completed())
            && date < today
        {
            Some(Self::Missed)
        } else {
            Some(Self::Day(date))
        }
    }
}

impl UpcomingView {
    fn section_at(&self, position: Point<Pixels>) -> Option<UpcomingSection> {
        let (ix, mut bounds) = self.scroll_handle.row_at_position(position)?;
        let width = bounds
            .size
            .width
            .clamp(LIST_VIEW_MIN_WIDTH, LIST_VIEW_MAX_WIDTH);
        bounds.origin.x += ((bounds.size.width - width) / 2.).max(px(0.));
        bounds.size.width = width;
        if !bounds.contains(&position) {
            return None;
        }
        Some(self.agenda.rows.get(*self.layout.rows.get(ix)?)?.section())
    }

    fn set_hovered_section(&mut self, section: Option<UpcomingSection>, cx: &mut Context<Self>) {
        if self.hovered_section != section {
            self.hovered_section = section;
            cx.notify();
        }
    }

    pub(super) fn hover_observer(&self, cx: &Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        canvas(
            |_, _, _| (),
            move |_, _, window, cx| {
                let position = window.is_window_hovered().then(|| window.mouse_position());
                let reconciled_view = view.clone();
                cx.defer(move |cx| {
                    reconciled_view.update(cx, |view, cx| {
                        let section = position
                            .filter(|_| !view.mouse_exited)
                            .and_then(|position| view.section_at(position));
                        view.set_hovered_section(section, cx);
                    });
                });

                let moved_view = view.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase.capture() {
                        moved_view.update(cx, |view, cx| {
                            view.mouse_exited = false;
                            view.set_hovered_section(view.section_at(event.position), cx);
                        });
                    }
                });
                window.on_mouse_event(move |_: &MouseExitEvent, phase, _, cx| {
                    if phase.capture() {
                        view.update(cx, |view, cx| {
                            // GPUI retains the last mouse position after leaving the window.
                            view.mouse_exited = true;
                            view.set_hovered_section(None, cx);
                        });
                    }
                });
            },
        )
        .absolute()
        .inset_0()
    }
}

struct SectionState {
    expanded: bool,
    reveal: Transition<f32>,
}

impl SectionState {
    fn new(expanded: bool) -> Self {
        Self {
            expanded,
            reveal: Transition::new(if expanded { 1. } else { 0. }, transition::QUICK),
        }
    }
}

#[derive(Default)]
pub(super) struct UpcomingSections {
    states: HashMap<UpcomingSection, SectionState>,
    revealed_dates: BTreeSet<NaiveDate>,
    pending_reveals: HashSet<UpcomingSection>,
}

impl UpcomingSections {
    pub fn sync(&mut self, agenda: &Agenda, today: NaiveDate) {
        let populated: HashSet<_> = agenda
            .rows
            .iter()
            .filter_map(|row| match row {
                UpcomingRow::Item { section, .. } => Some(*section),
                _ => None,
            })
            .collect();
        self.states.retain(|section, _| populated.contains(section));
        for section in populated {
            self.states.entry(section).or_insert_with(|| {
                SectionState::new(
                    section == UpcomingSection::Day(today)
                        || self.pending_reveals.contains(&section),
                )
            });
        }
        for section in std::mem::take(&mut self.pending_reveals) {
            self.set_expanded(section, true);
        }
    }

    pub fn can_expand(&self, section: UpcomingSection) -> bool {
        self.states.contains_key(&section)
    }

    pub fn is_expanded(&self, section: UpcomingSection) -> bool {
        self.states.get(&section).is_none_or(|state| state.expanded)
    }

    pub fn set_expanded(&mut self, section: UpcomingSection, expanded: bool) {
        let Some(state) = self.states.get_mut(&section) else {
            return;
        };
        state.expanded = expanded;
        state.reveal.set(if expanded { 1. } else { 0. });
    }

    pub fn reveal(&mut self, section: UpcomingSection) {
        if let UpcomingSection::Day(date) = section {
            self.revealed_dates.insert(date);
        }
        self.pending_reveals.insert(section);
    }

    pub fn revealed_dates(&self) -> &BTreeSet<NaiveDate> {
        &self.revealed_dates
    }

    pub fn collapse_all(&mut self) {
        self.pending_reveals.clear();
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
            .map_or((true, 1.), |state| (state.expanded, state.reveal.value()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subroutine_core::{Action, Marker, SchedulePoint};

    #[test]
    fn only_today_starts_expanded_and_refresh_preserves_choices() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 10).unwrap();
        let tomorrow = today.succ_opt().unwrap();
        let items = [today.pred_opt().unwrap(), today, tomorrow].map(|date| {
            AnyItem::Action(
                Action::new("Item")
                    .with_queued(true)
                    .with_start(Some(SchedulePoint::Date(date))),
            )
        });
        let agenda = Agenda::build_revealing(&items, &[], &[], &[], today, &BTreeSet::new());
        let day = UpcomingSection::Day(today);
        let next_day = UpcomingSection::Day(tomorrow);
        let mut sections = UpcomingSections::default();
        sections.sync(&agenda, today);
        assert_eq!(sections.layout_state(day), (true, 1.));
        assert_eq!(sections.layout_state(next_day), (false, 0.));
        assert_eq!(sections.layout_state(UpcomingSection::Missed), (false, 0.));

        sections.set_expanded(day, false);
        sections.set_expanded(next_day, true);
        sections.sync(&agenda, today);
        assert!(!sections.is_expanded(day));
        assert!(sections.is_expanded(next_day));
    }

    #[test]
    fn expansion_requires_queue_rows_and_tracks_the_last_item() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 9).unwrap();
        let day = UpcomingSection::Day(today);
        let tomorrow = today.succ_opt().unwrap();
        let scheduled = UpcomingSection::Day(tomorrow);
        let marker = Marker::new("Marker only", today);
        let agenda = |items: &[AnyItem]| {
            Agenda::build_revealing(
                items,
                &[],
                std::slice::from_ref(&marker),
                &[],
                today,
                &BTreeSet::new(),
            )
        };
        let mut sections = UpcomingSections::default();
        sections.sync(&agenda(&[]), today);
        sections.set_expanded(scheduled, true);
        sections.reveal(day);
        sections.sync(&agenda(&[]), today);
        for section in [day, scheduled] {
            assert!(!sections.can_expand(section));
            assert_eq!(sections.layout_state(section), (true, 1.));
        }

        let undated = AnyItem::Action(Action::new("No start").with_queued(true));
        assert_eq!(UpcomingSection::for_item(&undated, today), None);
        let undated_agenda = agenda(&[undated]);
        assert!(
            undated_agenda
                .rows
                .iter()
                .all(|row| row.selectable_ids().all(|id| id == marker.id))
        );
        sections.sync(&undated_agenda, today);
        assert!(!sections.can_expand(day));
        assert!(!sections.can_expand(scheduled));

        let draft = AnyItem::Action(
            Action::new("New item")
                .with_queued(true)
                .with_start(Some(SchedulePoint::Date(tomorrow))),
        );
        sections.reveal(scheduled);
        sections.sync(&agenda(std::slice::from_ref(&draft)), today);
        assert!(sections.can_expand(scheduled));
        assert!(sections.is_expanded(scheduled));
        assert!(!sections.can_expand(day));

        sections.sync(&agenda(&[]), today);
        sections.set_expanded(scheduled, true);
        assert!(!sections.can_expand(scheduled));
        assert_eq!(sections.layout_state(scheduled), (true, 1.));

        sections.sync(&agenda(&[draft]), today);
        assert!(sections.can_expand(scheduled));
        assert!(!sections.is_expanded(scheduled));
    }
}
