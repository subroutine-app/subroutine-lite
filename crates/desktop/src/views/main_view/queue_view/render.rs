use gpui::{
    AnyElement, App, Context, DragMoveEvent, Hsla, InteractiveElement, IntoElement, ParentElement,
    PathBuilder, Pixels, Point, Render, Size, StatefulInteractiveElement, Styled, Window, canvas,
    div, point, prelude::FluentBuilder as _, px, size,
};
use gpui_kit::foundation::StyledExt as _;
use gpui_kit_semantics::{NodeSpec, Role, Semantic as _};
use gpui_kit_theme::ActiveTheme;
use std::time::Instant;

use crate::{
    AppIcon,
    color::ColorExt as _,
    components::{
        DragData, DraggedItems, DynamicList, EmptyState, ItemCard,
        ext::ElementExt as _,
        transition::{self, WindowTransitionExt as _},
    },
    icons::Icon,
    presentation::UxColor,
    selection::{CompleteSelected, SelectionManager},
    views::TOP_EDGE_INSET,
};

use super::{
    CURRENT_HEIGHT, CardPose, CurrentItem, EventMoment, FADE, Handoff, HandoffPhase, MainViewTab,
    PROMOTE, PresentedItem, QueueLane, QueueView,
    column::{CARD_HEIGHT, LIST_PADDING, QueueColumn},
    drop,
};

const COLUMN_BORDER: Pixels = px(1.);
const COLUMN_HEADER: Pixels = px(36.);
const COLUMN_GAP: Pixels = px(8.);

pub(super) struct QueueLayout {
    width: Pixels,
    pub column_height: Pixels,
    column_width: Pixels,
    column_left: Pixels,
    gap: Pixels,
    current: CardPose,
}

impl QueueLayout {
    pub(super) fn new(viewport: Size<Pixels>, count: usize, current_height: Pixels) -> Self {
        let width = viewport.width.max(px(1.));
        let height = viewport.height.max(px(340.));
        let gap = if width < px(640.) { px(12.) } else { px(24.) };
        let count = count.max(1) as f32;
        let column_width = ((width - px(32.) - gap * (count - 1.)) / count).clamp(px(1.), px(292.));
        let columns_width = column_width * count + gap * (count - 1.);
        let current_size = size(
            (width - px(32.)).clamp(px(1.), px(440.)),
            current_height.clamp(CURRENT_HEIGHT, (height * 0.4).max(CURRENT_HEIGHT)),
        );
        Self {
            width,
            column_height: (height * 0.5).min(height - current_size.height - px(80.)),
            column_width,
            column_left: (width - columns_width) / 2.,
            gap,
            current: CardPose {
                center: point(width / 2., height - px(16.) - current_size.height / 2.),
                size: current_size,
            },
        }
    }

    fn column_x(&self, index: usize) -> Pixels {
        self.column_left + (self.column_width + self.gap) * index as f32
    }

    pub(super) fn source_card(&self, index: usize) -> CardPose {
        CardPose {
            center: point(
                self.column_x(index) + self.column_width / 2.,
                self.column_height - LIST_PADDING - COLUMN_BORDER - CARD_HEIGHT / 2.,
            ),
            size: size(
                (self.column_width - (LIST_PADDING + COLUMN_BORDER) * 2.).max(px(1.)),
                CARD_HEIGHT,
            ),
        }
    }

    fn pipe_end(&self) -> Point<Pixels> {
        point(
            self.width / 2.,
            self.current.center.y - self.current.size.height / 2. - px(32.),
        )
    }
}

fn lane_icon(lane: QueueLane) -> AppIcon {
    match lane {
        QueueLane::Missed => AppIcon::ClockAlert,
        QueueLane::Today => AppIcon::Calendar,
        QueueLane::Unscheduled => AppIcon::Inbox,
    }
}

