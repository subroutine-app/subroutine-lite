use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, Context, Div, Edges, ElementId, Entity, FocusHandle,
    InteractiveElement, IntoElement, ParentElement, Pixels, Point, Render, RenderOnce,
    ScrollHandle, ScrollStrategy, Size, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::components::elastic_overscroll::ElasticOverscroll;

const OVERSCAN: usize = 4;

const FIRST_FRAME_ROWS: usize = 24;

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
            if scroll.reveal_bounds(
                bounds,
                Edges {
                    top: inset,
                    ..Edges::default()
                },
            ) {
                window.refresh();
            }
        });
    }
}

pub(crate) fn row_is_visible(
    top: Pixels,
    height: Pixels,
    scroll: &ScrollHandle,
    inset: Pixels,
) -> bool {
    let viewport = scroll.bounds().size.height;
    let top = top + scroll.offset().y;
    viewport <= px(0.) || (top + height > inset && top < viewport)
}

#[derive(Clone, Default)]
pub struct VirtualListScrollHandle {
    base: ScrollHandle,
    metrics: Rc<RefCell<Metrics>>,
    elastic: Rc<RefCell<ElasticOverscroll>>,
}

#[derive(Default)]
struct Metrics {
    offsets: Vec<Pixels>,
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

    pub fn scroll_to_item(&self, ix: usize, strategy: ScrollStrategy) {
        let metrics = self.metrics.borrow();
        let (Some(top), Some(height)) = (metrics.offset_of(ix), metrics.height_of(ix)) else {
            return;
        };

        let viewport = self.base.bounds().size.height;
        let current = -self.base.offset().y;

        let target = match strategy {
            ScrollStrategy::Top => top,
            ScrollStrategy::Center => top - (viewport - height) / 2.,
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
        self.base
            .set_offset(Point::new(self.base.offset().x, -clamped));
    }

    pub fn bounds(&self) -> Bounds<Pixels> {
        self.base.bounds()
    }

    pub fn scroll_by_y(&self, delta: Pixels) -> Pixels {
        let metrics = self.metrics.borrow();
        let offset = self.base.offset();
        let next = clamped_scroll_offset(
            offset.y,
            delta,
            metrics.total(),
            self.base.bounds().size.height,
        );
        self.base.set_offset(Point::new(offset.x, next));
        next - offset.y
    }

    pub fn row_at_position(&self, position: Point<Pixels>) -> Option<(usize, Bounds<Pixels>)> {
        let viewport = self.base.bounds();
        let local = viewport.localize(&position)?;
        let metrics = self.metrics.borrow();
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
    scroll_to: Option<(usize, ScrollStrategy)>,
    render: RenderRange,
    notify: Box<dyn Fn(&mut App)>,
}

impl VirtualList {
    pub fn scroll_to_item(mut self, index: usize, strategy: ScrollStrategy) -> Self {
        self.scroll_to = Some((index, strategy));
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
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let handle = self.scroll_handle.unwrap_or_default();

        let mut offsets = Vec::with_capacity(self.item_sizes.len() + 1);
        let mut running = px(0.);
        offsets.push(running);
        for size in self.item_sizes.iter() {
            running += size.height;
            offsets.push(running);
        }
        let total = running;
        handle.metrics.borrow_mut().offsets = offsets.clone();
        if let Some((index, strategy)) = self.scroll_to {
            handle.scroll_to_item(index, strategy);
        }

        let viewport = handle.base.bounds().size.height;
        let scrolled = -handle.base.offset().y;

        let range = if viewport <= px(0.) {
            0..self.item_sizes.len().min(FIRST_FRAME_ROWS)
        } else {
            let first = offsets
                .partition_point(|offset| *offset <= scrolled)
                .saturating_sub(1);
            let last = offsets.partition_point(|offset| *offset < scrolled + viewport);
            first.saturating_sub(OVERSCAN)..(last + OVERSCAN).min(self.item_sizes.len())
        };

        let rows = (self.render)(range.clone(), window, cx);
        let focus = ScrollFocus::new(self.id, window, cx);
        let overscroll_y = handle.elastic.borrow_mut().advance(cx);
        let placed: Vec<_> = rows
            .into_iter()
            .enumerate()
            .map(|(nth, row)| {
                let ix = range.start + nth;
                let top = offsets.get(ix).copied().unwrap_or_default();
                let height = self.item_sizes[ix].height;
                let visible = row_is_visible(top + overscroll_y, height, &handle.base, px(0.));
                let focus = focus.clone();
                let scroll = handle.base.clone();
                div()
                    .id(("virtual-list-row", ix))
                    .focusable()
                    .tab_stop(false)
                    .absolute()
                    .top(top)
                    .left_0()
                    .right_0()
                    .when(!visible, |row| row.invisible())
                    .on_focus_resolved(move |bounds, row, window, cx| {
                        focus.reveal(bounds, row, &scroll, px(0.), window, cx);
                    })
                    .child(row)
            })
            .collect();
        if handle.elastic.borrow().needs_frame(cx) {
            window.request_animation_frame();
        }
        let elastic = handle.elastic.clone();
        let notify = self.notify;
        let measured = handle.base.clone();

        self.base
            .overflow_y_scroll()
            .track_scroll(&handle.base)
            .on_focus_resolved(move |_, _, window, _| {
                if viewport <= px(0.) && measured.bounds().size.height > px(0.) {
                    window.refresh();
                }
            })
            .on_scroll_wheel(move |event, window, cx| {
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
    }
}
