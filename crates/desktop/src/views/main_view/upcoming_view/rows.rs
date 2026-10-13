use crate::AppIcon;
use crate::components::{Button, ButtonVariants, DragData, DraggedItems, Label};
use crate::icons::Icon;
use chrono::{Datelike, NaiveDate};
use gpui::{
    AnyElement, Context, Div, DragMoveEvent, ElementId, FontWeight, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, ParentElement, Pixels, SharedString, Stateful,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px, size,
};
use gpui_kit::foundation::{Disableable as _, FocusRing as _, StyledExt as _};
use gpui_kit_theme::ActiveTheme;
use subroutine_core::{AnyItem, SchedulePoint};
use uuid::Uuid;

use crate::{
    components::{
        ItemCard,
        transition::{self, WindowTransitionExt as _},
    },
    item_manager::ItemManager,
    selection::SelectionManager,
    settings::Settings,
};

use super::{
    UpcomingDropTarget, UpcomingSection, UpcomingView,
    agenda::{AgendaMarker, CreateKind, UpcomingRow},
    headers::{CONTROLS_WIDTH, HEADER_GAP, HEADER_PADDING, MARKER_GAP, MARKER_HEIGHT},
};

fn card_row(id: impl Into<ElementId>, height: Pixels) -> Stateful<Div> {
    div().id(id).relative().w_full().h(height).px_4()
}

impl UpcomingView {
    pub(super) fn render_row(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(row) = self
            .layout
            .rows
            .get(ix)
            .and_then(|source| self.agenda.rows.get(*source))
            .cloned()
        else {
            return div().into_any_element();
        };
        let interactive = self.row_interactive(ix);
        match row {
            UpcomingRow::Missed => self.render_missed(cx),
            UpcomingRow::Day { date, markers, .. } => self.render_day(date, &markers, window, cx),
            UpcomingRow::Item {
                item, projected, ..
            } => self.render_item(
                item,
                projected,
                self.agenda.item_neighbors(self.layout.rows[ix]),
                interactive,
                window,
                cx,
            ),
            UpcomingRow::Create { date, kind } => {
                self.render_create(date, kind, interactive, window, cx)
            }
        }
    }

