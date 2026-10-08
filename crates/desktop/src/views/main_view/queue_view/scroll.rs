use std::ops::Range;

use gpui::{Pixels, point, px};

use crate::components::virtual_list::VirtualListScrollHandle;

use super::agenda::{Agenda, AgendaLayout, QueueRowKey};

pub(super) struct RowAnchor {
    key: QueueRowKey,
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

pub(super) fn restore_offset(
    handle: &VirtualListScrollHandle,
    offset: Pixels,
    mut range: Range<usize>,
    row_count: usize,
) -> Range<usize> {
    handle.scroll_by_y(-offset);
    let viewport = handle.bounds();
    let bottom = point(viewport.left(), viewport.bottom() - px(1.));
    let end = handle
        .row_at_position(bottom)
        .map_or(row_count, |(index, _)| (index + 2).min(row_count));
    range.end = range.end.max(end);
    range
}
