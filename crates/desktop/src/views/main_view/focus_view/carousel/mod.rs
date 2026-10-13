mod controls;
mod geometry;
mod slot;

use std::collections::HashSet;

use gpui::{
    AnyElement, Context, Div, ElementId, InteractiveElement, IntoElement, ParentElement, Pixels,
    Size, Stateful, StatefulInteractiveElement, Styled, Window, container_query, div,
    prelude::FluentBuilder as _, px, relative, size,
};
use gpui_kit::{
    foundation::StyledExt as _,
    layout::{FadeEdges, ScrollFade},
    motion::Transition,
};

use subroutine_core::AnyItem;
use uuid::Uuid;

use crate::{
    components::{ItemCard, transition},
    item_manager::{DraftView, ItemManager},
    selection::SelectionOrder,
    settings::{FocusCarouselOrientation, Settings},
    stores::AppDatabaseStore,
};

use super::{
    FocusMode, FocusView,
    temporal::{self, EventMoment},
};
use controls::CarouselNavigation;
use geometry::{
    CAROUSEL_GAP_PX, CarouselGeometry, MIN_CAROUSEL_SCALE, carousel_base_size,
    carousel_footer_height, carousel_geometry, entrance_distance, vertical_centers,
};

use slot::CarouselSlot;

struct CarouselContent {
    objects: AnyElement,
    navigation: CarouselNavigation,
    position: Option<CarouselPosition>,
}

pub(super) struct CarouselMotion {
    distance: Transition<f32>,
    opacity: Transition<f32>,
}

struct CarouselItems {
    ids: Vec<Uuid>,
    active_index: usize,
    completing_ids: HashSet<Uuid>,
    editing_id: Option<Uuid>,
    displayed: Vec<usize>,
}

#[derive(Default)]
struct CarouselPosition {
    is_completing: bool,
    progress: f32,
    label: String,
}