    fn render_item(
        &mut self,
        item: AnyItem,
        projected: bool,
        (previous, next): (Option<Uuid>, Option<Uuid>),
        interactive: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = item.id();
        let card_height = self.card_height(&item);
        let meta = super::super::format_item_meta(&item);
        let meta = match (
            UpcomingSection::for_item(&item, self.today),
            item.start_date(),
            meta,
        ) {
            (Some(UpcomingSection::Missed), Some(date), Some(meta)) => {
                Some(format!("{} · {meta}", date.format("%b %-d, %Y")).into())
            }
            (_, _, meta) => meta,
        };
        let handle = (!projected && interactive)
            .then(|| self.item_focus_handles.get(&id).cloned())
            .flatten();
        let order = self.order.clone();
        let date = item.start_date();
        let anchor = match item.start() {
            Some(SchedulePoint::DateTime(start)) => Some(start),
            _ => None,
        };
        let after_anchor = anchor.map(|start| {
            let duration = item.duration().or_else(|| {
                matches!(&item, AnyItem::Action(_))
                    .then_some(Settings::global(cx).schedule.default_action_duration)
            });
            duration
                .map(|duration| {
                    (start.with_timezone(&chrono::Local) + duration).with_timezone(&chrono::Utc)
                })
                .unwrap_or(start + chrono::Duration::minutes(1))
        });
        let before_active = matches!(
            self.drop_target,
            Some(UpcomingDropTarget::Before { item, .. }) if item == id
        );
        let after_active = matches!(
            self.drop_target,
            Some(UpcomingDropTarget::After { item, .. }) if item == id
        );
        let day_item_active = matches!(
            self.drop_target,
            Some(UpcomingDropTarget::OnDayItem { item, .. }) if item == id
        );
        let focus = cx.theme().colors.focus;

        card_row(("upcoming-item-drop", id.as_u64_pair().1), card_height)
            .when_some(date.filter(|_| interactive), |this, date| {
                this.on_drag_move::<DragData<DraggedItems>>(cx.listener(
                    move |view, event: &DragMoveEvent<DragData<DraggedItems>>, _, cx| {
                        let dragged = &event.drag(cx).data;
                        if !event.bounds.contains(&event.event.position)
                            || dragged.items.is_empty()
                            || dragged.items.iter().any(|item| item.id() == id)
                        {
                            return;
                        }
                        let target = if dragged.source_anchor.is_some()
                            || dragged.items.iter().any(|item| {
                                matches!(item, AnyItem::Marker(_) | AnyItem::Routine(_))
                            }) {
                            UpcomingDropTarget::OnDayItem { date, item: id }
                        } else {
                            match anchor {
                                Some(anchor) => {
                                    let before = event.event.position.y
                                        < event.bounds.origin.y + event.bounds.size.height / 2.;
                                    if before {
                                        UpcomingDropTarget::Before {
                                            date,
                                            item: id,
                                            anchor: Some(anchor),
                                        }
                                    } else {
                                        UpcomingDropTarget::After {
                                            date,
                                            item: id,
                                            anchor: after_anchor,
                                        }
                                    }
                                }
                                None => UpcomingDropTarget::OnDayItem { date, item: id },
                            }
                        };
                        view.set_drop_target(target, cx);
                    },
                ))
                .on_drop::<DragData<DraggedItems>>(cx.listener(
                    move |view, data: &DragData<DraggedItems>, window, cx| {
                        if data.data.items.is_empty()
                            || data.data.items.iter().any(|item| item.id() == id)
                        {
                            view.clear_drop_target(cx);
                            return;
                        }
                        let target = match view.drop_target {
                            Some(target @ UpcomingDropTarget::Before { item, .. })
                            | Some(target @ UpcomingDropTarget::After { item, .. })
                            | Some(target @ UpcomingDropTarget::OnDayItem { item, .. })
                                if item == id =>
                            {
                                target
                            }
                            _ => UpcomingDropTarget::Day(date),
                        };
                        view.commit_drop(&data.data, target, window, cx);
                        cx.stop_propagation();
                    },
                ))
            })
            .child(
                ItemCard::new(&item, meta, window, cx)
                    .neighbors(previous, next)
                    .details(self.details.get(id))
                    .when_some(handle, |this, handle| {
                        this.with_focus_handle(handle).selectable(order)
                    })
                    .w_full()
                    .h(card_height)
                    .flex_none()
                    .actionable(!projected)
                    .editable(!projected && interactive)
                    .tab_stop(!projected && interactive)
                    .when(
                        interactive
                            && projected
                            && matches!(&item, AnyItem::Event(event)
                            if event.source_provider.is_none() && event.recurrence.is_some()),
                        |card| {
                            card.projected_event_availability(
                                crate::stores::AppDatabaseStore::global(cx),
                            )
                        },
                    )
                    .draggable(!projected && interactive, None)
                    .when(projected, |this| this.opacity(0.55))
                    .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                        if projected || !interactive {
                            return;
                        }
                        if event.is_held || ItemManager::global(cx).read(cx).is_being_edited(id) {
                            return;
                        }
                        let key = event.keystroke.key.as_str();
                        let delta = match key {
                            "down" | "j" => 1,
                            "up" | "k" => -1,
                            _ => return,
                        };
                        let extend_selection =
                            event.keystroke.modifiers.shift && matches!(key, "up" | "down");
                        cx.stop_propagation();
                        view.move_cursor_from(id, delta, extend_selection, window, cx);
                    })),
            )
            .when(before_active, |this| {
                this.child(
                    div()
                        .absolute()
                        .top(px(0.))
                        .left_4()
                        .right_4()
                        .h(px(2.))
                        .rounded_full()
                        .bg(focus),
                )
            })
            .when(after_active, |this| {
                this.child(
                    div()
                        .absolute()
                        .bottom(px(0.))
                        .left_4()
                        .right_4()
                        .h(px(2.))
                        .rounded_full()
                        .bg(focus),
                )
            })
            .when(day_item_active, |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .mx_4()
                        .rounded_xl()
                        .border_1()
                        .border_color(focus),
                )
            })
            .into_any_element()
    }
}

