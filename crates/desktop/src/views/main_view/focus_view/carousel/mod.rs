mod controls;
mod geometry;

use std::collections::HashSet;

use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder as _, px, relative,
    size,
};
use gpui_kit::{
    foundation::StyledExt as _,
    layout::{FadeEdges, ScrollFade},
    overlay::Tooltip,
};
use gpui_kit_semantics::{NodeSpec, Role, Semantic as _};

use subroutine_core::AnyItem;
use uuid::Uuid;

use crate::{
    AppIcon,
    components::{
        EmptyState, ItemCard,
        ext::ElementExt as _,
        transition::{self, WindowTransitionExt as _},
    },
    icons::Icon,
    item_manager::ItemManager,
    selection::{SelectionOrder, SelectionScope},
    settings::{FocusCarouselOrientation, Settings},
};

use super::{
    FocusMode, FocusView,
    temporal::{self, EventMoment},
};
use controls::CarouselNavigation;
use geometry::{
    CAROUSEL_GAP_PX, CarouselGeometry, MIN_CAROUSEL_SCALE, carousel_base_size, carousel_geometry,
    entrance_distance, vertical_centers,
};

#[derive(Clone, Copy, Debug)]
struct CarouselPose {
    index: usize,
    distance: f32,
    exit_opacity: f32,
    geometry: CarouselGeometry,
}

impl FocusView {
    pub(super) fn render_carousel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let carousel_overscroll = self.carousel_overscroll.advance(cx);
        if self.carousel_overscroll.needs_frame(cx) {
            window.request_animation_frame();
        }

        let action_mode = self.mode == FocusMode::Action;
        let ids: Vec<_> = if action_mode {
            self.items.iter().map(AnyItem::id).collect()
        } else {
            self.event_moments
                .iter()
                .map(EventMoment::notice_id)
                .collect()
        };
        let Some(active_index) = self
            .carousel_active_id()
            .and_then(|active| ids.iter().position(|id| *id == active))
        else {
            return self.render_empty(cx);
        };
        let settings = Settings::global(cx);
        let orientation = settings.focus_carousel_orientation;

        let active_id = ids[active_index];
        let focus_handles = if action_mode {
            &self.item_focus_handles
        } else {
            &self.event_focus_handles
        };
        let active_handle = focus_handles
            .get(&active_id)
            .cloned()
            .unwrap_or_else(|| cx.focus_handle());
        let (completing_ids, editing_id): (HashSet<Uuid>, Option<Uuid>) = if action_mode {
            let manager = ItemManager::global(cx);
            let manager = manager.read(cx);
            (
                self.items
                    .iter()
                    .filter(|item| manager.is_completing(item.id()))
                    .map(AnyItem::id)
                    .collect(),
                self.items
                    .iter()
                    .find(|item| manager.is_being_edited(item.id()))
                    .map(AnyItem::id),
            )
        } else {
            (HashSet::new(), None)
        };
        let order = SelectionOrder::new(
            SelectionScope::Focus,
            ids.iter()
                .copied()
                .filter(|id| !completing_ids.contains(id)),
        );
        let previous = (0..active_index)
            .rev()
            .find(|index| !completing_ids.contains(&ids[*index]));
        let next =
            ((active_index + 1)..ids.len()).find(|index| !completing_ids.contains(&ids[*index]));
        let is_completing = completing_ids.contains(&active_id);
        let completion_target = action_mode
            .then(|| self.completion_target(active_index, cx))
            .flatten();
        let viewport_width = f32::from(self.carousel_size.width);
        let viewport_height = f32::from(self.carousel_size.height);
        let (base_width, base_height) =
            carousel_base_size(viewport_width, viewport_height, orientation);
        let narrow_card = base_width < 360.0;
        let footer_height = if action_mode {
            0.0
        } else {
            let height: f32 = if narrow_card { 80.0 } else { 56.0 };
            height.min((base_height - 1.0).max(0.0))
        };
        let primary_extent = match orientation {
            FocusCarouselOrientation::Horizontal => viewport_width,
            FocusCarouselOrientation::Vertical => viewport_height,
        };

