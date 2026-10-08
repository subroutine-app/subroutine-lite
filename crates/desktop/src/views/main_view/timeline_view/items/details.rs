use std::collections::HashSet;

use gpui::{
    AnyElement, Bounds, Context, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Pixels, Styled, Window, div, point, px, size,
};
use gpui_kit_theme::ActiveTheme;
use uuid::Uuid;

use crate::{
    components::ext::ElementExt,
    selection::{SelectionManager, SelectionScope},
    views::TOP_EDGE_INSET,
};

use super::{super::TimelineView, TimelineSlot};

#[derive(Clone, Copy)]
pub(in crate::views::main_view::timeline_view) enum DetailsAnchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl DetailsAnchor {
    fn for_bounds(anchor: Bounds<Pixels>, viewport: Bounds<Pixels>) -> Self {
        match (
            anchor.center().x > viewport.center().x,
            anchor.top() > viewport.center().y,
        ) {
            (false, false) => Self::TopLeft,
            (true, false) => Self::TopRight,
            (false, true) => Self::BottomLeft,
            (true, true) => Self::BottomRight,
        }
    }
}

fn details_bounds(
    anchor: Bounds<Pixels>,
    viewport: Bounds<Pixels>,
    desired_height: Pixels,
    progress: f32,
    placement: DetailsAnchor,
) -> Bounds<Pixels> {
    let width = (anchor.size.width
        + (anchor.size.width.max(px(360.)) - anchor.size.width) * progress.clamp(0., 1.))
    .min(viewport.size.width);
    let height = desired_height.min(px(520.)).min(viewport.size.height);
    let left = match placement {
        DetailsAnchor::TopLeft | DetailsAnchor::BottomLeft => anchor.left(),
        DetailsAnchor::TopRight | DetailsAnchor::BottomRight => anchor.right() - width,
    }
    .clamp(viewport.left(), viewport.right() - width);
    let top = match placement {
        DetailsAnchor::TopLeft | DetailsAnchor::TopRight => anchor.top(),
        DetailsAnchor::BottomLeft | DetailsAnchor::BottomRight => anchor.bottom() - height,
    }
    .clamp(viewport.top(), viewport.bottom() - height);
    Bounds::new(point(left, top), size(width, height))
}

impl TimelineView {
    pub(super) fn sync_item_details(
        &mut self,
        slots: &[TimelineSlot],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let standalone: HashSet<_> = slots
            .iter()
            .filter_map(|slot| match slot {
                TimelineSlot::Item { index, .. } => Some(self.items[*index].item.id()),
                _ => None,
            })
            .collect();
        let focused_hidden = self.items.iter().find(|entry| {
            !standalone.contains(&entry.item.id())
                && self.item_details.is_open(entry.item.id())
                && entry.focus_handle.contains_focused(window, cx)
        });
        if let Some(entry) = focused_hidden {
            let id = entry.item.id();
            let focus = if self.expanded_bin_contains(id) {
                &entry.focus_handle
            } else {
                slots
                    .iter()
                    .find_map(|slot| {
                        let TimelineSlot::Bin(bin) = slot else {
                            return None;
                        };
                        let top = self.time_to_offset(bin.start)
                            + self.scroll_offset
                            + self.center_relative().y;
                        let visible = self.bounds.is_some_and(|bounds| {
                            top + super::MIN_ITEM_HEIGHT - super::SLOT_GAP > TOP_EDGE_INSET
                                && top < bounds.size.height
                        });
                        (visible
                            && bin
                                .members
                                .iter()
                                .any(|index| self.items[*index].item.id() == id))
                        .then(|| self.bin_focus_handles.get(&bin.key))
                        .flatten()
                    })
                    .unwrap_or(&self.focus_handle)
            };
            focus.focus(window, cx);
        }
        self.item_details.retain(standalone);
        self.item_details_anchors
            .retain(|id, _| self.item_details.is_open(*id));
    }

    pub(super) fn render_item_details(
        &mut self,
        id: Uuid,
        card: AnyElement,
        anchor: Bounds<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let viewport_size = self
            .bounds
            .map_or(window.viewport_size(), |bounds| bounds.size);
        let inset = px(12.).min(viewport_size.width / 2.);
        let top = (TOP_EDGE_INSET + px(12.)).min(viewport_size.height);
        let bottom = (viewport_size.height - px(12.)).max(top);
        let viewport = Bounds::from_corners(
            point(inset, top),
            point(viewport_size.width - inset, bottom),
        );
        let details = self.item_details.get(id);
        let placement = *self
            .item_details_anchors
            .entry(id)
            .or_insert_with(|| DetailsAnchor::for_bounds(anchor, viewport));
        let bounds = details_bounds(
            anchor,
            viewport,
            self.item_details.height(id, anchor.size.height),
            details.progress(),
            placement,
        );

        div()
            .id(("timeline-item-frame", id.as_u64_pair().1))
            .absolute()
            .top(bounds.top())
            .left(bounds.left())
            .w(bounds.size.width)
            .h(bounds.size.height)
            .rounded_xl()
            .bg(cx.theme().colors.panel)
            .shadow_lg()
            .block_mouse_except_scroll()
            .capture_any_mouse_down(cx.listener(move |view, event: &MouseDownEvent, _, _| {
                view.force_press = None;
                view.inspect_force_item = None;
                view.inspect_press_item = (event.button == MouseButton::Left).then_some(id);
            }))
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(move |_, event: &MouseDownEvent, _, cx| {
                if !cx.has_active_drag()
                    && !crate::components::menu::context_menu_contains_position(event.position, cx)
                    && details.collapse()
                {
                    cx.notify();
                }
            }))
            .on_prepaint(|bounds, window, cx| {
                SelectionManager::occlude_cards(
                    SelectionScope::Timeline,
                    bounds.intersect(&window.content_mask().bounds),
                    cx,
                );
            })
            .child(card)
            .into_any_element()
    }
}
