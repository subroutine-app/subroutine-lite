use crate::AppIcon;
use crate::components::{Button, ButtonVariants, DragData, DraggedItems, Label};
use crate::icons::Icon;
use chrono::{Datelike, NaiveDate};
use gpui::{
    AnyElement, App, Context, DragMoveEvent, ElementId, FontWeight, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, ParentElement, StatefulInteractiveElement, Styled,
    Window, div, prelude::FluentBuilder, px, size,
};
use gpui_kit::foundation::{Disableable as _, StyledExt as _};
use gpui_kit_theme::ActiveTheme;
use subroutine_core::{AnyItem, SchedulePoint};

use crate::{
    components::ext::InteractiveElementExt as _,
    components::{Draggable, ItemCard, create_drag_data, item_context_menu},
    item_manager::ItemManager,
    selection::SelectionManager,
    settings::Settings,
};

use super::{
    QueueDropTarget, QueueSection, QueueView,
    agenda::{AgendaMarker, ITEM_ROW_PADDING, MARKER_ROW_HEIGHT, QueueRow},
};

impl QueueView {
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
            QueueRow::Missed => self.render_missed(cx),
            QueueRow::Day {
                date,
                markers,
                relative,
            } => self.render_day(date, &markers, relative, cx),
            QueueRow::Item {
                item, projected, ..
            } => self.render_item(item, projected, interactive, window, cx),
            QueueRow::Create { date } => self.render_create(date, interactive, cx),
            QueueRow::AnyTime => self.render_any_time(cx),
        }
    }

    fn render_item(
        &mut self,
        item: AnyItem,
        projected: bool,
        interactive: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = item.id();
        let card_height = self.card_height(&item);
        let meta = super::super::format_item_meta(&item);
        let meta = match (
            QueueSection::for_item(&item, self.today),
            item.start_date(),
            meta,
        ) {
            (QueueSection::Missed, Some(date), Some(meta)) => {
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
            Some(QueueDropTarget::Before { item, .. }) if item == id
        );
        let after_active = matches!(
            self.drop_target,
            Some(QueueDropTarget::After { item, .. }) if item == id
        );
        let day_item_active = matches!(
            self.drop_target,
            Some(QueueDropTarget::OnDayItem { item, .. }) if item == id
        );
        let focus = cx.theme().colors.focus;

        div()
            .id(("queue-item-drop", id.as_u64_pair().1))
            .relative()
            .w_full()
            .h(card_height + ITEM_ROW_PADDING * 2.)
            .px_4()
            .py(ITEM_ROW_PADDING)
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
                            QueueDropTarget::OnDayItem { date, item: id }
                        } else {
                            match anchor {
                                Some(anchor) => {
                                    let before = event.event.position.y
                                        < event.bounds.origin.y + event.bounds.size.height / 2.;
                                    if before {
                                        QueueDropTarget::Before {
                                            date,
                                            item: id,
                                            anchor: Some(anchor),
                                        }
                                    } else {
                                        QueueDropTarget::After {
                                            date,
                                            item: id,
                                            anchor: after_anchor,
                                        }
                                    }
                                }
                                None => QueueDropTarget::OnDayItem { date, item: id },
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
                            Some(target @ QueueDropTarget::Before { item, .. })
                            | Some(target @ QueueDropTarget::After { item, .. })
                            | Some(target @ QueueDropTarget::OnDayItem { item, .. })
                                if item == id =>
                            {
                                target
                            }
                            _ => QueueDropTarget::Day(date),
                        };
                        view.commit_drop(&data.data, target, window, cx);
                        cx.stop_propagation();
                    },
                ))
            })
            .child(
                ItemCard::new(&item, meta, window, cx)
                    .details(self.details.get(id))
                    .when_some(handle, |this, handle| {
                        this.with_focus_handle(handle).selectable(order)
                    })
                    .w_full()
                    .h(card_height)
                    .flex_none()
                    .actionable(!projected && interactive)
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
                    .border(false)
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
                        .my(ITEM_ROW_PADDING)
                        .rounded_xl()
                        .border_1()
                        .border_color(focus),
                )
            })
            .into_any_element()
    }
}