        let displayed: Vec<usize> = (0..ids.len())
            .filter(|index| !completing_ids.contains(&ids[*index]))
            .collect();
        let active_position = displayed
            .iter()
            .position(|index| *index == active_index)
            .unwrap_or_default() as isize;
        let progress = if displayed.is_empty() {
            1.0
        } else {
            (active_position as f32 + 1.0) / displayed.len() as f32
        };
        let visual_center = active_position as f32;

        let primary_base = match orientation {
            FocusCarouselOrientation::Horizontal => base_width,
            FocusCarouselOrientation::Vertical => base_height,
        };
        let minimum_spacing = primary_base * MIN_CAROUSEL_SCALE + CAROUSEL_GAP_PX;
        let overscan =
            (((primary_extent / 2.0 + primary_base / 2.0) / minimum_spacing).ceil() as isize + 2)
                .max(3);
        let mut candidates = HashSet::new();
        if !displayed.is_empty() {
            let first = (visual_center.floor() as isize - overscan).max(0) as usize;
            let last = (visual_center.ceil() as isize + overscan)
                .min(displayed.len() as isize - 1)
                .max(0) as usize;
            if first <= last {
                candidates.extend(displayed[first..=last].iter().copied());
            }
        }
        candidates.insert(active_index);
        for (index, id) in ids.iter().enumerate() {
            if completing_ids.contains(id)
                && (index as isize - active_index as isize).abs() <= overscan
            {
                candidates.insert(index);
            }
        }

        let mut poses = Vec::new();
        for index in candidates {
            let id = ids[index];
            let completing = completing_ids.contains(&id);
            let target_distance = if completing {
                match index.cmp(&active_index) {
                    std::cmp::Ordering::Less => -2.0,
                    std::cmp::Ordering::Greater => 2.0,
                    std::cmp::Ordering::Equal => 0.0,
                }
            } else {
                let position = displayed
                    .iter()
                    .position(|candidate| *candidate == index)
                    .unwrap_or_default() as isize;
                (position - active_position) as f32
            };
            let entering = action_mode && self.entering_ids.contains(&id);
            let distance = window.keyed_transition(
                ("focus-carousel-distance", id.as_u64_pair().1),
                cx,
                transition::SCROLL,
                || {
                    if entering {
                        entrance_distance(target_distance)
                    } else {
                        target_distance
                    }
                },
            );
            distance.set(target_distance, cx);
            let distance = distance.animate(window, cx);

            let opacity = window.keyed_transition(
                ("focus-carousel-opacity", id.as_u64_pair().1),
                cx,
                transition::SCROLL,
                || if entering { 0.0 } else { 1.0 },
            );
            opacity.set(if completing { 0.0 } else { 1.0 }, cx);
            let opacity = opacity.animate(window, cx);

            let mut geometry = carousel_geometry(
                distance,
                base_width,
                base_height,
                orientation,
                primary_extent,
            );
            let footer = footer_height * geometry.scale;
            let collapsed = px(geometry.height - footer);
            let limit = px((viewport_height - 8.0).max(base_height) * geometry.scale - footer);
            let height = self.card_states.height(id, collapsed).min(limit);
            geometry.height = f32::from(height) + footer;
            poses.push(CarouselPose {
                index,
                distance,
                exit_opacity: opacity,
                geometry,
            });
        }

