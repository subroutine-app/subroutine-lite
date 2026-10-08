use crate::components::ButtonVariants as _;
use chrono::Duration as ChronoDuration;
use gpui::{
    AnyElement, App, Bounds, Context, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    MouseButton, ParentElement, Pixels, ScrollHandle, Size, StatefulInteractiveElement, Styled,
    Window, div, point, prelude::FluentBuilder, px, size,
};
use gpui_kit::foundation::{FocusRing as _, Sizable as _, StyledExt as _};
use gpui_kit_theme::ActiveTheme;
use uuid::Uuid;

use crate::{
    AppIcon,
    components::{
        Button, ItemCard, Label, SIDEBAR_ITEM_GAP, SIDEBAR_ITEM_HEIGHT, ext::ElementExt,
        scrollbar::Scrollbar,
    },
    icons::Icon,
    item_manager::ItemManager,
    selection::{SelectionGesture, SelectionManager, SelectionScope},
    settings::Settings,
    views::TOP_EDGE_INSET,
};

use super::super::{CloseTimelineBin, TimelineView};
use super::{ItemNavigation, NavigationHandoffs, TimelineBin, TimelineSlot, attached::SlotKind};

const HEADER_HEIGHT: Pixels = px(58.);
const LIST_PADDING: Pixels = px(12.);
const MAX_HEIGHT: Pixels = px(520.);

pub(in crate::views::main_view::timeline_view) struct ExpandedBin {
    anchor: Uuid,
    members: Vec<Uuid>,
    scroll: ScrollHandle,
    focus: FocusHandle,
    focused_item: Option<Uuid>,
    expands_left: bool,
    expands_up: bool,
}

fn expanded_bounds(
    anchor: Bounds<Pixels>,
    viewport: Size<Pixels>,
    content_height: Pixels,
    expands_left: bool,
    expands_up: bool,
) -> Bounds<Pixels> {
    let inset = px(12.);
    let visible_top = (TOP_EDGE_INSET + inset).min(viewport.height);
    let width = anchor
        .size
        .width
        .max(px(360.))
        .min(px(520.))
        .min((viewport.width - inset * 2.).max(px(0.)));
    let height = (HEADER_HEIGHT + LIST_PADDING * 2. + content_height + px(2.))
        .min(MAX_HEIGHT)
        .min((viewport.height - visible_top - inset).max(px(0.)));
    let left = if expands_left {
        anchor.right() - width
    } else {
        anchor.left()
    }
    .clamp(inset, (viewport.width - width - inset).max(inset));
    let top = if expands_up {
        anchor.bottom() - height
    } else {
        anchor.top()
    }
    .clamp(
        visible_top,
        (viewport.height - height - inset).max(visible_top),
    );
    Bounds::new(point(left, top), size(width, height))
}

impl TimelineView {
    pub(super) fn expanded_bin_contains(&self, id: Uuid) -> bool {
        self.expanded_bin
            .as_ref()
            .is_some_and(|bin| bin.members.contains(&id))
    }

    pub(super) fn bin_navigation_target(
        &self,
        direction: ItemNavigation,
        window: &Window,
        cx: &App,
    ) -> Option<Uuid> {
        let members = if let Some(bin) = &self.expanded_bin
            && bin.focus.is_focused(window)
        {
            bin.members.clone()
        } else {
            let bin = self
                .layout_slots(cx)
                .into_iter()
                .find_map(|slot| match slot {
                    TimelineSlot::Bin(bin)
                        if self
                            .bin_focus_handles
                            .get(&bin.key)
                            .is_some_and(|focus| focus.is_focused(window)) =>
                    {
                        Some(bin)
                    }
                    _ => None,
                })?;
            self.bin_members(&bin)
        };
        match direction {
            ItemNavigation::Previous => members.last().copied(),
            ItemNavigation::Next => members.first().copied(),
        }
    }

    pub(super) fn bin_is_expanded(&self, bin: &TimelineBin) -> bool {
        self.expanded_bin.as_ref().is_some_and(|expanded| {
            bin.members
                .iter()
                .any(|index| self.items[*index].item.id() == expanded.anchor)
        })
    }