fn lane_drop_target<E: InteractiveElement>(
    element: E,
    lane: QueueLane,
    cx: &mut Context<QueueView>,
) -> E {
    element
        .can_drop(move |value, _, cx| {
            value
                .downcast_ref::<DragData<DraggedItems>>()
                .is_some_and(|data| drop::accepts(&data.data, lane, cx))
        })
        .on_drop(
            cx.listener(move |view, data: &DragData<DraggedItems>, window, cx| {
                if drop::accepts(&data.data, lane, cx) {
                    cx.stop_propagation();
                    view.commit_lane_drop(&data.data, lane, window, cx);
                }
            }),
        )
}

fn column(
    column: &QueueColumn,
    index: usize,
    layout: &QueueLayout,
    priority: bool,
    dragging: bool,
    active: bool,
    cx: &mut Context<QueueView>,
) -> impl IntoElement {
    let color = if priority {
        UxColor::Selected.text(cx.theme())
    } else {
        cx.theme().colors.text_muted
    };
    let height =
        column.list.read(cx).content_height().min(
            (layout.column_height - COLUMN_BORDER * 2. - COLUMN_HEADER - COLUMN_GAP).max(px(0.)),
        );
    let lane = column.lane;
    div()
        .id(format!("queue-lane-{}", lane.id()))
        .map(|element| lane_drop_target(element, lane, cx))
        .on_drag_move(cx.listener(
            move |view, event: &DragMoveEvent<DragData<DraggedItems>>, _, cx| {
                let active = event.bounds.contains(&event.event.position)
                    && drop::accepts(&event.drag(cx).data, lane, cx);
                let target = if active {
                    Some(lane)
                } else {
                    view.drop_target.filter(|target| *target != lane)
                };
                if view.drop_target != target {
                    view.drop_target = target;
                    cx.notify();
                }
            },
        ))
        .absolute()
        .left(layout.column_x(index))
        .top_0()
        .w(layout.column_width)
        .h(layout.column_height)
        .semantic_in(
            cx,
            NodeSpec::new(format!("queue-lane-{}", column.lane.id()), Role::Group)
                .text(format!("{} queue", column.lane.label())),
        )
        .column()
        .gap(COLUMN_GAP)
        .child(column.hover_observer())
        .child(
            div()
                .row()
                .items_center()
                .gap_2()
                .h(COLUMN_HEADER)
                .px(LIST_PADDING)
                .flex_none()
                .text_sm()
                .text_color(color)
                .child(Icon::new(lane_icon(column.lane)).size_4())
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .column()
                        .child(div().truncate().child(column.lane.label()))
                        .when(dragging, |header| {
                            header.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.text_muted)
                                    .truncate()
                                    .child(match lane {
                                        QueueLane::Missed => "Move items out",
                                        QueueLane::Today => "Today · date only",
                                        QueueLane::Unscheduled => "No date or time",
                                    }),
                            )
                        }),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().colors.text_muted)
                        .child(column.len(cx).to_string()),
                )
                .when(
                    priority && !dragging && layout.column_width >= px(230.),
                    |header| {
                        header.child(
                            div()
                                .rounded_full()
                                .px_2()
                                .py_0p5()
                                .text_xs()
                                .bg(UxColor::Selected.wash(cx.theme()))
                                .child("Next"),
                        )
                    },
                ),
        )
        .child(
            div()
                .w_full()
                .flex_1()
                .min_h_0()
                .column()
                .justify_end()
                .rounded_xl()
                .border(COLUMN_BORDER)
                .border_color(if active {
                    cx.theme().colors.accent
                } else {
                    cx.theme().colors.hairline
                })
                .bg(if active {
                    UxColor::Selected.wash(cx.theme())
                } else {
                    cx.theme().colors.sunken.opacity(0.45)
                })
                .overflow_hidden()
                .when_else(
                    column.len(cx) == 0 && dragging && lane != QueueLane::Missed,
                    |body| {
                        body.justify_center()
                            .p(LIST_PADDING)
                            .items_center()
                            .text_xs()
                            .text_center()
                            .text_color(cx.theme().colors.text_muted)
                            .child(drop::hint(lane))
                    },
                    |body| body.child(DynamicList::new(&column.list).w_full().h(height)),
                ),
        )
        .when(dragging && lane != QueueLane::Missed, |column| {
            column.child(lane_drop_target(
                div()
                    .id(format!("queue-lane-drop-{}", lane.id()))
                    .absolute()
                    .inset_0()
                    .block_mouse_except_scroll(),
                lane,
                cx,
            ))
        })
}

