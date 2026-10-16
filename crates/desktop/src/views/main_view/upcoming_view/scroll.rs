use gpui::{Pixels, px};

use crate::components::virtual_list::VirtualListScrollHandle;

use super::agenda::{Agenda, AgendaLayout, UpcomingRowKey};

pub(super) struct RowAnchor {
    key: UpcomingRowKey,
    offset: Pixels,
}

impl RowAnchor {
    pub fn capture(
        agenda: &Agenda,
        layout: &AgendaLayout,
        handle: &VirtualListScrollHandle,
    ) -> Option<Self> {
        let viewport = handle.bounds();
        let (index, bounds) = handle.row_at_position(viewport.origin)?;
        Some(Self {
            key: agenda.rows.get(*layout.rows.get(index)?)?.key(),
            offset: viewport.top() - bounds.top(),
        })
    }

    pub fn resolve(&self, agenda: &Agenda, layout: &AgendaLayout) -> Option<(usize, Pixels)> {
        let index = layout
            .rows
            .iter()
            .position(|source| agenda.rows[*source].key() == self.key)?;
        let offset = if self.offset < layout.sizes[index].height {
            self.offset.max(px(0.))
        } else {
            px(0.)
        };
        Some((index, offset))
    }
}