impl UpcomingView {
    fn section_header(&self, section: UpcomingSection, cx: &Context<Self>) -> Stateful<Div> {
        let id = match section {
            UpcomingSection::Missed => ElementId::from("upcoming-missed"),
            UpcomingSection::Day(date) => {
                ElementId::from(("upcoming-day", date.num_days_from_ce() as u64))
            }
        };
        div().id(id).relative().row().size_full().px_4().when(
            self.sections.can_expand(section),
            |row| {
                row.cursor_pointer()
                    .focusable()
                    .tab_stop(true)
                    .focus_ring(cx.theme())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        SelectionManager::claim_press(cx);
                    })
                    .on_click(cx.listener(move |view, _, window, cx| {
                        cx.stop_propagation();
                        view.toggle_section(section, window, cx);
                    }))
            },
        )
    }

    fn section_label(
        &self,
        section: UpcomingSection,
        label: String,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let colors = &cx.theme().colors;
        let today = section == UpcomingSection::Day(self.today);
        let expandable = self.sections.can_expand(section);
        let expanded = self.sections.is_expanded(section);
        let id = match section {
            UpcomingSection::Missed => ElementId::from("upcoming-toggle-missed"),
            UpcomingSection::Day(date) => {
                ElementId::from(("upcoming-toggle-day", date.num_days_from_ce() as u64))
            }
        };
        let tooltip = if expandable {
            format!("{} {label}", if expanded { "Collapse" } else { "Expand" })
        } else {
            label.clone()
        };
        div()
            .id(id)
            .row()
            .items_center()
            .justify_center()
            .flex_none()
            .h(MARKER_HEIGHT)
            .px(px(8.))
            .gap(px(4.))
            .min_w_0()
            .max_w_full()
            .flex_shrink_1()
            .child(
                Icon::new(if expanded {
                    AppIcon::ChevronDown
                } else {
                    AppIcon::ChevronRight
                })
                .size(px(12.))
                .when(!expandable, |icon| icon.invisible())
                .text_color(colors.text_muted.opacity(0.7)),
            )
            .tooltip(move |_, cx| {
                gpui_kit::overlay::Tooltip::new(tooltip.clone(), tooltip.clone()).view(cx)
            })
            .child(
                Label::new(label)
                    .h_auto()
                    .min_w_0()
                    .flex_shrink_1()
                    .when(matches!(section, UpcomingSection::Day(_)), |label| {
                        label.flex_1()
                    })
                    .truncate()
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .text_color(if today { colors.accent } else { colors.text }),
            )
            .when_some(
                match section {
                    UpcomingSection::Day(date) => Some(date),
                    _ => None,
                },
                |label, date| {
                    label
                        .w(self.headers.days[&date].date_width)
                        .flex_none()
                        .justify_start()
                        .child(
                            Label::new(date.format("%a").to_string())
                                .h_auto()
                                .flex_shrink_0()
                                .ml(px(4.))
                                .text_xs()
                                .text_color(colors.text_muted),
                        )
                },
            )
    }

    pub(super) fn section_controls(&self, cx: &Context<Self>) -> impl IntoElement {
        let mut sections = self
            .agenda
            .rows
            .iter()
            .filter(|row| row.is_heading())
            .map(UpcomingRow::section)
            .filter(|section| self.sections.can_expand(*section));
        let enabled = sections.clone().next().is_some();
        let expanded = !sections.any(|section| self.sections.is_expanded(section));
        let (icon, tooltip) = if expanded {
            (AppIcon::ListChevronsUpDown, "Expand all lists")
        } else {
            (AppIcon::ListChevronsDownUp, "Collapse all lists")
        };

        div()
            .id("upcoming-section-controls")
            .w(CONTROLS_WIDTH)
            .block_mouse_except_scroll()
            .on_click(|_, _, cx| cx.stop_propagation())
            .row()
            .flex_none()
            .items_center()
            .child(
                Button::new("upcoming-toggle-all")
                    .ghost()
                    .size(MARKER_HEIGHT)
                    .icon(Icon::new(icon).size_4())
                    .text_color(cx.theme().colors.text_muted)
                    .tooltip(tooltip)
                    .disabled(!enabled)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        SelectionManager::claim_press(cx)
                    })
                    .when(enabled, |button| {
                        button.on_click(cx.listener(move |view, _, window, cx| {
                            cx.stop_propagation();
                            view.set_all_expanded(expanded, window, cx);
                        }))
                    }),
            )
    }

    fn render_day(
        &mut self,
        date: NaiveDate,
        markers: &[AgendaMarker],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let inline_add = self.sections.add_in_header(&self.agenda, date);
        let header = &self.headers.days[&date];
        let marker_rows: Vec<_> = header
            .marker_rows
            .iter()
            .map(|range| {
                div()
                    .row()
                    .w_full()
                    .h(MARKER_HEIGHT)
                    .flex_none()
                    .justify_end()
                    .gap(MARKER_GAP)
                    .children(range.clone().map(|index| {
                        self.render_marker(
                            date,
                            &markers[index],
                            header.marker_widths[index],
                            window,
                            cx,
                        )
                    }))
            })
            .collect();
        let active = self.drop_target == Some(UpcomingDropTarget::Day(date));
        let active_bg = cx.theme().colors.selected;

        let day_row = self
            .section_header(UpcomingSection::Day(date), cx)
            .h_auto()
            .flex_1()
            .min_h_0()
            .py(HEADER_PADDING)
            .items_start()
            .gap(HEADER_GAP)
            .when(active, |this| this.bg(active_bg))
            .on_drag_move::<DragData<DraggedItems>>(cx.listener(
                move |view, event: &DragMoveEvent<DragData<DraggedItems>>, _, cx| {
                    if event.bounds.contains(&event.event.position)
                        && !event.drag(cx).data.items.is_empty()
                    {
                        view.set_drop_target(UpcomingDropTarget::Day(date), cx);
                    }
                },
            ))
            .on_drop::<DragData<DraggedItems>>(cx.listener(
                move |view, data: &DragData<DraggedItems>, window, cx| {
                    view.commit_drop(&data.data, UpcomingDropTarget::Day(date), window, cx);
                    cx.stop_propagation();
                },
            ))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |view, event, window, cx| {
                    view.open_context_menu(date, event, window, cx);
                }),
            )
            .child(
                div()
                    .row()
                    .flex_none()
                    .when(inline_add, |slot| slot.w(self.headers.side_width))
                    .child(self.section_label(
                        UpcomingSection::Day(date),
                        header.label.clone(),
                        cx,
                    )),
            )
            .when(inline_add, |row| {
                row.child(self.add_button(date, true, window, cx).h(MARKER_HEIGHT))
            })
            .child(
                div()
                    .column()
                    .flex_1()
                    .min_w_0()
                    .gap(MARKER_GAP)
                    .children(marker_rows),
            );

        div().column().size_full().child(day_row).into_any_element()
    }
}