fn stroke_pipe(
    start: Point<Pixels>,
    end: Point<Pixels>,
    width: Pixels,
    color: Hsla,
    window: &mut Window,
) {
    let mut path = PathBuilder::stroke(width);
    let middle = (start.y + end.y) / 2.;
    path.move_to(start);
    path.cubic_bezier_to(end, point(start.x, middle), point(end.x, middle));
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

fn pipes(layout: &QueueLayout, weights: Vec<(usize, f32)>, cx: &App) -> impl IntoElement {
    let paths = weights
        .into_iter()
        .map(|(index, weight)| {
            (
                point(
                    layout.column_x(index) + layout.column_width / 2.,
                    layout.column_height + px(6.),
                ),
                weight,
            )
        })
        .collect::<Vec<_>>();
    let end = layout.pipe_end();
    let idle = cx.theme().colors.hairline;
    let active = UxColor::Selected.color(cx.theme());
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let end = bounds.origin + end;
            for (start, weight) in &paths {
                let start = bounds.origin + *start;
                stroke_pipe(start, end, px(1.5), idle, window);
                if *weight > 0.001 {
                    stroke_pipe(start, end, px(8.), active.opacity(0.08 * weight), window);
                    stroke_pipe(start, end, px(2.25), active.opacity(*weight), window);
                }
            }
            if !paths.is_empty() {
                let weight = paths.iter().map(|(_, weight)| *weight).fold(0., f32::max);
                let mut arrow = PathBuilder::stroke(px(2.));
                arrow.move_to(end + point(px(-4.), px(-5.)));
                arrow.line_to(end);
                arrow.line_to(end + point(px(4.), px(-5.)));
                if let Ok(arrow) = arrow.build() {
                    window.paint_path(arrow, idle.mix(active, weight));
                }
            }
        },
    )
    .absolute()
    .size_full()
}

fn promoted_pose(origin: CardPose, target: CardPose, progress: f32) -> CardPose {
    let t = 1. - (1. - progress).powi(3);
    let middle = (origin.center.y + target.center.y) / 2.;
    let a = origin.center;
    let b = point(a.x, middle);
    let c = point(target.center.x, middle);
    let d = target.center;
    let center = a * (1. - t).powi(3)
        + b * (3. * (1. - t).powi(2) * t)
        + c * (3. * (1. - t) * t * t)
        + d * t.powi(3);
    CardPose {
        center,
        size: size(
            origin.size.width + (target.size.width - origin.size.width) * t,
            origin.size.height + (target.size.height - origin.size.height) * t,
        ),
    }
}

impl Handoff {
    fn presented(
        &self,
        incoming: Option<&CurrentItem>,
        target: CardPose,
        now: Instant,
    ) -> Option<PresentedItem> {
        let elapsed = now.saturating_duration_since(self.started);
        let elapsed = match &self.phase {
            HandoffPhase::Fading(outgoing) if elapsed < FADE => {
                return Some(PresentedItem {
                    opacity: outgoing.opacity * (1. - elapsed.as_secs_f32() / FADE.as_secs_f32()),
                    ..outgoing.clone()
                });
            }
            HandoffPhase::Fading(_) => elapsed.saturating_sub(FADE),
            HandoffPhase::Promoting => elapsed,
        };
        let progress = (elapsed.as_secs_f32() / PROMOTE.as_secs_f32()).clamp(0., 1.);
        let origin = self.origin.map(|origin| origin.pose).unwrap_or(CardPose {
            center: target.center - point(px(0.), px(16.)),
            size: target.size,
        });
        incoming.map(|item| PresentedItem {
            item: item.clone(),
            pose: promoted_pose(origin, target, progress),
            opacity: if self.origin.is_some() {
                1.
            } else {
                (progress * 4.).min(1.)
            },
        })
    }
}