    fn bin_members(&self, bin: &TimelineBin) -> Vec<Uuid> {
        let mut entries: Vec<_> = bin
            .members
            .iter()
            .map(|index| &self.items[*index])
            .collect();
        entries.sort_unstable_by_key(|entry| (entry.item.start_datetime(), entry.item.id()));
        entries.into_iter().map(|entry| entry.item.id()).collect()
    }

    fn bin_containing(&self, id: Uuid, cx: &App) -> Option<TimelineBin> {
        self.layout_slots(cx)
            .into_iter()
            .find_map(|slot| match slot {
                TimelineSlot::Bin(bin)
                    if bin
                        .members
                        .iter()
                        .any(|index| self.items[*index].item.id() == id) =>
                {
                    Some(bin)
                }
                _ => None,
            })
    }

    fn open_bin(&mut self, bin: &TimelineBin, anchor: Uuid, cx: &mut Context<Self>) {
        let bounds = self.lane_bounds(bin.start, ChronoDuration::zero(), bin.lane);
        let screen_top = bounds.top() + self.scroll_offset + self.center_relative().y;
        let viewport = self
            .bounds
            .map_or(size(px(800.), px(800.)), |bounds| bounds.size);
        self.marquee.end();
        self.edge_scroll_speed = None;
        self.expanded_bin = Some(ExpandedBin {
            anchor,
            members: self.bin_members(bin),
            scroll: ScrollHandle::new(),
            focus: cx.focus_handle(),
            focused_item: None,
            expands_left: bounds.center().x > viewport.width / 2.,
            expands_up: screen_top + bounds.size.height / 2. > viewport.height / 2.,
        });
        cx.notify();
    }

    pub(in crate::views::main_view::timeline_view) fn close_bin(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let manager = ItemManager::global(cx);
        manager.update(cx, |manager, cx| {
            manager.commit_open_edit(window, cx);
        });
        if manager.read(cx).is_editing() {
            return;
        }
        if let Some(bin) = self.expanded_bin.take() {
            if bin.focus.contains_focused(window, cx) {
                self.focus_handle.focus(window, cx);
            }
            if self
                .pending_item_focus
                .is_some_and(|id| bin.members.contains(&id))
            {
                self.pending_item_focus = None;
            }
            self.marquee.end();
            self.edge_scroll_speed = None;
            cx.notify();
        }
    }

    fn toggle_bin(&mut self, anchor: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        if self.expanded_bin_contains(anchor) {
            self.close_bin(window, cx);
        } else if let Some(bin) = self.bin_containing(anchor, cx) {
            self.close_bin(window, cx);
            if ItemManager::global(cx).read(cx).is_editing() {
                return;
            }
            self.open_bin(&bin, anchor, cx);
            if let Some(expanded) = &self.expanded_bin {
                expanded.focus.focus(window, cx);
            }
        }
    }

    pub(super) fn reveal_binned_item(&mut self, id: Uuid, cx: &mut Context<Self>) -> bool {
        let Some(bin) = self.bin_containing(id, cx) else {
            return false;
        };
        if !self.bin_is_expanded(&bin) {
            self.open_bin(&bin, id, cx);
        }
        if let Some(expanded) = &self.expanded_bin
            && let Some(index) = self
                .bin_members(&bin)
                .iter()
                .position(|member| *member == id)
        {
            expanded.scroll.scroll_to_item(index);
        }
        let top = self.time_to_offset(bin.start) + self.scroll_offset + self.center_relative().y;
        if self.bounds.is_some_and(|bounds| {
            top + HEADER_HEIGHT <= TOP_EDGE_INSET || top >= bounds.size.height
        }) {
            self.scroll_to(bin.start, cx);
        }
        true
    }

