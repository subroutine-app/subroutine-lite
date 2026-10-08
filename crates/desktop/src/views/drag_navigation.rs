use std::time::Duration;

use gpui::{Bounds, Pixels, Point, px};

use crate::views::SelectedMainView;

pub(super) const DRAG_NAVIGATION_DELAY: Duration = Duration::from_millis(350);

pub(super) fn is_scheduler(view: SelectedMainView) -> bool {
    scheduler_index(view).is_some()
}

fn scheduler_index(view: SelectedMainView) -> Option<usize> {
    match view {
        SelectedMainView::Timeline => Some(0),
        SelectedMainView::Calendar => Some(1),
        SelectedMainView::Queue => Some(2),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct HoverTicket {
    pub(super) target: SelectedMainView,
    pub(super) generation: u64,
}

#[derive(Default)]
pub(super) struct DragNavigation {
    bounds: [Option<Bounds<Pixels>>; 3],
    pending: Option<HoverTicket>,
    generation: u64,
}

impl DragNavigation {
    pub(super) fn record_bounds(&mut self, view: SelectedMainView, bounds: Bounds<Pixels>) {
        if let Some(index) = scheduler_index(view) {
            self.bounds[index] =
                (bounds.size.width > px(0.) && bounds.size.height > px(0.)).then_some(bounds);
        }
    }

    pub(super) fn hover(
        &mut self,
        view: SelectedMainView,
        position: Point<Pixels>,
    ) -> Option<HoverTicket> {
        if !self.contains(view, position) {
            if self.pending() == Some(view) {
                self.clear();
            }
            return None;
        }
        if self.pending() == Some(view) {
            return None;
        }

        let ticket = HoverTicket {
            target: view,
            generation: self.next_generation(),
        };
        self.pending = Some(ticket);
        Some(ticket)
    }

    pub(super) fn pending(&self) -> Option<SelectedMainView> {
        self.pending.map(|ticket| ticket.target)
    }

    pub(super) fn clear(&mut self) -> bool {
        if self.pending.take().is_none() {
            return false;
        }
        self.next_generation();
        true
    }

    pub(super) fn complete(
        &mut self,
        ticket: HoverTicket,
        pointer: Point<Pixels>,
        drag_active: bool,
    ) -> Option<SelectedMainView> {
        if self.pending != Some(ticket) {
            return None;
        }

        self.clear();
        (drag_active && self.contains(ticket.target, pointer)).then_some(ticket.target)
    }

    fn contains(&self, view: SelectedMainView, position: Point<Pixels>) -> bool {
        scheduler_index(view)
            .and_then(|index| self.bounds[index].as_ref())
            .is_some_and(|bounds| bounds.contains(&position))
    }

    fn next_generation(&mut self) -> u64 {
        self.generation = self
            .generation
            .checked_add(1)
            .expect("drag navigation generation exhausted");
        self.generation
    }
}