#[derive(Clone, Copy, Default)]
enum MarkerFocusReveal {
    #[default]
    Idle,
    Pending,
    Settled,
}

impl UpcomingView {
    fn render_marker(
        &self,
        date: NaiveDate,
        entry: &AgendaMarker,
        width: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let item = AnyItem::Marker(entry.marker.clone());
        let id = item.id();
        let projected = entry.projected;
        let first_date = self
            .layout
            .positions
            .get(&id)
            .and_then(|index| self.agenda.rows.get(self.layout.rows[*index]))
            .and_then(|row| match row {
                UpcomingRow::Day { date, .. } => Some(*date),
                _ => None,
            });
        let handle = (first_date == Some(date))
            .then(|| self.item_focus_handles.get(&id).cloned())
            .flatten();
        let editing = ItemManager::global(cx).read(cx).is_being_edited(id);
        let edit_date = self
            .marker_edit
            .filter(|(item, _)| *item == id)
            .map(|(_, date)| date)
            .or(first_date);
        let editable = !projected && (!editing || edit_date == Some(date));
        let scroll = self.scroll_handle.clone();
        let owner = window.current_view();
        let tooltip = if projected {
            format!("{} (projected)", entry.marker.title)
        } else {
            format!("{} — double-click to edit", entry.marker.title)
        };

        let element_id =
            ElementId::from(SharedString::from(format!("upcoming-marker-{date}-{id}")));
        let focus_reveal =
            window.use_keyed_state((element_id.clone(), "focus-reveal"), cx, |_, _| {
                MarkerFocusReveal::default()
            });

        ItemCard::new_with_id(element_id, &item, None, window, cx)
            .compact(true)
            .subtle()
            .w(width)
            .h(MARKER_HEIGHT)
            .flex_none()
            .border(false)
            .actionable(!projected)
            .editable(editable)
            .draggable(!projected, Some(size(width, MARKER_HEIGHT)))
            .when(!projected, |card| {
                card.selectable(self.order.clone())
                    .when_some(handle, |card, handle| card.with_focus_handle(handle))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |view, _, _, cx| {
                            if !ItemManager::global(cx).read(cx).is_editing() {
                                view.marker_edit = Some((id, date));
                            }
                        }),
                    )
                    .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                        if event.is_held || ItemManager::global(cx).read(cx).is_being_edited(id) {
                            return;
                        }
                        if !ItemManager::global(cx).read(cx).is_editing() {
                            view.marker_edit = Some((id, date));
                        }
                        let key = event.keystroke.key.as_str();
                        let delta = match key {
                            "down" | "j" => 1,
                            "up" | "k" => -1,
                            _ => return,
                        };
                        let extend =
                            event.keystroke.modifiers.shift && matches!(key, "up" | "down");
                        cx.stop_propagation();
                        view.move_cursor_from(id, delta, extend, window, cx);
                    }))
            })
            .when(projected, |card| card.opacity(0.55))
            .on_focus_resolved(move |bounds, handle, window, cx| {
                let focused = handle.is_some_and(|handle| handle.contains_focused(window, cx));
                let reveal = focus_reveal.update(cx, |state, _| {
                    match (focused, *state) {
                        (false, _) => *state = MarkerFocusReveal::Idle,
                        (true, MarkerFocusReveal::Idle) => {
                            // Let the virtual list reveal the heading before its wrapped marker.
                            *state = MarkerFocusReveal::Pending;
                            window.on_next_frame(move |_, cx| cx.notify(owner));
                        }
                        (true, MarkerFocusReveal::Pending) => {
                            *state = MarkerFocusReveal::Settled;
                            return true;
                        }
                        (true, MarkerFocusReveal::Settled) => {}
                    }
                    false
                });
                if !reveal {
                    return;
                }
                let viewport = scroll.bounds();
                let offset = if bounds.top() < viewport.top() {
                    viewport.top() - bounds.top()
                } else if bounds.bottom() > viewport.bottom() {
                    viewport.bottom() - bounds.bottom()
                } else {
                    px(0.)
                };
                if scroll.scroll_by_y(offset) != px(0.) {
                    window.refresh();
                }
            })
            .tooltip(move |_, cx| {
                gpui_kit::overlay::Tooltip::new(tooltip.clone(), tooltip.clone()).view(cx)
            })
            .block_mouse_except_scroll()
            .into_any_element()
    }
}