    pub(super) fn sync_expanded_bin(
        &mut self,
        slots: &[TimelineSlot],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.bin_focus_handles.retain(|key, _| {
            slots
                .iter()
                .any(|slot| matches!(slot, TimelineSlot::Bin(bin) if bin.key == *key))
        });
        for slot in slots {
            if let TimelineSlot::Bin(bin) = slot {
                self.bin_focus_handles
                    .entry(bin.key)
                    .or_insert_with(|| cx.focus_handle());
            }
        }
        let Some(expanded) = &self.expanded_bin else {
            return;
        };
        let bins = || {
            slots.iter().filter_map(|slot| match slot {
                TimelineSlot::Bin(bin) => Some(bin),
                _ => None,
            })
        };
        let active = self
            .pending_item_focus
            .filter(|id| expanded.members.contains(id))
            .or_else(|| {
                self.items
                    .iter()
                    .find(|entry| {
                        expanded.members.contains(&entry.item.id())
                            && (self.is_being_edited(entry.item.id(), cx)
                                || entry.focus_handle.contains_focused(window, cx))
                    })
                    .map(|entry| entry.item.id())
            });
        if let Some(id) = active
            && let Some(entry) = slots.iter().find_map(|slot| match slot {
                TimelineSlot::Item { index, .. } if self.items[*index].item.id() == id => {
                    Some(&self.items[*index])
                }
                _ => None,
            })
        {
            let start = entry.item.start_datetime();
            let editing = self.is_being_edited(id, cx);
            self.expanded_bin = None;
            self.marquee.end();
            self.edge_scroll_speed = None;
            if !editing {
                self.pending_item_focus = Some(id);
            }
            if let Some(start) = start {
                let top =
                    self.time_to_offset(start) + self.scroll_offset + self.center_relative().y;
                if self.bounds.is_some_and(|bounds| {
                    top < TOP_EDGE_INSET || top + HEADER_HEIGHT > bounds.size.height
                }) {
                    self.scroll_to(start, cx);
                }
            }
            return;
        }
        let bin = active
            .and_then(|id| {
                bins().find(|bin| {
                    bin.members
                        .iter()
                        .any(|index| self.items[*index].item.id() == id)
                })
            })
            .or_else(|| bins().find(|bin| self.bin_is_expanded(bin)))
            .or_else(|| {
                bins().find(|bin| {
                    bin.members
                        .iter()
                        .any(|index| expanded.members.contains(&self.items[*index].item.id()))
                })
            });
        if let Some(bin) = bin {
            let members = self.bin_members(bin);
            let expanded = self.expanded_bin.as_mut().unwrap();
            let active = active.filter(|id| members.contains(id));
            if let Some(id) = active {
                expanded.anchor = id;
                if (expanded.focused_item != active || expanded.members != members)
                    && let Some(index) = members.iter().position(|member| *member == id)
                {
                    expanded.scroll.scroll_to_item(index);
                }
            } else if !members.contains(&expanded.anchor) {
                expanded.anchor = members[0];
            }
            expanded.focused_item = active;
            if expanded.members != members {
                expanded.members = members;
                self.marquee.end();
                self.edge_scroll_speed = None;
            }
        } else if let Some(expanded) = self.expanded_bin.take() {
            if expanded.focus.is_focused(window) {
                self.focus_handle.focus(window, cx);
            }
            self.marquee.end();
            self.edge_scroll_speed = None;
        }
    }