fn carousel_object(
    id: impl Into<ElementId>,
    geometry: CarouselGeometry,
    orientation: FocusCarouselOrientation,
    opacity: f32,
    card: AnyElement,
) -> Stateful<Div> {
    let object = div()
        .id(id)
        .absolute()
        .row()
        .items_center()
        .justify_center()
        .w(px(geometry.width))
        .h(px(geometry.height))
        .ml(px(-geometry.width / 2.))
        .mt(px(-geometry.height / 2.))
        .opacity(geometry.opacity * opacity)
        .child(card);
    match orientation {
        FocusCarouselOrientation::Horizontal => {
            object.left(relative(geometry.center)).top(relative(0.5))
        }
        FocusCarouselOrientation::Vertical => {
            object.left(relative(0.5)).top(relative(geometry.center))
        }
    }
}

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
        order: SelectionOrder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let action_mode = self.mode == FocusMode::Action;
        let ids: Vec<_> = if action_mode {
            self.items.iter().map(AnyItem::id).collect()
        } else {
            self.event_moments
                .iter()
                .map(EventMoment::notice_id)
                .collect()
        };
        self.carousel_motion.retain(|id, _| ids.contains(id));
        let active_index = self
            .carousel_active_id()
            .and_then(|active| ids.iter().position(|id| *id == active))
            .or_else(|| (action_mode && !ids.is_empty()).then_some(0));
        let orientation = Settings::global(cx).focus_carousel_orientation;

        let draft = ItemManager::global(cx)
            .read(cx)
            .view_draft(DraftView::Focus)
            .cloned();
        let slot = CarouselSlot::resolve(
            draft,
            AppDatabaseStore::global(cx).read(cx).workspace_content(),
            self.mode,
            self.queue_blocker.is_some(),
            active_index.is_some(),
        );
        if let Some(slot) = slot {
            let view = cx.entity();
            let objects = container_query(move |size, window, cx| {
                view.update(cx, |view, cx| {
                    let (width, height) = carousel_base_size(
                        f32::from(size.width),
                        f32::from(size.height),
                        orientation,
                    );
                    let extent = match orientation {
                        FocusCarouselOrientation::Horizontal => size.width,
                        FocusCarouselOrientation::Vertical => size.height,
                    };
                    let geometry =
                        carousel_geometry(0.0, width, height, orientation, extent.into());
                    let footer = carousel_footer_height(width, height, action_mode);
                    div()
                        .relative()
                        .size_full()
                        .child(view.render_carousel_slot(
                            slot,
                            geometry,
                            footer,
                            orientation,
                            window,
                            cx,
                        ))
                })
            })
            .into_any_element();
            return self.render_carousel_frame(
                CarouselContent {
                    objects,
                    navigation: CarouselNavigation {
                        has_previous: false,
                        has_next: false,
                    },
                    position: None,
                },
                orientation,
                window,
                cx,
            );
        }
        let Some(active_index) = active_index else {
            unreachable!("a carousel without an active card renders a slot");
        };

        let active_id = ids[active_index];
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

        let previous = (0..active_index)
            .rev()
            .find(|index| !completing_ids.contains(&ids[*index]));
        let next =
            ((active_index + 1)..ids.len()).find(|index| !completing_ids.contains(&ids[*index]));
        let is_completing = completing_ids.contains(&active_id);

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
        let position = CarouselPosition {
            is_completing,
            progress,
            label: format!("{} / {}", active_position + 1, displayed.len()),
        };
        let items = CarouselItems {
            ids,
            active_index,
            completing_ids,
            editing_id,
            displayed,
        };
        let view = cx.entity();
        let objects = container_query(move |size, window, cx| {
            view.update(cx, |view, cx| {
                view.render_carousel_items(items, size, orientation, order, window, cx)
            })
        })
        .into_any_element();
        self.render_carousel_frame(
            CarouselContent {
                objects,
                navigation: CarouselNavigation {
                    has_previous: previous.is_some(),
                    has_next: next.is_some(),
                },
                position: Some(position),
            },
            orientation,
            window,
            cx,
        )
    }

    fn render_carousel_items(
        &mut self,
        items: CarouselItems,
        viewport: Size<Pixels>,
        orientation: FocusCarouselOrientation,
        order: SelectionOrder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let CarouselItems {
            ids,
            active_index,
            completing_ids,
            editing_id,
            displayed,
        } = items;
        let active_id = ids[active_index];
        let action_mode = self.mode == FocusMode::Action;
        let focus_handles = if action_mode {
            &self.item_focus_handles
        } else {
            &self.event_focus_handles
        };
        let viewport_width = f32::from(viewport.width);
        let viewport_height = f32::from(viewport.height);
        let (base_width, base_height) =
            carousel_base_size(viewport_width, viewport_height, orientation);
        let narrow_card = base_width < 360.0;
        let footer_height = carousel_footer_height(base_width, base_height, action_mode);
        let primary_extent = match orientation {
            FocusCarouselOrientation::Horizontal => viewport_width,
            FocusCarouselOrientation::Vertical => viewport_height,
        };
        let active_position = displayed
            .iter()
            .position(|index| *index == active_index)
            .unwrap_or_default() as isize;
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
            let inline = self.inline_card_id == Some(id);
            let entering = action_mode && self.entering_ids.contains(&id) && !inline;
            let motion = self
                .carousel_motion
                .entry(id)
                .or_insert_with(|| CarouselMotion {
                    distance: Transition::new(
                        if entering {
                            entrance_distance(target_distance)
                        } else {
                            target_distance
                        },
                        transition::SCROLL,
                    ),
                    opacity: Transition::new(if entering { 0.0 } else { 1.0 }, transition::SCROLL),
                });
            if inline {
                motion.distance.snap(target_distance);
                motion.opacity.snap(1.0);
            }
            motion.distance.set(target_distance);
            motion.opacity.set(if completing { 0.0 } else { 1.0 });
            let distance = motion.distance.animate(window, cx);
            let opacity = motion.opacity.animate(window, cx);

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
                card.actionable(false).draggable(false, None)
            } else if is_active {
                card.selectable(order.clone())
                    .draggable(!editing, Some(card_size))
            } else {
                card.selectable(order.clone())
                    .draggable(true, Some(card_size))
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

            let object = carousel_object(
                ("focus-carousel-object", id.as_u64_pair().1),
                geometry,
                orientation,
                exit_opacity,
                card,
            );
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

        if self.inline_card_id == Some(active_id) {
            self.inline_card_id = None;
        }
        div()
            .relative()
            .size_full()
            .children(objects)
            .into_any_element()
    }

    fn render_carousel_frame(
        &mut self,
        content: CarouselContent,
        orientation: FocusCarouselOrientation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let CarouselContent {
            objects,
            navigation,
            position,
        } = content;
        let carousel_overscroll = if position.is_some() {
            let offset = self.carousel_overscroll.advance(cx);
            if self.carousel_overscroll.needs_frame(cx) {
                window.request_animation_frame();
            }
            offset
        } else {
            px(0.)
        };
        let fade_edges = match orientation {
            FocusCarouselOrientation::Horizontal => FadeEdges {
                left: navigation.has_previous,
                right: navigation.has_next,
                ..FadeEdges::default()
            },
            FocusCarouselOrientation::Vertical => FadeEdges {
                top: navigation.has_previous,
                bottom: navigation.has_next,
                ..FadeEdges::default()
            },
        };
        let carousel = div()
            .id("focus-carousel")
            .relative()
            .size_full()
            .overflow_hidden()
            .on_scroll_wheel(
                cx.listener(|view, event, window, cx| view.handle_wheel(event, window, cx)),
            )
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
                    .child(objects),
            );
        let controls = self.render_controls(orientation, navigation, position, cx);
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
}