impl UpcomingView {
    fn add_button(
        &self,
        date: NaiveDate,
        interactive: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Button {
        let id = ElementId::from(("upcoming-create", date.num_days_from_ce() as u32));
        let tooltip = format!("Add an item on {}", date.format("%A, %-d %B"));
        let visible = interactive && self.hovered_section == Some(UpcomingSection::Day(date));
        let fade =
            window.keyed_transition((id.clone(), "hover-fade"), cx, transition::QUICK, || 0.0);
        fade.set(if visible { 1.0 } else { 0.0 }, cx);
        let opacity = fade.animate(window, cx);

        Button::new(id)
            .ghost()
            .block_mouse_except_scroll()
            .opacity(opacity)
            .focus(|style| style.opacity(1.0))
            .w(self.headers.add_width)
            .h_8()
            .px_3()
            .gap_1()
            .rounded_full()
            .icon(Icon::new(AppIcon::Plus).size_4())
            .label("Add item")
            .text_sm()
            .font_weight(FontWeight::NORMAL)
            .text_color(cx.theme().colors.text_muted)
            .tooltip(tooltip)
            .when(interactive, |button| {
                button
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        SelectionManager::claim_press(cx);
                    })
                    .on_click(cx.listener(move |view, _, window, cx| {
                        cx.stop_propagation();
                        view.add_draft(date, window, cx);
                    }))
            })
            .tab_stop(interactive)
    }

    fn render_create(
        &mut self,
        date: NaiveDate,
        kind: CreateKind,
        interactive: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let target = UpcomingDropTarget::Day(date);
        let active = self.drop_target == Some(target);
        let row_id = ElementId::from(("upcoming-create-row", date.num_days_from_ce() as u64));
        let accepts = move |dragged: &DraggedItems| interactive && !dragged.items.is_empty();
        let button = if kind == CreateKind::Slot {
            ItemCard::create_slot(
                ("upcoming-create", date.num_days_from_ce() as u32),
                format!("New action for {}", date.format("%A, %B %-d, %Y")),
                cx,
            )
            .tooltip("New action")
            .disabled(!interactive)
            .tab_stop(interactive)
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                SelectionManager::claim_press(cx);
            })
            .when(interactive, |button| {
                button.on_click(cx.listener(move |view, _, window, cx| {
                    cx.stop_propagation();
                    view.add_draft(date, window, cx);
                }))
            })
            .into_any_element()
        } else {
            self.add_button(date, interactive, window, cx)
                .into_any_element()
        };

        card_row(row_id, kind.height())
            .row()
            .items_center()
            .justify_center()
            .when(active, |this| this.bg(cx.theme().colors.selected))
            .on_drag_move::<DragData<DraggedItems>>(cx.listener(
                move |view, event: &DragMoveEvent<DragData<DraggedItems>>, _, cx| {
                    if event.bounds.contains(&event.event.position) && accepts(&event.drag(cx).data)
                    {
                        view.set_drop_target(target, cx);
                    }
                },
            ))
            .on_drop::<DragData<DraggedItems>>(cx.listener(
                move |view, data: &DragData<DraggedItems>, window, cx| {
                    if accepts(&data.data) {
                        view.commit_drop(&data.data, target, window, cx);
                        cx.stop_propagation();
                    } else {
                        view.clear_drop_target(cx);
                    }
                },
            ))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |view, event, window, cx| {
                    view.open_context_menu(date, event, window, cx);
                }),
            )
            .child(button)
            .into_any_element()
    }
}

impl UpcomingView {
    fn render_missed(&self, cx: &Context<Self>) -> AnyElement {
        self.section_header(UpcomingSection::Missed, cx)
            .items_center()
            .justify_between()
            .child(self.section_label(UpcomingSection::Missed, "Missed actions".into(), cx))
            .into_any_element()
    }
}