        if orientation == FocusCarouselOrientation::Vertical {
            poses.sort_by(|a, b| {
                a.distance
                    .total_cmp(&b.distance)
                    .then(a.index.cmp(&b.index))
            });
            let extents: Vec<_> = poses
                .iter()
                .filter(|pose| !completing_ids.contains(&ids[pose.index]))
                .map(|pose| {
                    (
                        (pose.geometry.center - 0.5) * primary_extent,
                        pose.geometry.height,
                    )
                })
                .collect();
            for (pose, center) in poses
                .iter_mut()
                .filter(|pose| !completing_ids.contains(&ids[pose.index]))
                .zip(vertical_centers(&extents))
            {
                pose.geometry.center = 0.5 + center / primary_extent.max(1.0);
            }
        }
        poses.retain(|pose| {
            let geometry = pose.geometry;
            let extent = match orientation {
                FocusCarouselOrientation::Horizontal => geometry.width,
                FocusCarouselOrientation::Vertical => geometry.height,
            };
            let center = geometry.center * primary_extent;
            (center + extent / 2.0 >= 0.0 && center - extent / 2.0 <= primary_extent)
                || pose.index == active_index
                || completing_ids.contains(&ids[pose.index])
        });

        poses.sort_by(|a, b| b.distance.abs().total_cmp(&a.distance.abs()));