    pub(super) fn render_bin(
        &self,
        bin: &TimelineBin,
        handoffs: &NavigationHandoffs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let members = self.bin_members(bin);
        let anchor = members[0];
        let expanded = self
            .expanded_bin
            .as_ref()
            .filter(|_| self.bin_is_expanded(bin));
        let open = expanded.is_some();
        let selection = SelectionManager::global(cx);
        let selected = members
            .iter()
            .filter(|id| {
                selection
                    .read(cx)
                    .is_selected_in(SelectionScope::Timeline, **id)
            })
            .count();
        let all_selected = selected == members.len();
        let order = self.selection_order();
        let focus = expanded.map_or_else(
            || self.bin_focus_handles[&bin.key].clone(),
            |bin| bin.focus.clone(),
        );
        let scroll_y = self.scroll_offset + self.center_relative().y;
        let collapsed = self.lane_bounds(bin.start, ChronoDuration::zero(), bin.lane);
        let mut target = collapsed;
        if let Some(expanded) = expanded {
            let content_height = bin
                .members
                .iter()
                .map(|index| {
                    self.bin_details
                        .height(self.items[*index].item.id(), SIDEBAR_ITEM_HEIGHT)
                })
                .sum::<Pixels>()
                + SIDEBAR_ITEM_GAP * members.len().saturating_sub(1) as f32;
            target = expanded_bounds(
                collapsed + point(px(0.), scroll_y),
                self.bounds
                    .map_or(size(px(800.), px(800.)), |bounds| bounds.size),
                content_height,
                expanded.expands_left,
                expanded.expands_up,
            ) - point(px(0.), scroll_y);
        }
        let bounds = self.transition_bounds(SlotKind::Bin, bin.key, target, false, window, cx)
            + point(px(0.), scroll_y);
        let accent = cx.theme().colors.focus;
        let muted = cx.theme().colors.text_muted;
        let hairline = cx.theme().colors.hairline;
        let preview = if selected > 0 {
            format!("{selected} of {} selected", members.len()).into()
        } else if open {
            if bin.start.date_naive() == bin.end.date_naive() {
                format!(
                    "{} · {} – {}",
                    bin.start.format("%b %-d"),
                    bin.start.format("%-H:%M"),
                    bin.end.format("%-H:%M")
                )
                .into()
            } else {
                format!(
                    "{} – {}",
                    bin.start.format("%b %-d"),
                    bin.end.format("%b %-d")
                )
                .into()
            }
        } else {
            self.items[bin.members[0]].cached_title.clone()
        };

        let select_members = members.clone();
        let select_order = order.clone();
        let click_members = members.clone();
        let click_order = order.clone();
        let key_members = members.clone();
        let key_order = order.clone();
        let header_focus = focus.clone();
        let key_focus = focus.clone();
        let header = div()
            .row()
            .flex_none()
            .h(HEADER_HEIGHT)
            .items_center()
            .gap_1()
            .px_2()
            .when(open, |header| header.border_b_1().border_color(hairline))
            .child(
                div()
                    .id(("timeline-bin-disclosure", bin.key))
                    .row()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        SelectionManager::claim_press(cx);
                        header_focus.focus(window, cx);
                        cx.stop_propagation();
                    })
                    .on_click(cx.listener(move |view, _, window, cx| {
                        cx.stop_propagation();
                        let gesture = Settings::global(cx).selection.gesture(&window.modifiers());
                        if gesture == SelectionGesture::Replace {
                            view.toggle_bin(anchor, window, cx);
                        } else {
                            SelectionManager::global(cx).update(cx, |selection, cx| {
                                selection.select_group(&click_order, &click_members, gesture, cx)
                            });
                        }
                    }))
                    .child(
                        div()
                            .column()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                Label::new(format!("{} items", members.len()))
                                    .text_sm()
                                    .truncate(),
                            )
                            .child(
                                Label::new(preview)
                                    .text_xs()
                                    .text_color(if selected > 0 { accent } else { muted })
                                    .truncate(),
                            ),
                    )
                    .child(
                        Icon::new(if open {
                            AppIcon::ChevronUp
                        } else {
                            AppIcon::ChevronDown
                        })
                        .size_3()
                        .flex_none()
                        .text_color(muted),
                    ),
            )
            .when(open, |header| {
                header.child(
                    Button::new(("timeline-bin-select", bin.key))
                        .ghost()
                        .small()
                        .label(if all_selected {
                            "Deselect all"
                        } else {
                            "Select all"
                        })
                        .tooltip(if all_selected {
                            "Deselect items in this bin"
                        } else {
                            "Select all items in this bin"
                        })
                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                            SelectionManager::claim_press(cx);
                            cx.stop_propagation();
                        })
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            SelectionManager::global(cx).update(cx, |selection, cx| {
                                selection.select_group(
                                    &select_order,
                                    &select_members,
                                    SelectionGesture::Toggle,
                                    cx,
                                )
                            });
                        }),
                )
            });

        let mut frame = div()
            .id(("timeline-bin", bin.key))
            .absolute()
            .top(bounds.top())
            .left(bounds.left())
            .w(bounds.size.width)
            .h(bounds.size.height)
            .column()
            .overflow_hidden()
            .rounded_xl()
            .border_1()
            .border_color(if selected > 0 { accent } else { hairline })
            .bg(cx.theme().colors.panel)
            .text_color(cx.theme().colors.text)
            .when_else(open, |frame| frame.shadow_lg(), |frame| frame.shadow_sm())
            .block_mouse_except_scroll()
            .capture_any_mouse_down(cx.listener(
                move |view, event: &gpui::MouseDownEvent, _, cx| {
                    view.force_press = None;
                    view.inspect_force_item = None;
                    view.inspect_press_item = if open && event.button == MouseButton::Left {
                        SelectionManager::global(cx)
                            .read(cx)
                            .card_at(SelectionScope::Timeline, event.position)
                    } else {
                        None
                    };
                },
            ))
            .track_focus(&focus)
            .tab_stop(true)
            .key_context("TimelineBin")
            .focus_ring(cx.theme())
            .on_action(
                cx.listener(|view, _: &CloseTimelineBin, window, cx| view.close_bin(window, cx)),
            )
            .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                if !key_focus.is_focused(window) || event.is_held {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "enter" => {
                        view.toggle_bin(anchor, window, cx);
                        cx.stop_propagation();
                    }
                    "space" => {
                        SelectionManager::global(cx).update(cx, |selection, cx| {
                            selection.select_group(
                                &key_order,
                                &key_members,
                                SelectionGesture::Toggle,
                                cx,
                            )
                        });
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            }))
            .when(open, |frame| {
                frame
                    .on_prepaint(|bounds, window, cx| {
                        SelectionManager::occlude_cards(
                            SelectionScope::Timeline,
                            bounds.intersect(&window.content_mask().bounds),
                            cx,
                        );
                    })
                    .on_mouse_down_out(cx.listener(
                        |view, event: &gpui::MouseDownEvent, window, cx| {
                            if !cx.has_active_drag()
                                && !crate::components::menu::context_menu_contains_position(
                                    event.position,
                                    cx,
                                )
                            {
                                view.close_bin(window, cx);
                            }
                        },
                    ))
            })
            .child(header);

        if let Some(expanded) = expanded {
            let rows = members
                .iter()
                .filter_map(|id| self.items.iter().find(|entry| entry.item.id() == *id))
                .map(|entry| {
                    let id = entry.item.id();
                    let card = ItemCard::new_with_id(
                        ("timeline-bin-item", entry.item.id_u64()),
                        &entry.item,
                        entry.cached_meta.clone(),
                        window,
                        cx,
                    )
                    .with_focus_handle(entry.focus_handle.clone())
                    .selectable(order.clone())
                    .next_focus(handoffs.get(&id).cloned().flatten())
                    .details(self.bin_details.get(id))
                    .border(false)
                    .draggable(true, None)
                    .w_full()
                    .h(self.bin_details.height(id, SIDEBAR_ITEM_HEIGHT))
                    .flex_none();
                    self.wire_timeline_item_card(card, &entry.item, cx)
                        .into_any_element()
                })
                .collect::<Vec<_>>();
            frame = frame
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .child(
                            div()
                                .id(("timeline-bin-entries", bin.key))
                                .column()
                                .size_full()
                                .gap(SIDEBAR_ITEM_GAP)
                                .p(LIST_PADDING)
                                .overflow_y_scroll()
                                .track_scroll(&expanded.scroll)
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(
                                        |view, event: &gpui::MouseDownEvent, window, cx| {
                                            let Some(bin) = &view.expanded_bin else {
                                                return;
                                            };
                                            if view.marquee.begin_in_scroll_region(
                                                event.position,
                                                Some(view.focus_handle.clone()),
                                                &bin.scroll,
                                                bin.members.iter().copied(),
                                                window,
                                                cx,
                                            ) {
                                                view.focus_handle.focus(window, cx);
                                                view.force_press = None;
                                                cx.stop_propagation();
                                                cx.notify();
                                            }
                                        },
                                    ),
                                )
                                .children(rows),
                        )
                        .child(Scrollbar::new(
                            ("timeline-bin-scrollbar", bin.key),
                            &expanded.scroll,
                            gpui::Axis::Vertical,
                        )),
                );
        } else {
            frame = frame.on_prepaint(move |bounds, window, cx| {
                let visible = bounds.intersect(&window.content_mask().bounds);
                if !visible.is_empty() {
                    for id in members {
                        SelectionManager::report_card(SelectionScope::Timeline, id, visible, cx);
                    }
                }
            });
        }
        frame.into_any_element()
    }
}