impl QueueView {
    fn section_toggle(&self, section: QueueSection, label: String, cx: &Context<Self>) -> Button {
        let expanded = self.sections.is_expanded(section);
        let id = match section {
            QueueSection::Missed => ElementId::from("queue-toggle-missed"),
            QueueSection::Unscheduled => ElementId::from("queue-toggle-unscheduled"),
            QueueSection::Day(date) => {
                ElementId::from(("queue-toggle-day", date.num_days_from_ce() as u64))
            }
        };
        Button::new(id)
            .ghost()
            .h_7()
            .px_1()
            .gap_1()
            .min_w_0()
            .icon(
                Icon::new(if expanded {
                    AppIcon::ChevronDown
                } else {
                    AppIcon::ChevronRight
                })
                .size_4(),
            )
            .tooltip(format!(
                "{} {label}",
                if expanded { "Collapse" } else { "Expand" }
            ))
            .child(
                Label::new(label)
                    .h_auto()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .text_color(cx.theme().colors.text),
            )
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                SelectionManager::claim_press(cx)
            })
            .on_click(cx.listener(move |view, _, window, cx| {
                cx.stop_propagation();
                view.toggle_section(section, window, cx);
            }))
    }

    fn section_controls(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .id("queue-section-controls")
            .on_click(|_, _, cx| cx.stop_propagation())
            .row()
            .items_center()
            .gap_0p5()
            .children([true, false].into_iter().map(|expanded| {
                let (id, icon, tooltip) = if expanded {
                    (
                        "queue-expand-all",
                        AppIcon::ListChevronsUpDown,
                        "Expand all lists",
                    )
                } else {
                    (
                        "queue-collapse-all",
                        AppIcon::ListChevronsDownUp,
                        "Collapse all lists",
                    )
                };
                let enabled = self.agenda.rows.iter().any(|row| {
                    row.is_heading() && self.sections.is_expanded(row.section()) != expanded
                });
                Button::new(id)
                    .ghost()
                    .size_7()
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
                    })
            }))
    }

    fn render_day(
        &mut self,
        date: NaiveDate,
        markers: &[AgendaMarker],
        relative: Option<&'static str>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        const VISIBLE_MARKERS: usize = 2;

        let theme = cx.theme();

        let calendar_date = match relative {
            Some(relative) => relative.to_owned(),
            None if date.year() == chrono::Local::now().year() => date.format("%B %-d").to_string(),
            None => date.format("%B %-d, %Y").to_string(),
        };
        let weekday = date.format("%A").to_string();
        let marker_chips: Vec<_> = markers
            .iter()
            .take(VISIBLE_MARKERS)
            .map(|entry| marker_chip(entry, cx))
            .collect();
        let hidden_markers = markers.len().saturating_sub(VISIBLE_MARKERS);
        let has_markers = !marker_chips.is_empty();
        let active = self.drop_target == Some(QueueDropTarget::Day(date));
        let active_bg = theme.colors.selected;

        div()
            .id(("queue-day", date.num_days_from_ce() as u64))
            .column()
            .size_full()
            .px_4()
            .justify_center()
            .gap_0p5()
            .when(active, |this| this.bg(active_bg))
            .on_drag_move::<DragData<DraggedItems>>(cx.listener(
                move |view, event: &DragMoveEvent<DragData<DraggedItems>>, _, cx| {
                    if event.bounds.contains(&event.event.position)
                        && !event.drag(cx).data.items.is_empty()
                    {
                        view.set_drop_target(QueueDropTarget::Day(date), cx);
                    }
                },
            ))
            .on_drop::<DragData<DraggedItems>>(cx.listener(
                move |view, data: &DragData<DraggedItems>, window, cx| {
                    view.commit_drop(&data.data, QueueDropTarget::Day(date), window, cx);
                    cx.stop_propagation();
                },
            ))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |view, event, window, cx| {
                    view.open_context_menu(Some(date), event, window, cx);
                }),
            )
            .on_double_click(cx.listener(move |view, _, window, cx| {
                cx.stop_propagation();
                view.add_draft(Some(date), window, cx);
            }))
            .child(
                div()
                    .row()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(self.section_toggle(QueueSection::Day(date), calendar_date, cx))
                    .child(
                        Label::new(weekday)
                            .h_auto()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(theme.colors.text_muted),
                    ),
            )
            .when(has_markers, |this| {
                this.child(
                    div()
                        .row()
                        .id(("queue-day-markers", date.num_days_from_ce() as u64))
                        .w_full()
                        .h(MARKER_ROW_HEIGHT)
                        .min_w_0()
                        .items_center()
                        .gap_0p5()
                        .overflow_hidden()
                        .when(hidden_markers > 0, |this| {
                            this.child(
                                Label::new(format!("+{hidden_markers}"))
                                    .h_auto()
                                    .flex_shrink_0()
                                    .text_xs()
                                    .text_color(theme.colors.text_muted.opacity(0.7)),
                            )
                        })
                        .children(marker_chips),
                )
            })
            .into_any_element()
    }
}

