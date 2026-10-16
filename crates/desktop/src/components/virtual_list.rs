use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, Context, Div, ElementId, Entity, FocusHandle, InteractiveElement,
    IntoElement, ParentElement, Pixels, Point, Render, RenderOnce, ScrollHandle, ScrollStrategy,
    Size, StatefulInteractiveElement, Styled, Window, container_query, div,
    prelude::FluentBuilder as _, px,
};

use crate::components::{
    elastic_overscroll::ElasticOverscroll,
    ext::ElementExt as _,
    scroll_motion::{ScrollMotion, scroll_ranges},
};

const OVERSCAN: usize = 4;

#[derive(Default)]
enum FocusReveal {
    #[default]
    None,
    Pending(FocusHandle),
    Settled(FocusHandle),
}

#[derive(Clone)]
pub(crate) struct ScrollFocus(Entity<FocusReveal>);

impl ScrollFocus {
    pub(crate) fn new(id: ElementId, window: &mut Window, cx: &mut App) -> Self {
        let state = window.use_keyed_state((id, "scroll-focus"), cx, |_, _| FocusReveal::None);
        let focused = window.focused(cx);
        state.update(cx, |state, _| {
            let previous = match state {
                FocusReveal::None => None,
                FocusReveal::Pending(handle) | FocusReveal::Settled(handle) => Some(&*handle),
            };
            if previous != focused.as_ref() {
                *state = focused.map_or(FocusReveal::None, FocusReveal::Pending);
            }
        });
        Self(state)
    }

    pub(crate) fn reveal(
        &self,
        bounds: Bounds<Pixels>,
        row_focus: Option<&FocusHandle>,
        scroll: &ScrollHandle,
        motion: &ScrollMotion,
        inset: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (Some(row_focus), FocusReveal::Pending(target)) = (row_focus, self.0.read(cx)) else {
            return;
        };
        let row_focus = row_focus.clone();
        let target = target.clone();
        let state = self.0.clone();
        let motion = motion.clone();
        let scroll = scroll.clone();
        let offset = scroll.offset();
        window.on_next_frame(move |window, cx| {
            if window.focused(cx).as_ref() != Some(&target)
                || !row_focus.contains_focused(window, cx)
                || !matches!(state.read(cx), FocusReveal::Pending(handle) if handle == &target)
            {
                return;
            }
            let available = scroll.bounds().size.height - inset;
            if available <= px(0.) {
                return;
            }
            state.update(cx, |state, _| *state = FocusReveal::Settled(target));
            let mut bounds = bounds;
            bounds.origin += scroll.offset() - offset;
            bounds.size.height = bounds.size.height.min(available);
            if motion.reveal(&scroll, bounds, scroll.bounds(), inset) {
                window.refresh();
            }
        });
    }
}

#[derive(Clone, Default)]
pub struct VirtualListScrollHandle {
    base: ScrollHandle,
    metrics: Rc<RefCell<Metrics>>,
    elastic: Rc<RefCell<ElasticOverscroll>>,
    motion: ScrollMotion,
}

#[derive(Default)]
struct Metrics {
    offsets: Vec<Pixels>,
    bounds: Bounds<Pixels>,
    pending_scroll: Option<ScrollRequest>,
}

#[derive(Clone, Copy)]
enum ScrollRequest {
    Reveal(usize, ScrollStrategy),
    Anchor(usize, Pixels),
}

impl Metrics {
    fn offset_of(&self, ix: usize) -> Option<Pixels> {
        self.offsets.get(ix).copied()
    }

    fn height_of(&self, ix: usize) -> Option<Pixels> {
        Some(*self.offsets.get(ix + 1)? - *self.offsets.get(ix)?)
    }

    fn total(&self) -> Pixels {
        self.offsets.last().copied().unwrap_or_default()
    }

    fn row_at(&self, offset: Pixels) -> Option<usize> {
        if offset < px(0.) || offset >= self.total() {
            return None;
        }
        Some(
            self.offsets
                .partition_point(|row_offset| *row_offset <= offset)
                .saturating_sub(1),
        )
    }
}