        let mut objects = Vec::new();
        for pose in poses {
            let index = pose.index;
            let exit_opacity = pose.exit_opacity;
            let geometry = pose.geometry;
            let footer = footer_height * geometry.scale;
            let card_size = size(px(geometry.width), px(geometry.height - footer));
            let id = ids[index];
            let is_active = index == active_index;
            let completing = completing_ids.contains(&id);
            let editing = editing_id == Some(id);
            let handle = focus_handles
                .get(&id)
                .cloned()
                .unwrap_or_else(|| cx.focus_handle());
            let next_focus = if is_active {
                completion_target.clone()
            } else {
                Some((active_id, active_handle.clone()))
            };
            let card = if action_mode {
                let item = &self.items[index];
                ItemCard::new_with_id(
                    ("focus-carousel-card", id.as_u64_pair().1),
                    item,
                    super::super::format_item_meta(item),
                    window,
                    cx,
                )
            } else {
                temporal::event_carousel_card(&self.event_moments[index], self.now, window, cx)
            }
            .details(self.card_states.get(id))
            .with_focus_handle(handle)
            .editable(false)
            .when_else(
                narrow_card,
                |card| card.large_title(true),
                |card| card.display_title(),
            )
            .tab_stop(is_active && !completing)
            .next_focus(next_focus)
            .size_full()
            .when(self.card_states.is_open(id), |card| {
                card.on_scroll_wheel(cx.listener(|view, _, _, cx| {
                    view.reset_wheel();
                    cx.stop_propagation();
                }))
            });
            let card = if !action_mode {
                card.actionable(is_active)
                    .draggable(is_active, Some(card_size))
            } else if completing {
                card.draggable(false, None)
            } else if is_active {
                card.selectable(order.clone())
                    .draggable(!editing, Some(card_size))
            } else {
                card.draggable(true, Some(card_size))
            };

            let card = if action_mode {
                card.into_any_element()
            } else {
                div()
                    .column()
                    .size_full()
                    .child(div().w_full().flex_1().min_h_0().child(card))
                    .child(
                        div()
                            .w_full()
                            .h(px(footer))
                            .flex_none()
                            .overflow_hidden()
                            .opacity((1.0 - pose.distance.abs()).clamp(0.0, 1.0))
                            .pt_1()
                            .when(is_active, |footer| {
                                footer.child(temporal::event_carousel_status(
                                    &self.event_moments[index],
                                    narrow_card,
                                    cx,
                                ))
                            }),
                    )
                    .into_any_element()
            };

            let object = div()
                .id(("focus-carousel-object", id.as_u64_pair().1))
                .absolute()
                .row()
                .items_center()
                .justify_center()
                .w(px(geometry.width))
                .h(px(geometry.height))
                .opacity(geometry.opacity * exit_opacity)
                .child(card);
            let object = match orientation {
                FocusCarouselOrientation::Horizontal => object
                    .left(relative(geometry.center))
                    .top(relative(0.5))
                    .ml(px(-geometry.width / 2.))
                    .mt(px(-geometry.height / 2.)),
                FocusCarouselOrientation::Vertical => object
                    .left(relative(0.5))
                    .top(relative(geometry.center))
                    .ml(px(-geometry.width / 2.))
                    .mt(px(-geometry.height / 2.)),
            };
            let object = if is_active || completing {
                object
            } else {
                object
                    .cursor_pointer()
                    .hover(|style| style.opacity(1.))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        cx.stop_propagation();
                        view.activate(id, window, cx);
                    }))
            };
            objects.push(object.into_any_element());
        }

        let fade_edges = match orientation {
            FocusCarouselOrientation::Horizontal => FadeEdges {
                left: previous.is_some(),
                right: next.is_some(),
                ..FadeEdges::default()
            },
            FocusCarouselOrientation::Vertical => FadeEdges {
                top: previous.is_some(),
                bottom: next.is_some(),
                ..FadeEdges::default()
            },
        };
        let view = cx.entity();
        let carousel = div()
            .id("focus-carousel")
            .relative()
            .size_full()
            .overflow_hidden()
            .on_scroll_wheel(
                cx.listener(|view, event, window, cx| view.handle_wheel(event, window, cx)),
            )
            .on_prepaint(move |bounds, _, cx| {
                let measured = bounds.size;
                view.update(cx, |view, cx| {
                    if view.carousel_size != measured {
                        view.carousel_size = measured;
                        cx.notify();
                    }
                });
            })
            .child(
                div()
                    .relative()
                    .size_full()
                    .when(
                        orientation == FocusCarouselOrientation::Horizontal,
                        |this| this.left(carousel_overscroll),
                    )
                    .when(orientation == FocusCarouselOrientation::Vertical, |this| {
                        this.top(carousel_overscroll)
                    })
                    .children(objects),
            );
        let controls = self.render_controls(
            orientation,
            CarouselNavigation {
                has_previous: previous.is_some(),
                has_next: next.is_some(),
            },
            is_completing,
            progress,
            format!("{} / {}", active_position + 1, displayed.len()),
            cx,
        );
        let controls = match orientation {
            FocusCarouselOrientation::Horizontal => div()
                .flex_none()
                .py_4()
                .flex()
                .justify_center()
                .child(controls)
                .into_any_element(),
            FocusCarouselOrientation::Vertical => div()
                .flex_none()
                .px_4()
                .flex()
                .items_center()
                .child(controls)
                .into_any_element(),
        };

        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .when_else(
                orientation == FocusCarouselOrientation::Horizontal,
                |this| this.column(),
                |this| this.row(),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .h_full()
                    .when(
                        orientation == FocusCarouselOrientation::Horizontal,
                        |this| this.w_full().h_auto(),
                    )
                    .child(
                        ScrollFade::new("focus-carousel-edge-fade")
                            .edges(fade_edges)
                            .child(carousel),
                    ),
            )
            .child(controls)
            .into_any_element()
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> AnyElement {
        let (id, icon, title, caption) = if !self.loaded {
            ("focus-loading", AppIcon::Clock, "Loading…", None)
        } else {
            let settings = Settings::global(cx);
            let horizon = temporal::format_horizon(match self.mode {
                FocusMode::Action => settings.focus_action_horizon_hours(),
                FocusMode::Event => settings.focus_horizon_hours(),
            });
            match self.mode {
                FocusMode::Action => (
                    "focus-action-empty",
                    AppIcon::Check,
                    "No upcoming actions",
                    Some(format!("Nothing due in the next {horizon}.")),
                ),
                FocusMode::Event => (
                    "focus-event-empty",
                    AppIcon::CalendarClock,
                    "No upcoming events",
                    Some(format!("No events in the next {horizon}.")),
                ),
            }
        };
        let status = caption.unwrap_or_else(|| title.to_owned());
        let tooltip = status.clone();

        div()
            .id(id)
            .size_full()
            .child(EmptyState::new(Icon::new(icon), title))
            .tooltip(move |_, cx| Tooltip::new(id, tooltip.clone()).view(cx))
            .semantic_in(cx, NodeSpec::new(id, Role::Status).text(status))
            .into_any_element()
    }
}