impl QueueView {
    pub(super) fn presented(&self, now: Instant, cx: &App) -> Option<PresentedItem> {
        let layout = self.layout(self.visible_lanes(cx).len());
        match &self.handoff {
            Some(motion) => motion.presented(self.current.as_ref(), layout.current, now),
            None => self.current.as_ref().map(|item| PresentedItem {
                item: item.clone(),
                pose: layout.current,
                opacity: 1.,
            }),
        }
    }

    fn render_card(
        &self,
        item: &CurrentItem,
        pose: CardPose,
        opacity: f32,
        interactive: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = item.id();
        let card = match item {
            CurrentItem::Candidate(item) => ItemCard::new_with_id(
                ("execution-current", id.as_u64_pair().1),
                item,
                super::super::format_item_meta(item),
                window,
                cx,
            )
            .when(interactive, |card| card.selectable(self.order.clone()))
            .next_focus(
                self.next_candidate()
                    .map(|next| (next.id(), self.current_focus.clone())),
            )
            .schedule_navigation(),
            CurrentItem::Event(moment) => super::super::event_notice_card(
                ("execution-event", id.as_u64_pair().1),
                moment,
                window,
                cx,
            ),
        };
        div()
            .absolute()
            .left(pose.center.x - pose.size.width / 2.)
            .top(pose.center.y - pose.size.height / 2.)
            .w(pose.size.width)
            .h(pose.size.height)
            .opacity(opacity)
            .child(
                card.details(self.card_states.get(id))
                    .with_focus_handle(self.current_focus.clone())
                    .large_title(true)
                    .editable(false)
                    .actionable(interactive)
                    .tab_stop(interactive)
                    .draggable(interactive, Some(pose.size))
                    .size_full(),
            )
            .into_any_element()
    }

    fn current_label(&self) -> &'static str {
        match self.current.as_ref() {
            Some(CurrentItem::Event(EventMoment::InProgress { .. })) => {
                "Event in progress · queue paused"
            }
            Some(CurrentItem::Event(EventMoment::Upcoming { .. })) => {
                "Event starting soon · queue paused"
            }
            Some(CurrentItem::Candidate(_)) => "Current item",
            None => "All clear",
        }
    }
}