fn clamped_scroll_offset(
    current: Pixels,
    delta: Pixels,
    content_height: Pixels,
    viewport_height: Pixels,
) -> Pixels {
    let min = -(content_height - viewport_height).max(px(0.));
    (current + delta).clamp(min, px(0.))
}

impl VirtualListScrollHandle {
    pub fn new() -> Self {
        Self::default()
    }

    fn apply_pending_scroll(&self) {
        let mut metrics = self.metrics.borrow_mut();
        let viewport = metrics.bounds.size.height;
        if metrics.bounds.size.width <= px(0.) || viewport <= px(0.) {
            return;
        }
        let Some(request) = metrics.pending_scroll else {
            return;
        };
        let ix = match request {
            ScrollRequest::Reveal(ix, _) | ScrollRequest::Anchor(ix, _) => ix,
        };
        let (Some(top), Some(height)) = (metrics.offset_of(ix), metrics.height_of(ix)) else {
            return;
        };
        metrics.pending_scroll = None;
        if let ScrollRequest::Anchor(_, within) = request {
            let target = -(top + within).clamp(px(0.), (metrics.total() - viewport).max(px(0.)));
            self.motion.rebase(&self.base, target);
            return;
        }
        let current = -self.motion.target(&self.base);

        let target = match request {
            ScrollRequest::Reveal(_, ScrollStrategy::Top) => top,
            ScrollRequest::Reveal(_, ScrollStrategy::Center) => top - (viewport - height) / 2.,
            _ => {
                if height > viewport || top < current {
                    top
                } else if top + height > current + viewport {
                    top + height - viewport
                } else {
                    current
                }
            }
        };

        let max = (metrics.total() - viewport).max(px(0.));
        let clamped = target.clamp(px(0.), max);
        self.motion.set(&self.base, -clamped);
    }

    pub fn bounds(&self) -> Bounds<Pixels> {
        self.metrics.borrow().bounds
    }

    pub(crate) fn set_bounds(&self, bounds: Bounds<Pixels>) {
        self.metrics.borrow_mut().bounds = bounds;
    }

    pub fn scroll_by_y(&self, delta: Pixels) -> Pixels {
        self.motion.cancel();
        let metrics = self.metrics.borrow();
        if metrics.bounds.size.width <= px(0.) || metrics.bounds.size.height <= px(0.) {
            return px(0.);
        }
        let offset = self.base.offset();
        let next =
            clamped_scroll_offset(offset.y, delta, metrics.total(), metrics.bounds.size.height);
        self.base.set_offset(Point::new(offset.x, next));
        next - offset.y
    }

    pub fn reveal_bounds(&self, bounds: Bounds<Pixels>) -> bool {
        self.motion
            .reveal(&self.base, bounds, self.bounds(), px(0.))
    }

    pub fn cancel_scroll(&self) {
        self.motion.cancel();
        self.metrics.borrow_mut().pending_scroll = None;
    }

    pub fn row_at_position(&self, position: Point<Pixels>) -> Option<(usize, Bounds<Pixels>)> {
        let metrics = self.metrics.borrow();
        let viewport = metrics.bounds;
        let local = viewport.localize(&position)?;
        let content_y = local.y - self.base.offset().y - self.elastic.borrow().offset();
        let ix = metrics.row_at(content_y)?;
        let top = metrics.offset_of(ix)?;
        let height = metrics.height_of(ix)?;
        Some((
            ix,
            Bounds {
                origin: Point::new(
                    viewport.origin.x,
                    viewport.origin.y + self.base.offset().y + self.elastic.borrow().offset() + top,
                ),
                size: Size::new(viewport.size.width, height),
            },
        ))
    }
}

pub fn v_virtual_list<R, V>(
    view: Entity<V>,
    id: impl Into<ElementId>,
    item_sizes: Rc<Vec<Size<Pixels>>>,
    f: impl 'static + Fn(&mut V, Range<usize>, &mut Window, &mut Context<V>) -> Vec<R>,
) -> VirtualList
where
    R: IntoElement,
    V: Render,
{
    let notify_view = view.downgrade();
    let id = id.into();
    VirtualList {
        base: div().id(id.clone()),
        id,
        item_sizes,
        scroll_handle: None,
        scroll_to: None,
        render: Box::new(move |range, window, cx| {
            view.update(cx, |view, cx| {
                f(view, range, window, cx)
                    .into_iter()
                    .map(IntoElement::into_any_element)
                    .collect()
            })
        }),
        notify: Box::new(move |cx| {
            notify_view.update(cx, |_, cx| cx.notify()).ok();
        }),
    }
}