fn marker_chip(entry: &AgendaMarker, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let title = match entry.marker.title.trim() {
        "" => "Untitled".to_string(),
        title => title.to_string(),
    };
    let chip = div()
        .row()
        .max_w(px(116.))
        .min_w_0()
        .flex_shrink_1()
        .overflow_hidden()
        .px_2()
        .py_0p5()
        .rounded_sm()
        .bg(theme.colors.raised)
        .when(entry.projected, |this| this.opacity(0.55))
        .child(
            Label::new(title)
                .h_auto()
                .min_w_0()
                .flex_1()
                .overflow_hidden()
                .truncate()
                .text_xs()
                .text_color(theme.colors.text_muted),
        );

    if entry.projected {
        return chip.into_any_element();
    }

    let item = AnyItem::Marker(entry.marker.clone());
    let drag_data = create_drag_data(
        DraggedItems::single(item.clone()),
        size(px(116.), MARKER_ROW_HEIGHT),
        cx,
    );
    let menu_item = item.clone();
    Draggable::new(("queue-marker-draggable", item.id_u64()), drag_data)
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            SelectionManager::claim_press(cx)
        })
        .on_mouse_down(MouseButton::Right, move |event, window, cx| {
            cx.stop_propagation();
            crate::components::menu::open_context_menu(
                item_context_menu(&menu_item, None, false, cx),
                event.position,
                window,
                cx,
            );
        })
        .h(MARKER_ROW_HEIGHT)
        .max_w(px(116.))
        .min_w_0()
        .child(chip)
        .into_any_element()
}

impl QueueView {
    fn render_create(
        &mut self,
        date: Option<NaiveDate>,
        interactive: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let target = date.map_or(QueueDropTarget::Unscheduled, QueueDropTarget::Day);
        let active = self.drop_target == Some(target);
        let muted = cx.theme().colors.text_muted;
        let (row_id, button_id, tooltip) = match date {
            Some(date) => (
                ElementId::from(("queue-create-row", date.num_days_from_ce() as u64)),
                ElementId::from(("queue-create", date.num_days_from_ce() as u32)),
                format!("Add an item on {}", date.format("%A, %-d %B")),
            ),
            None => (
                ElementId::from("queue-create-unscheduled-row"),
                ElementId::from("queue-new-unscheduled"),
                "Add an unscheduled queued action".to_owned(),
            ),
        };
        let accepts = move |dragged: &DraggedItems| {
            interactive
                && match date {
                    Some(_) => !dragged.items.is_empty(),
                    None => dragged.actions().next().is_some(),
                }
        };

        div()
            .id(row_id)
            .row()
            .size_full()
            .px_4()
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
            .child(
                Button::new(button_id)
                    .ghost()
                    .h_8()
                    .px_3()
                    .rounded_full()
                    .icon(Icon::new(AppIcon::Plus).size_4())
                    .label("Add item")
                    .text_sm()
                    .text_color(muted)
                    .tooltip(tooltip)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        SelectionManager::claim_press(cx);
                    })
                    .when(interactive, |button| {
                        button.on_click(cx.listener(move |view, _, window, cx| {
                            cx.stop_propagation();
                            view.add_draft(date, window, cx);
                        }))
                    })
                    .tab_stop(interactive),
            )
            .into_any_element()
    }
}

impl QueueView {
    fn render_missed(&self, cx: &Context<Self>) -> AnyElement {
        div()
            .id("queue-missed")
            .row()
            .size_full()
            .px_4()
            .items_center()
            .justify_between()
            .child(self.section_toggle(QueueSection::Missed, "Missed actions".into(), cx))
            .child(self.section_controls(cx))
            .into_any_element()
    }

    fn render_any_time(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let active = self.drop_target == Some(QueueDropTarget::Unscheduled);

        div()
            .id("queue-unscheduled")
            .row()
            .size_full()
            .px_4()
            .items_center()
            .justify_between()
            .when(active, |this| this.bg(cx.theme().colors.selected))
            .on_drag_move::<DragData<DraggedItems>>(cx.listener(
                |view, event: &DragMoveEvent<DragData<DraggedItems>>, _, cx| {
                    if event.bounds.contains(&event.event.position)
                        && event.drag(cx).data.actions().next().is_some()
                    {
                        view.set_drop_target(QueueDropTarget::Unscheduled, cx);
                    }
                },
            ))
            .on_drop::<DragData<DraggedItems>>(cx.listener(
                |view, data: &DragData<DraggedItems>, window, cx| {
                    view.commit_drop(&data.data, QueueDropTarget::Unscheduled, window, cx);
                    cx.stop_propagation();
                },
            ))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|view, event, window, cx| {
                    view.open_context_menu(None, event, window, cx);
                }),
            )
            .on_double_click(cx.listener(|view, _, window, cx| {
                cx.stop_propagation();
                view.add_draft(None, window, cx);
            }))
            .child(self.section_toggle(QueueSection::Unscheduled, "Unscheduled".into(), cx))
            .when(
                !matches!(self.agenda.rows.first(), Some(QueueRow::Missed)),
                |row| row.child(self.section_controls(cx)),
            )
            .into_any_element()
    }
}