impl Render for QueueView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let animation_now = Instant::now();
        if !cx.has_active_drag() {
            self.clear_drop_target(cx);
        }
        self.advance_handoff(animation_now, cx);
        self.card_states.animate(window, cx);
        SelectionManager::report_order(&self.order, cx);
        let lanes = self.visible_lanes(cx);
        let layout = self.layout(lanes.len());
        let priority = self.next_lane();
        let weights = lanes
            .iter()
            .enumerate()
            .map(|(index, lane)| {
                let weight = window.keyed_transition(
                    format!("execution-route-{}", lane.id()),
                    cx,
                    transition::SCROLL,
                    || 0.,
                );
                weight.set(if priority == Some(*lane) { 1. } else { 0. }, cx);
                (index, weight.animate(window, cx))
            })
            .collect();
        let view = cx.entity();
        let bottom_lanes = self.bottom_lanes(cx);
        let mut diagram = div()
            .relative()
            .w_full()
            .flex_1()
            .min_h(px(340.))
            .on_prepaint(move |bounds, window, cx| {
                view.update(cx, |view, cx| {
                    if view.viewport != bounds.size {
                        if view.viewport.height != bounds.size.height {
                            view.scroll_pending.extend(bottom_lanes.iter().copied());
                        }
                        view.viewport = bounds.size;
                        cx.notify();
                    }
                    let pending = std::mem::take(&mut view.scroll_pending);
                    if !pending.is_empty() {
                        cx.on_next_frame(window, move |view, _, cx| {
                            for column in &view.columns {
                                if pending.contains(&column.lane) {
                                    column.scroll_to_priority(cx);
                                }
                            }
                        });
                    }
                });
            })
            .child(pipes(&layout, weights, cx));
        for (index, lane) in lanes.iter().enumerate() {
            let lane_column = self
                .columns
                .iter()
                .find(|column| column.lane == *lane)
                .unwrap();
            diagram = diagram.child(column(
                lane_column,
                index,
                &layout,
                priority == Some(*lane),
                self.dragging,
                self.drop_target == Some(*lane),
                cx,
            ));
        }
        if lanes.is_empty() {
            diagram = diagram.child(
                div()
                    .absolute()
                    .top_0()
                    .w_full()
                    .h(layout.column_height)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        EmptyState::new(Icon::new(AppIcon::ListChecks), "No actions waiting")
                            .child(
                                div()
                                    .text_color(cx.theme().colors.text_muted)
                                    .child("Future items stay in Upcoming."),
                            ),
                    ),
            );
        }
        let color = if self.snapshot.blocker.is_some() {
            UxColor::Attention
        } else {
            UxColor::Selected
        };
        diagram = diagram.child(
            div()
                .absolute()
                .top(layout.pipe_end().y + px(8.))
                .w_full()
                .flex()
                .justify_center()
                .text_xs()
                .text_color(color.text(cx.theme()))
                .child(self.current_label()),
        );
        if let Some(shown) = self.presented(animation_now, cx) {
            diagram = diagram.child(self.render_card(
                &shown.item,
                shown.pose,
                shown.opacity,
                self.handoff.is_none(),
                window,
                cx,
            ));
        } else if self.handoff.is_none() {
            diagram = diagram.child(
                div()
                    .absolute()
                    .left(layout.current.center.x - layout.current.size.width / 2.)
                    .top(layout.current.center.y - layout.current.size.height / 2.)
                    .w(layout.current.size.width)
                    .h(layout.current.size.height)
                    .rounded_xl()
                    .border_1()
                    .border_color(cx.theme().colors.hairline)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(cx.theme().colors.text_muted)
                    .child("Nothing to do right now"),
            );
        }
        if self.handoff.is_some() {
            window.request_animation_frame();
        }
        let count = self.snapshot.candidates.len();
        let summary = if self.snapshot.blocker.is_some() {
            format!("{count} waiting · paused for event")
        } else {
            format!("{count} ready now")
        };
        let blocked = self.snapshot.blocker.is_some();
        let root = div()
            .id("execution-queue")
            .semantic_in(
                cx,
                NodeSpec::new("execution-queue", Role::Group).text("Queue"),
            )
            .size_full()
            .column()
            .overflow_y_scroll()
            .bg(cx.theme().colors.canvas)
            .on_drag_move(cx.listener(
                |view, event: &DragMoveEvent<DragData<DraggedItems>>, _, cx| {
                    let dragging = drop::has_actions(&event.drag(cx).data, cx);
                    if view.dragging != dragging {
                        view.dragging = dragging;
                        cx.notify();
                    }
                },
            ))
            .capture_action::<CompleteSelected>(move |_, _, cx| {
                if blocked {
                    cx.stop_propagation();
                }
            })
            .child(div().h(TOP_EDGE_INSET).w_full().flex_none())
            .child(
                div()
                    .h(px(56.))
                    .flex_none()
                    .row()
                    .items_center()
                    .gap_3()
                    .px_6()
                    .child(
                        Icon::new(AppIcon::ListOrdered)
                            .size_5()
                            .text_color(UxColor::Selected.color(cx.theme())),
                    )
                    .child(div().text_lg().child("Queue"))
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.text_muted)
                            .child(summary),
                    ),
            )
            .child(diagram);
        self.tab_root(root, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::super::PromotionOrigin;
    use super::*;
    use std::time::Duration;
    use subroutine_core::{Action, AnyItem};

    fn candidate(title: &str) -> CurrentItem {
        CurrentItem::Candidate(AnyItem::Action(Action::new(title)))
    }

    #[test]
    fn incoming_stays_in_its_lane_until_the_promotion_boundary() {
        let now = Instant::now();
        let layout = QueueLayout::new(size(px(900.), px(560.)), 1, CURRENT_HEIGHT);
        let outgoing = candidate("Completed");
        let incoming = candidate("Next");
        let origin = PromotionOrigin {
            lane: QueueLane::Today,
            pose: layout.source_card(0),
        };
        let mut handoff = Handoff::new(
            Some(PresentedItem {
                item: outgoing.clone(),
                pose: layout.current,
                opacity: 1.,
            }),
            Some(origin),
            vec![origin.lane],
            now,
        );
        assert_eq!(handoff.hidden_action(Some(&incoming)), outgoing.action_id());
        assert!(!handoff.advance(now + FADE - Duration::from_nanos(1)));
        assert!(handoff.advance(now + FADE));
        assert_eq!(handoff.hidden_action(Some(&incoming)), incoming.action_id());
        assert!(handoff.outgoing().is_none());
        let shown = handoff
            .presented(Some(&incoming), layout.current, now + FADE)
            .unwrap();
        assert_eq!(shown.pose, origin.pose);
        assert_eq!(shown.opacity, 1.);
        assert_eq!(handoff.retained_lanes, [QueueLane::Today]);
        assert!(!handoff.advance(now + FADE));
        assert!(!handoff.finished(now + FADE + PROMOTE - Duration::from_nanos(1), true));
        assert!(handoff.finished(now + FADE + PROMOTE, true));
    }

    #[test]
    fn rapid_retarget_preserves_the_displayed_pose_and_opacity_and_latest_item_wins() {
        let now = Instant::now();
        let layout = QueueLayout::new(size(px(900.), px(560.)), 2, CURRENT_HEIGHT);
        let first = candidate("First");
        let second = candidate("Second");
        let latest = candidate("Latest");
        let motion = Handoff::new(None, None, Vec::new(), now);
        let retarget_at = now + PROMOTE / 8;
        let shown = motion
            .presented(Some(&first), layout.current, retarget_at)
            .unwrap();
        assert_ne!(shown.pose, layout.current);
        assert!(shown.opacity > 0. && shown.opacity < 1.);

        let replacement = Handoff::new(Some(shown.clone()), None, Vec::new(), retarget_at);
        let retargeted = replacement
            .presented(Some(&second), layout.current, retarget_at)
            .unwrap();
        assert_eq!(retargeted.item.id(), first.id());
        assert_eq!(retargeted.pose, shown.pose);
        assert_eq!(retargeted.opacity, shown.opacity);
        let again_at = retarget_at + FADE / 2;
        let fading = replacement
            .presented(Some(&second), layout.current, again_at)
            .unwrap();
        assert_eq!(fading.pose, shown.pose);
        assert_eq!(fading.opacity, shown.opacity / 2.);

        let replacement = Handoff::new(Some(fading.clone()), None, Vec::new(), again_at);
        let retargeted = replacement
            .presented(Some(&latest), layout.current, again_at)
            .unwrap();
        assert_eq!(retargeted.pose, fading.pose);
        assert_eq!(retargeted.opacity, fading.opacity);
        let landed = replacement
            .presented(Some(&latest), layout.current, again_at + FADE + PROMOTE)
            .unwrap();
        assert_eq!(landed.item.id(), latest.id());
        assert_eq!(landed.pose, layout.current);
        assert_eq!(landed.opacity, 1.);
    }

    #[test]
    fn expanded_current_is_capped_and_bottom_anchored() {
        let viewport = size(px(900.), px(560.));
        let collapsed = QueueLayout::new(viewport, 3, CURRENT_HEIGHT);
        let expanded = QueueLayout::new(viewport, 3, px(1000.));
        assert_eq!(collapsed.current.size.height, CURRENT_HEIGHT);
        assert_eq!(expanded.current.size.height, viewport.height * 0.4);
        for layout in [collapsed, expanded] {
            assert_eq!(
                layout.current.center.y + layout.current.size.height / 2.,
                viewport.height - px(16.)
            );
            assert!(layout.column_height + px(6.) < layout.pipe_end().y);
        }
    }
}