type RenderRange = Box<dyn Fn(Range<usize>, &mut Window, &mut App) -> Vec<AnyElement>>;

#[derive(IntoElement)]
pub struct VirtualList {
    base: gpui::Stateful<Div>,
    id: ElementId,
    item_sizes: Rc<Vec<Size<Pixels>>>,
    scroll_handle: Option<VirtualListScrollHandle>,
    scroll_to: Option<ScrollRequest>,
    render: RenderRange,
    notify: Box<dyn Fn(&mut App)>,
}

impl VirtualList {
    pub fn scroll_to_item(mut self, index: usize, strategy: ScrollStrategy) -> Self {
        self.scroll_to = Some(ScrollRequest::Reveal(index, strategy));
        self
    }

    pub fn restore_anchor(mut self, index: usize, within: Pixels) -> Self {
        self.scroll_to = Some(ScrollRequest::Anchor(index, within));
        self
    }

    pub fn track_scroll(mut self, handle: &VirtualListScrollHandle) -> Self {
        self.scroll_handle = Some(handle.clone());
        self
    }
}

impl Styled for VirtualList {
    fn style(&mut self) -> &mut gpui::StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for VirtualList {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let handle = self.scroll_handle.unwrap_or_default();
        if let Some(target) = self.scroll_to {
            handle.metrics.borrow_mut().pending_scroll = Some(target);
        }
        let measured = handle.clone();

        self.base
            .relative()
            .overflow_hidden()
            .on_prepaint(move |bounds, _, _| measured.set_bounds(bounds))
            .child(container_query(move |size, window, cx| {
                if size.width <= px(0.) || size.height <= px(0.) {
                    return div().size_full().into_any_element();
                }

                let mut offsets = Vec::with_capacity(self.item_sizes.len() + 1);
                let mut total = px(0.);
                offsets.push(total);
                for item in self.item_sizes.iter() {
                    total += item.height;
                    offsets.push(total);
                }
                {
                    let mut metrics = handle.metrics.borrow_mut();
                    metrics.offsets = offsets.clone();
                    metrics.bounds.size = size;
                }
                let viewport = size.height;

                handle.apply_pending_scroll();
                handle
                    .motion
                    .animate(&handle.base, total - viewport, window, cx);
                let range_at = |offset: Pixels| {
                    let first = offsets
                        .partition_point(|top| *top <= -offset)
                        .saturating_sub(1);
                    let last = offsets.partition_point(|top| *top < -offset + viewport);
                    first.saturating_sub(OVERSCAN).min(self.item_sizes.len())
                        ..(last + OVERSCAN).min(self.item_sizes.len())
                };
                // Keep destination controls mounted so focus and typing survive the journey.
                let ranges = scroll_ranges(
                    range_at(handle.base.offset().y),
                    range_at(handle.motion.target(&handle.base)),
                );
                let rows: Vec<_> = ranges
                    .into_iter()
                    .flat_map(|range| {
                        let rows = (self.render)(range.clone(), window, cx);
                        range.zip(rows)
                    })
                    .collect();
                let scrolling = handle.motion.is_animating(&handle.base);
                let focus = ScrollFocus::new(self.id, window, cx);
                let overscroll_y = handle.elastic.borrow_mut().advance(cx);
                let offset = handle.base.offset().y;
                let placed: Vec<_> = rows
                    .into_iter()
                    .map(|(ix, row)| {
                        let top = offsets[ix];
                        let height = self.item_sizes[ix].height;
                        let visible_top = top + overscroll_y + offset;
                        let visible = visible_top + height > px(0.) && visible_top < viewport;
                        let focus = focus.clone();
                        let scroll = handle.base.clone();
                        let motion = handle.motion.clone();
                        div()
                            .id(("virtual-list-row", ix))
                            .focusable()
                            .tab_stop(false)
                            .absolute()
                            .top(top)
                            .left_0()
                            .right_0()
                            .when(!visible && !scrolling, |row| row.invisible())
                            .on_focus_resolved(move |bounds, row, window, cx| {
                                focus.reveal(bounds, row, &scroll, &motion, px(0.), window, cx);
                            })
                            .child(row)
                    })
                    .collect();
                if handle.elastic.borrow().needs_frame(cx) {
                    window.request_animation_frame();
                }
                let elastic = handle.elastic.clone();
                let motion = handle.motion.clone();
                let notify = self.notify;

                div()
                    .id("virtual-list-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&handle.base)
                    .on_scroll_wheel(move |event, window, cx| {
                        motion.cancel();
                        if elastic.borrow_mut().handle_scroll(event, window, cx) {
                            notify(cx);
                        }
                    })
                    .child(
                        div().relative().w_full().h(total).child(
                            div()
                                .absolute()
                                .top(overscroll_y)
                                .left_0()
                                .right_0()
                                .h(total)
                                .children(placed),
                        ),
                    )
                    .into_any_element()
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cold_reveal_waits_for_a_usable_viewport() {
        let handle = VirtualListScrollHandle::new();
        {
            let mut metrics = handle.metrics.borrow_mut();
            metrics.offsets = (0..=100).map(|ix| px(ix as f32 * 40.)).collect();
            metrics.pending_scroll = Some(ScrollRequest::Reveal(75, ScrollStrategy::Center));
        }
        handle.apply_pending_scroll();
        assert_eq!(handle.base.offset().y, px(0.));
        assert!(handle.metrics.borrow().pending_scroll.is_some());

        handle.set_bounds(Bounds::new(
            Point::default(),
            gpui::size(px(640.), px(320.)),
        ));
        handle.apply_pending_scroll();
        assert_eq!(handle.base.offset().y, px(0.));
        assert_eq!(handle.motion.target(&handle.base), px(-2860.));
        assert!(handle.metrics.borrow().pending_scroll.is_none());
    }

    #[test]
    fn anchor_restoration_is_immediate_and_rebases_navigation() {
        let handle = VirtualListScrollHandle::new();
        handle.set_bounds(Bounds::new(
            Point::default(),
            gpui::size(px(640.), px(120.)),
        ));
        handle.metrics.borrow_mut().offsets = vec![px(0.), px(100.), px(200.), px(500.)];
        handle.base.set_offset(Point::new(px(0.), px(-25.)));
        handle.motion.set(&handle.base, px(-200.));
        handle.metrics.borrow_mut().pending_scroll = Some(ScrollRequest::Anchor(1, px(25.)));

        handle.apply_pending_scroll();
        assert_eq!(handle.base.offset().y, px(-125.));
        assert_eq!(handle.motion.target(&handle.base), px(-300.));
        assert!(handle.motion.is_animating(&handle.base));

        handle.cancel_scroll();
        handle.metrics.borrow_mut().pending_scroll = Some(ScrollRequest::Anchor(0, px(25.)));
        handle.apply_pending_scroll();
        assert_eq!(handle.base.offset().y, px(-25.));
        assert!(!handle.motion.is_animating(&handle.base));
    }

    #[test]
    fn queued_reveal_uses_resized_viewport_and_reflowed_rows() {
        let handle = VirtualListScrollHandle::new();
        handle.set_bounds(Bounds::new(
            Point::default(),
            gpui::size(px(640.), px(320.)),
        ));
        {
            let mut metrics = handle.metrics.borrow_mut();
            metrics.offsets = vec![px(0.), px(40.), px(80.)];
            metrics.pending_scroll = Some(ScrollRequest::Reveal(1, ScrollStrategy::Center));
        }

        handle.set_bounds(Bounds::new(
            Point::default(),
            gpui::size(px(320.), px(120.)),
        ));
        handle.metrics.borrow_mut().offsets = vec![px(0.), px(200.), px(240.), px(400.)];
        handle.apply_pending_scroll();
        assert_eq!(handle.base.offset().y, px(0.));
        assert_eq!(handle.motion.target(&handle.base), px(-160.));
        assert!(handle.metrics.borrow().pending_scroll.is_none());
    }
}
