use crate::easing::ease_out_cubic;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::components::{
    MarqueeSelection, elastic_overscroll::ElasticOverscroll, scrollbar::Scrollbar,
    virtual_list::ScrollFocus,
};
use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, DragMoveEvent, ElementId, Entity,
    EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Pixels, Render, RenderOnce, ScrollHandle, SharedString, Size,
    StatefulInteractiveElement, StyleRefinement, Styled, WeakEntity, Window, container_query, div,
    prelude::FluentBuilder as _, px,
};
use gpui_kit_theme::ActiveTheme;

use super::{DYNAMIC_LIST_ITEM_GAP, DYNAMIC_LIST_ITEM_HEIGHT, DynamicListDelegate};
use crate::components::ext::{ElementExt as _, StyledRefineExt as _};

const DEFAULT_OVERSCAN: usize = 6;
const SHIFT_MS: u64 = 180;
const STRUCT_MS: u64 = 220;
const EDGE_ZONE: f32 = 56.0;
const EDGE_MAX_SPEED: f32 = 18.0;

const FALLBACK_PREVIEW_WIDTH: Pixels = px(240.);

fn edge_scroll_speed(local_y: f32, viewport_height: f32) -> Option<f32> {
    if local_y < EDGE_ZONE {
        let t = (1.0 - (local_y / EDGE_ZONE)).clamp(0.0, 1.0);
        Some(t * t * EDGE_MAX_SPEED)
    } else if local_y > viewport_height - EDGE_ZONE {
        let t = (1.0 - ((viewport_height - local_y) / EDGE_ZONE)).clamp(0.0, 1.0);
        Some(-(t * t * EDGE_MAX_SPEED))
    } else {
        None
    }
}

fn hover_gap_for_row(hovered: usize, from: usize) -> usize {
    match hovered.cmp(&from) {
        std::cmp::Ordering::Greater => hovered + 1,
        std::cmp::Ordering::Less => hovered,
        std::cmp::Ordering::Equal => from,
    }
}

fn commit_slot(from: usize, raw_gap: usize, len: usize) -> usize {
    let raw = raw_gap.min(len);
    if raw > from { raw - 1 } else { raw }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicListDrag {
    pub list_id: ElementId,
    pub item_id: ElementId,
    pub ix: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DynamicListEvent {
    DragStarted { ix: usize },
    DragEnded,
    Moved { from: usize, to: usize },
    LayoutChanged { scroll_delta: Pixels },
}

#[derive(Clone, PartialEq)]
struct RowLayout {
    id: ElementId,
    top: f32,
    height: f32,
}

fn layout_rows(
    items: impl ExactSizeIterator<Item = (ElementId, Pixels)>,
    inset: f32,
    padding: f32,
    gap: f32,
) -> (Vec<RowLayout>, f32) {
    let mut rows = Vec::with_capacity(items.len());
    let mut cursor = inset + padding;
    for (id, height) in items {
        let height = f32::from(height).max(0.0);
        rows.push(RowLayout {
            id,
            top: cursor,
            height,
        });
        cursor += height + gap;
    }
    let content_height = if rows.is_empty() {
        inset + padding * 2.0
    } else {
        (cursor - gap).max(inset + padding) + padding
    };
    (rows, content_height)
}

fn row_scroll_bounds(rows: &[RowLayout], ix: usize, padding: f32) -> Option<std::ops::Range<f32>> {
    let row = rows.get(ix)?;
    let top = row.top - if ix == 0 { padding } else { 0.0 };
    let bottom = row.top + row.height + if ix + 1 == rows.len() { padding } else { 0.0 };
    Some(top..bottom)
}

fn reveal_scroll_target(
    bounds: std::ops::Range<f32>,
    scroll_top: f32,
    viewport_height: f32,
    inset: f32,
) -> Option<f32> {
    if bounds.end - bounds.start > viewport_height - inset || bounds.start < scroll_top + inset {
        Some(bounds.start - inset)
    } else if bounds.end > scroll_top + viewport_height {
        Some(bounds.end - viewport_height)
    } else {
        None
    }
}

fn drag_preview_width(measured: Pixels, padding: Pixels) -> Pixels {
    let width = if measured > px(0.) {
        measured
    } else {
        FALLBACK_PREVIEW_WIDTH
    };
    if padding == px(0.) {
        width
    } else {
        (width - padding * 2.).max(px(1.))
    }
}

struct ScrollAnchor {
    id: ElementId,
    within: f32,
}

impl ScrollAnchor {
    fn capture(rows: &[RowLayout], offset: f32, inset: f32) -> Option<Self> {
        if offset >= 0.0 {
            return None;
        }
        let visible_top = -offset + inset;
        let row = rows.iter().find(|row| row.top + row.height > visible_top)?;
        Some(Self {
            id: row.id.clone(),
            within: visible_top - row.top,
        })
    }

    fn offset(&self, rows: &[RowLayout], inset: f32) -> Option<f32> {
        let row = rows.iter().find(|row| row.id == self.id)?;
        let within = if self.within < row.height {
            self.within
        } else {
            0.0
        };
        Some(inset - row.top - within)
    }
}

fn clamp_scroll_offset(offset: f32, content_height: f32, viewport_height: f32) -> f32 {
    offset.clamp(-(content_height - viewport_height).max(0.0), 0.0)
}

fn row_at(rows: &[RowLayout], y: f32) -> usize {
    rows.partition_point(|row| row.top <= y).saturating_sub(1)
}

fn visible_rows(
    rows: &[RowLayout],
    scroll_top: f32,
    viewport_height: f32,
    overscan: usize,
) -> std::ops::Range<usize> {
    if rows.is_empty() {
        return 0..0;
    }
    let first = row_at(rows, scroll_top).saturating_sub(overscan);
    let last = (row_at(rows, scroll_top + viewport_height) + overscan).min(rows.len() - 1);
    first..last + 1
}

struct DepartingRow<D> {
    id: ElementId,
    ix: usize,
    top: f32,
    height: f32,
    started: Instant,
    opacity_from: f32,
    delegate: Rc<RefCell<D>>,
}

impl<D> DepartingRow<D> {
    fn progress(&self, now: Instant) -> f32 {
        (now.saturating_duration_since(self.started).as_secs_f32()
            / Duration::from_millis(STRUCT_MS).as_secs_f32())
        .clamp(0.0, 1.0)
    }

    fn opacity(&self, now: Instant) -> f32 {
        self.opacity_from * (1.0 - ease_out_cubic(self.progress(now)))
    }

    fn settled(&self, now: Instant) -> bool {
        self.progress(now) >= 1.0
    }
}

struct Motion {
    from: f32,
    to: f32,
    started: Instant,
    duration: Duration,
    entering: bool,
}

impl Motion {
    fn resting(at: f32, started: Instant, entering: bool) -> Self {
        Self {
            from: at,
            to: at,
            started,
            duration: if entering {
                Duration::from_millis(STRUCT_MS)
            } else {
                Duration::ZERO
            },
            entering,
        }
    }

    fn snapped(at: f32, now: Instant) -> Self {
        Self {
            from: at,
            to: at,
            started: now,
            duration: Duration::ZERO,
            entering: false,
        }
    }

    fn reflow(&mut self, top: f32) {
        self.from = top;
        self.to = top;
        if !self.entering {
            self.duration = Duration::ZERO;
        }
    }

    fn progress(&self, now: Instant) -> f32 {
        let duration = self.duration.as_secs_f32();
        if duration <= 0.0 {
            return 1.0;
        }
        (now.saturating_duration_since(self.started).as_secs_f32() / duration).clamp(0.0, 1.0)
    }

    fn y(&self, now: Instant) -> f32 {
        let t = ease_out_cubic(self.progress(now));
        self.from + (self.to - self.from) * t
    }

    fn settled(&self, now: Instant) -> bool {
        self.progress(now) >= 1.0
    }
}

enum ScrollRequest {
    Explicit(ElementId),
    Priority(ElementId),
}

pub struct DynamicListState<D: DynamicListDelegate> {
    id: ElementId,
    focus_handle: FocusHandle,
    delegate: D,
    scroll_handle: ScrollHandle,
    viewport: Bounds<Pixels>,
    pending_scroll: Option<ScrollRequest>,
    rows: Vec<RowLayout>,
    content_height: f32,
    layout_dirty: bool,
    layout_epoch: u64,
    marquee_active: bool,
    gap: Pixels,
    content_padding: Pixels,
    content_inset_top: Pixels,
    overscan: usize,
    scrollbar_visible: bool,
    elastic_overscroll: bool,
    overscroll: ElasticOverscroll,
    dragging_from: Option<usize>,
    hover_gap: Option<usize>,
    drag_amount: f32,
    edge_scroll_speed: Option<f32>,
    motions: HashMap<ElementId, Motion>,
    departing: Vec<DepartingRow<D>>,
    initialized: bool,
}

impl<D: DynamicListDelegate> DynamicListState<D> {
    pub fn new(delegate: D, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let layout_epoch = delegate.layout_epoch();
        let mut this = Self {
            id: ElementId::View(cx.entity_id()),
            focus_handle: cx.focus_handle(),
            delegate,
            scroll_handle: ScrollHandle::new(),
            viewport: Bounds::default(),
            pending_scroll: None,
            rows: Vec::new(),
            content_height: 0.0,
            layout_dirty: true,
            layout_epoch,
            marquee_active: false,
            gap: DYNAMIC_LIST_ITEM_GAP,
            content_padding: px(0.),
            content_inset_top: px(0.),
            overscan: DEFAULT_OVERSCAN,
            scrollbar_visible: true,
            elastic_overscroll: true,
            overscroll: ElasticOverscroll::default(),
            dragging_from: None,
            hover_gap: None,
            drag_amount: 0.0,
            edge_scroll_speed: None,
            motions: HashMap::new(),
            departing: Vec::new(),
            initialized: false,
        };
        this.rebuild_layout(cx);
        this
    }

    fn invalidate_initial_layout(&mut self) {
        self.layout_dirty = true;
        self.motions.clear();
        self.departing.clear();
        self.initialized = false;
    }

    pub fn gap(mut self, gap: Pixels) -> Self {
        self.gap = gap;
        self.invalidate_initial_layout();
        self
    }

    pub fn content_padding(mut self, padding: Pixels) -> Self {
        self.content_padding = padding.max(px(0.));
        self.invalidate_initial_layout();
        self
    }

    pub fn content_inset_top(mut self, inset: Pixels) -> Self {
        self.content_inset_top = inset.max(px(0.));
        self.invalidate_initial_layout();
        self
    }

    pub fn scrollbar_visible(mut self, visible: bool) -> Self {
        self.scrollbar_visible = visible;
        self
    }

    pub fn set_scrollbar_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.scrollbar_visible != visible {
            self.scrollbar_visible = visible;
            cx.notify();
        }
    }

    pub fn elastic_overscroll(mut self, enabled: bool) -> Self {
        self.elastic_overscroll = enabled;
        self
    }

    pub fn delegate(&self) -> &D {
        &self.delegate
    }

    pub fn delegate_mut(&mut self) -> &mut D {
        &mut self.delegate
    }

    pub fn scroll_handle(&self) -> &ScrollHandle {
        &self.scroll_handle
    }

    pub fn set_marquee_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.marquee_active != active {
            self.marquee_active = active;
            cx.notify();
        }
    }

    pub fn scroll_marquee(
        &mut self,
        marquee: &mut MarqueeSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let scrolled = marquee.scroll(&self.scroll_handle, self.content_inset_top, window);
        self.set_marquee_active(marquee.is_active(), cx);
        if scrolled {
            cx.notify();
        }
    }

    pub fn scroll_by_y(&mut self, delta: Pixels) -> Pixels {
        if self.viewport.size.width <= px(0.) || self.viewport.size.height <= px(0.) {
            return px(0.);
        }
        let viewport_height = f32::from(self.viewport.size.height);
        let max_scroll_down = (self.content_height - viewport_height).max(0.0);
        let mut offset = self.scroll_handle.offset();
        let previous = offset.y;
        offset.y = px((f32::from(offset.y) + f32::from(delta)).clamp(-max_scroll_down, 0.0));
        self.scroll_handle.set_offset(offset);
        offset.y - previous
    }

    pub fn content_height(&self) -> Pixels {
        px(self.content_height)
    }

    pub fn reorder_handle(
        ix: usize,
        item_id: ElementId,
        child: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let list_id = ElementId::View(cx.entity_id());
        let weak = cx.entity().downgrade();
        let drag = DynamicListDrag {
            list_id,
            item_id: item_id.clone(),
            ix,
        };

        div()
            .id(ElementId::from((
                item_id,
                SharedString::new_static("dynamic-list-reorder-handle"),
            )))
            .cursor_grab()
            .block_mouse_except_scroll()
            .on_drag(drag, move |drag, _offset, _window, cx| {
                let (width, height) = weak
                    .update(cx, |this, cx| {
                        this.begin_drag(drag.ix, cx);
                        let width =
                            drag_preview_width(this.viewport.size.width, this.content_padding);
                        let height = this
                            .rows
                            .get(drag.ix)
                            .map(|row| px(row.height))
                            .unwrap_or(DYNAMIC_LIST_ITEM_HEIGHT);
                        (width, height)
                    })
                    .unwrap_or((FALLBACK_PREVIEW_WIDTH, DYNAMIC_LIST_ITEM_HEIGHT));
                let state = weak.clone();
                cx.new(move |_| DynamicListDragPreview {
                    state,
                    item_id: drag.item_id.clone(),
                    width,
                    height,
                })
            })
            .child(child)
    }

    pub fn update_items<R>(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut D, &mut Context<Self>) -> R,
    ) -> R {
        let previous_delegate = Rc::new(RefCell::new(self.delegate.clone()));
        let result = f(&mut self.delegate, cx);
        self.rebuild_layout_with_departures(cx, Some(previous_delegate));
        cx.notify();
        result
    }

    pub fn reset_items<R>(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut D, &mut Context<Self>) -> R,
    ) -> R {
        let result = f(&mut self.delegate, cx);
        self.reset_layout();
        self.rebuild_layout(cx);
        cx.notify();
        result
    }

    fn reset_layout(&mut self) {
        self.dragging_from = None;
        self.hover_gap = None;
        self.edge_scroll_speed = None;
        self.marquee_active = false;
        self.scroll_handle.set_offset(Default::default());
        self.pending_scroll = None;
        self.overscroll.reset();
        self.departing.clear();
        self.motions.clear();
        self.initialized = false;
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.layout_dirty = true;
        cx.notify();
    }

    /// Reveal a row before focusing it, without letting reflow hide it or report old focus bounds.
    pub fn reveal_item_for_focus(&mut self, ix: usize, cx: &mut Context<Self>) {
        if self.layout_changed(cx) {
            self.rebuild_layout(cx);
        }
        let Some(row) = self.rows.get(ix) else {
            return;
        };
        self.motions
            .insert(row.id.clone(), Motion::snapped(row.top, Instant::now()));
        self.overscroll.reset();
        self.scroll_item_into_view(ix, cx);
        cx.notify();
    }

    pub fn scroll_item_into_view(&mut self, ix: usize, cx: &mut Context<Self>) {
        if self.layout_changed(cx) {
            self.rebuild_layout(cx);
        }
        let Some(row) = self.rows.get(ix) else {
            return;
        };
        self.pending_scroll = Some(ScrollRequest::Explicit(row.id.clone()));
        cx.notify();
    }

    pub(crate) fn reveal_priority(&mut self, id: ElementId) {
        if !matches!(
            self.pending_scroll.as_ref(),
            Some(ScrollRequest::Explicit(_))
        ) {
            self.pending_scroll = Some(ScrollRequest::Priority(id));
        }
    }

    fn apply_pending_scroll(&mut self) {
        let Some(ScrollRequest::Explicit(id) | ScrollRequest::Priority(id)) =
            self.pending_scroll.take()
        else {
            return;
        };
        let Some(ix) = self.rows.iter().position(|row| row.id == id) else {
            return;
        };
        let Some(bounds) = row_scroll_bounds(&self.rows, ix, f32::from(self.content_padding))
        else {
            return;
        };
        let viewport_height = f32::from(self.viewport.size.height);
        let mut offset = self.scroll_handle.offset();
        let scroll_top = (-f32::from(offset.y)).max(0.0);
        let Some(target) = reveal_scroll_target(
            bounds,
            scroll_top,
            viewport_height,
            f32::from(self.content_inset_top),
        ) else {
            return;
        };

        let max_scroll = (self.content_height - viewport_height).max(0.0);
        offset.y = px(-target.clamp(0.0, max_scroll));
        self.scroll_handle.set_offset(offset);
    }

    fn clamp_scroll(&mut self, cx: &mut Context<Self>) {
        let mut offset = self.scroll_handle.offset();
        let previous = offset.y;
        offset.y = px(clamp_scroll_offset(
            f32::from(offset.y),
            self.content_height,
            f32::from(self.viewport.size.height),
        ));
        if offset.y != previous {
            self.scroll_handle.set_offset(offset);
            cx.emit(DynamicListEvent::LayoutChanged {
                scroll_delta: offset.y - previous,
            });
        }
    }

    fn layout_changed(&self, cx: &App) -> bool {
        self.layout_dirty
            || self.layout_epoch != self.delegate.layout_epoch()
            || self.rows.len() != self.delegate.items_count(cx)
            || self.rows.iter().enumerate().any(|(ix, row)| {
                row.id != self.delegate.item_id(ix, cx)
                    || row.height != f32::from(self.delegate.item_height(ix, cx)).max(0.0)
            })
    }

    fn rebuild_layout(&mut self, cx: &mut Context<Self>) {
        self.rebuild_layout_with_departures(cx, None);
    }

    fn rebuild_layout_with_departures(
        &mut self,
        cx: &mut Context<Self>,
        mut previous_delegate: Option<Rc<RefCell<D>>>,
    ) {
        let offset = self.scroll_handle.offset();
        let epoch_changed = self.layout_epoch != self.delegate.layout_epoch();
        if epoch_changed {
            self.reset_layout();
            self.layout_epoch = self.delegate.layout_epoch();
            previous_delegate = None;
        }
        let count = self.delegate.items_count(cx);
        let gap = f32::from(self.gap);
        let inset = f32::from(self.content_inset_top);
        let padding = f32::from(self.content_padding);
        let items = (0..count).map(|ix| {
            let height = self.delegate.item_height(ix, cx);
            (self.delegate.item_id(ix, cx), height)
        });
        let (rows, content_height) = layout_rows(items, inset, padding, gap);
        let geometry_changed = rows != self.rows;
        let height_reflow = self.initialized
            && geometry_changed
            && rows.len() == self.rows.len()
            && rows
                .iter()
                .zip(&self.rows)
                .all(|(new, old)| new.id == old.id);
        if geometry_changed && !height_reflow {
            self.cancel_drag(cx);
        }
        let anchor = height_reflow
            .then(|| ScrollAnchor::capture(&self.rows, f32::from(offset.y), inset))
            .flatten();
        self.content_height = content_height;

        let now = Instant::now();
        let reduce_motion = cx.reduce_motion();
        let live_ids: HashSet<_> = rows.iter().map(|row| row.id.clone()).collect();
        if reduce_motion || count == 0 {
            self.departing.clear();
        } else {
            self.departing.retain(|row| !live_ids.contains(&row.id));

            if let Some(previous_delegate) = previous_delegate {
                let already_departing: HashSet<_> =
                    self.departing.iter().map(|row| row.id.clone()).collect();
                for (ix, row) in self.rows.iter().enumerate() {
                    if live_ids.contains(&row.id) || already_departing.contains(&row.id) {
                        continue;
                    }
                    let (top, opacity_from) =
                        self.motions.get(&row.id).map_or((row.top, 1.0), |motion| {
                            let opacity = if motion.entering {
                                ease_out_cubic(motion.progress(now))
                            } else {
                                1.0
                            };
                            (motion.y(now), opacity)
                        });
                    self.departing.push(DepartingRow {
                        id: row.id.clone(),
                        ix,
                        top,
                        height: row.height,
                        started: now,
                        opacity_from,
                        delegate: previous_delegate.clone(),
                    });
                }
            }
        }

        let first_build = !self.initialized;
        let mut motions = HashMap::with_capacity(rows.len());
        for row in &rows {
            let mut motion = if reduce_motion {
                Motion::snapped(row.top, now)
            } else {
                self.motions
                    .remove(&row.id)
                    .unwrap_or_else(|| Motion::resting(row.top, now, !first_build))
            };
            if height_reflow {
                motion.reflow(row.top);
            }
            motions.insert(row.id.clone(), motion);
        }
        self.motions = motions;

        self.rows = rows;
        self.layout_dirty = false;
        self.initialized = true;
        if geometry_changed || epoch_changed {
            let target = anchor
                .and_then(|anchor| anchor.offset(&self.rows, inset))
                .unwrap_or(f32::from(offset.y));
            let next = if count == 0 || epoch_changed {
                gpui::point(px(0.), px(0.))
            } else {
                // Rebuilds can run while hidden; clamp only after this frame's viewport is assigned.
                gpui::point(offset.x, px(target.min(0.0)))
            };
            if next != offset {
                self.scroll_handle.set_offset(next);
            }
            if let Some(row) = self.dragging_from.and_then(|ix| self.rows.get(ix)) {
                self.drag_amount = row.height + gap;
            }
        }
        if geometry_changed || epoch_changed {
            cx.emit(DynamicListEvent::LayoutChanged {
                scroll_delta: self.scroll_handle.offset().y - offset.y,
            });
        }
    }

    fn advance(
        &mut self,
        id: &ElementId,
        target: f32,
        duration: u64,
        reduce_motion: bool,
    ) -> (f32, f32) {
        let now = Instant::now();
        let Some(motion) = self.motions.get_mut(id) else {
            return (target, 1.0);
        };

        if reduce_motion {
            *motion = Motion::snapped(target, now);
            return (target, 1.0);
        }

        if motion.to != target {
            let from = motion.y(now);
            *motion = Motion {
                from,
                to: target,
                started: now,
                duration: Duration::from_millis(duration),
                entering: false,
            };
        }

        let opacity = if motion.entering {
            ease_out_cubic(motion.progress(now))
        } else {
            1.0
        };
        (motion.y(now), opacity)
    }

    fn is_settling(&self) -> bool {
        let now = Instant::now();
        self.motions.values().any(|motion| !motion.settled(now))
            || self.departing.iter().any(|row| !row.settled(now))
    }

    fn remove_settled_departures(&mut self) {
        let now = Instant::now();
        self.departing.retain(|row| !row.settled(now));
    }

    fn finish_nonessential_motion(&mut self) {
        let now = Instant::now();
        self.departing.clear();
        for row in &self.rows {
            self.motions
                .insert(row.id.clone(), Motion::snapped(row.top, now));
        }
    }

    fn render_departing_rows(
        &mut self,
        overscroll_y: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let now = Instant::now();
        let scroll_y = f32::from(self.scroll_handle.offset().y);
        let viewport_height = f32::from(self.viewport.size.height);
        let mut rows = Vec::new();

        for row in &self.departing {
            let screen_top = row.top + scroll_y + f32::from(overscroll_y);
            if screen_top + row.height < 0.0 || screen_top > viewport_height {
                continue;
            }
            let content = row.delegate.borrow_mut().render_item(row.ix, window, cx);
            rows.push(
                div()
                    .id(ElementId::from((
                        row.id.clone(),
                        SharedString::new_static("dynamic-list-departing-row"),
                    )))
                    .absolute()
                    .top(px(screen_top))
                    .left(self.content_padding)
                    .right(self.content_padding)
                    .h(px(row.height))
                    .opacity(row.opacity(now))
                    .block_mouse_except_scroll()
                    .children(content)
                    .into_any_element(),
            );
        }

        rows
    }

    fn item_at(&self, y: f32) -> usize {
        row_at(&self.rows, y)
    }

    fn hover_gap_for_y(&self, y: f32, from: usize) -> usize {
        hover_gap_for_row(self.item_at(y), from)
    }

    fn commit_slot(&self, from: usize, raw_gap: usize) -> usize {
        commit_slot(from, raw_gap, self.rows.len())
    }

    fn begin_drag(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.dragging_from = Some(ix);
        self.hover_gap = Some(ix);
        self.drag_amount =
            self.rows.get(ix).map(|row| row.height).unwrap_or(0.0) + f32::from(self.gap);
        self.edge_scroll_speed = None;
        cx.emit(DynamicListEvent::DragStarted { ix });
        cx.notify();
    }

    fn drag_moved(&mut self, pointer_y: Pixels, content_top: Pixels, cx: &mut Context<Self>) {
        let Some(from) = self.dragging_from else {
            return;
        };
        let mut changed = false;

        let gap = self.hover_gap_for_y(f32::from(pointer_y - content_top), from);
        if self.hover_gap != Some(gap) {
            self.hover_gap = Some(gap);
            changed = true;
        }

        let viewport = self.viewport;
        let local_y = f32::from(pointer_y - viewport.origin.y);
        let speed = edge_scroll_speed(local_y, f32::from(viewport.size.height));
        if self.edge_scroll_speed != speed {
            self.edge_scroll_speed = speed;
            changed = true;
        }

        if changed {
            cx.notify();
        }
    }

    fn finish_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.edge_scroll_speed = None;

        if let (Some(from), Some(gap)) = (self.dragging_from.take(), self.hover_gap.take()) {
            let to = self.commit_slot(from, gap);
            if to != from {
                let dragged_id = self.rows.get(from).map(|row| row.id.clone());
                self.delegate.move_item(from, to, window, cx);
                self.rebuild_layout(cx);

                if let Some(dragged_id) = dragged_id {
                    let committed_top = self
                        .rows
                        .iter()
                        .find(|row| row.id == dragged_id)
                        .map(|row| row.top);
                    if let Some(committed_top) = committed_top {
                        self.motions
                            .insert(dragged_id, Motion::snapped(committed_top, Instant::now()));
                    }
                }

                cx.emit(DynamicListEvent::Moved { from, to });
            }
        }
        cx.emit(DynamicListEvent::DragEnded);
        cx.notify();
    }

    fn cancel_drag(&mut self, cx: &mut Context<Self>) {
        if self.dragging_from.take().is_some() {
            self.hover_gap = None;
            self.edge_scroll_speed = None;
            cx.emit(DynamicListEvent::DragEnded);
            cx.notify();
        }
    }

    fn apply_edge_scroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(speed) = self.edge_scroll_speed else {
            return;
        };
        let viewport = self.viewport;
        let moved = self.scroll_by_y(px(speed));
        if moved != px(0.) {
            let content_top = viewport.origin.y + self.scroll_handle.offset().y;
            self.drag_moved(window.mouse_position().y, content_top, cx);
        }
    }

    fn visible_range(&self) -> std::ops::Range<usize> {
        if self.rows.is_empty() {
            return 0..0;
        }
        let scroll_top = (-f32::from(self.scroll_handle.offset().y)).max(0.0);
        let viewport_height = f32::from(self.viewport.size.height);

        visible_rows(&self.rows, scroll_top, viewport_height, self.overscan)
    }

    fn render_row(
        &mut self,
        ix: usize,
        weak: &WeakEntity<Self>,
        preview_width: Pixels,
        focus: &ScrollFocus,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (item_id, top, height) = {
            let row = &self.rows[ix];
            (row.id.clone(), row.top, row.height)
        };

        if Some(ix) == self.dragging_from {
            return div()
                .absolute()
                .top(px(top))
                .left(self.content_padding)
                .right(self.content_padding)
                .h(px(height))
                .opacity(0.0)
                .into_any_element();
        }

        let side = match self.dragging_from {
            Some(from) if ix > from => -1.0,
            Some(from) if ix < from => 1.0,
            _ => 0.0,
        };
        let shifted = match (self.dragging_from, self.hover_gap) {
            (Some(from), Some(gap)) if gap > from => ix > from && ix < gap,
            (Some(from), Some(gap)) if gap < from => ix >= gap && ix < from,
            _ => false,
        };

        let (target, duration) = if shifted {
            (top + side * self.drag_amount, SHIFT_MS)
        } else if self.dragging_from.is_some() {
            (top, SHIFT_MS)
        } else {
            (top, STRUCT_MS)
        };
        let (y, opacity) = self.advance(&item_id, target, duration, cx.reduce_motion());

        let can_drag = self.delegate.can_drag(ix, cx);
        let content = self.delegate.render_item(ix, window, cx);
        let list_id = self.id.clone();
        let visible_top = px(y) + self.overscroll.offset() + self.scroll_handle.offset().y;
        let visible = visible_top + px(height) > self.content_inset_top
            && visible_top < self.viewport.size.height;
        let focus = focus.clone();
        let scroll = self.scroll_handle.clone();
        let inset = self.content_inset_top;

        div()
            .id(ElementId::from((
                item_id.clone(),
                SharedString::new_static("dynamic-list-row"),
            )))
            .focusable()
            .tab_stop(false)
            .when(!visible, |row| row.invisible())
            .on_focus_resolved(move |bounds, row, window, cx| {
                focus.reveal(bounds, row, &scroll, inset, window, cx);
            })
            .absolute()
            .top(px(y))
            .left(self.content_padding)
            .right(self.content_padding)
            .h(px(height))
            .when(opacity < 1.0, |this| this.opacity(opacity))
            .when(can_drag, |this| {
                let weak = weak.clone();
                let drag = DynamicListDrag {
                    list_id,
                    item_id: item_id.clone(),
                    ix,
                };
                this.cursor_grab()
                    .on_drag(drag, move |drag, _offset, _window, cx| {
                        let ix = drag.ix;
                        weak.update(cx, |this, cx| this.begin_drag(ix, cx)).ok();
                        let state = weak.clone();
                        cx.new(move |_| DynamicListDragPreview {
                            state,
                            item_id: drag.item_id.clone(),
                            width: preview_width,
                            height: px(height),
                        })
                    })
            })
            .children(content)
            .into_any_element()
    }
}

impl<D: DynamicListDelegate> Focusable for DynamicListState<D> {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl<D: DynamicListDelegate> EventEmitter<DynamicListEvent> for DynamicListState<D> {}

impl<D: DynamicListDelegate> Render for DynamicListState<D> {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let measured = view.clone();
        div()
            .id(ElementId::from((
                self.id.clone(),
                SharedString::new_static("dynamic-list"),
            )))
            .track_focus(&self.focus_handle)
            .relative()
            .size_full()
            .overflow_hidden()
            .on_prepaint(move |bounds, _, cx| {
                measured.update(cx, |list, _| list.viewport = bounds);
            })
            .child(container_query(move |size, window, cx| {
                view.update(cx, |list, cx| list.render_sized(size, window, cx))
            }))
    }
}

impl<D: DynamicListDelegate> DynamicListState<D> {
    fn render_sized(
        &mut self,
        size: Size<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.viewport.size = size;
        if size.width <= px(0.) || size.height <= px(0.) {
            self.delegate.pause_layout();
            return div().size_full().into_any_element();
        }
        if cx.reduce_motion() {
            self.departing.clear();
        } else {
            self.remove_settled_departures();
        }
        if !self.marquee_active && self.dragging_from.is_none() && !cx.has_active_drag() {
            self.delegate.prepare_layout(window, cx);
        } else {
            self.delegate.pause_layout();
        }
        if self.layout_changed(cx) {
            self.rebuild_layout(cx);
        }
        if cx.reduce_motion() {
            self.finish_nonessential_motion();
        }
        self.clamp_scroll(cx);
        self.apply_pending_scroll();
        self.apply_edge_scroll(window, cx);

        let overscroll_y = if self.elastic_overscroll {
            self.overscroll.advance(cx)
        } else {
            self.overscroll.reset();
            px(0.0)
        };

        let count = self.rows.len();
        let scrollbar_visible = self.scrollbar_visible && count > 0;

        let body: AnyElement = if count == 0 {
            div()
                .absolute()
                .top(self.content_inset_top + self.content_padding)
                .bottom(self.content_padding)
                .left(self.content_padding)
                .right(self.content_padding)
                .overflow_hidden()
                .child(self.delegate.render_empty(window, cx))
                .into_any_element()
        } else {
            let weak = cx.entity().downgrade();
            let preview_width = drag_preview_width(size.width, self.content_padding);

            let range = self.visible_range();
            let focus = ScrollFocus::new(self.id.clone(), window, cx);
            let mut rows = Vec::with_capacity(range.len());
            for ix in range {
                rows.push(self.render_row(ix, &weak, preview_width, &focus, window, cx));
            }

            let content_height = self.content_height;
            let list_id = self.id.clone();

            div()
                .id(ElementId::from((
                    self.id.clone(),
                    SharedString::new_static("scroll"),
                )))
                .size_full()
                .overflow_scroll()
                .track_scroll(&self.scroll_handle)
                .on_scroll_wheel({
                    let weak = weak.clone();
                    move |event, window, cx| {
                        weak.update(cx, |this, cx| {
                            if this.elastic_overscroll
                                && this.overscroll.handle_scroll(event, window, cx)
                            {
                                cx.notify();
                            }
                        })
                        .ok();
                    }
                })
                .on_mouse_up(MouseButton::Left, {
                    let weak = weak.clone();
                    move |_event, _window, cx| {
                        weak.update(cx, |this, cx| this.cancel_drag(cx)).ok();
                    }
                })
                .child(
                    div().relative().w_full().h(px(content_height)).child(
                        div()
                            .absolute()
                            .top(overscroll_y)
                            .left(px(0.))
                            .right(px(0.))
                            .h(px(content_height))
                            .on_drag_move::<DynamicListDrag>({
                                let weak = weak.clone();
                                move |event: &DragMoveEvent<DynamicListDrag>, _window, cx| {
                                    let pointer_y = event.event.position.y;
                                    let content_top = event.bounds.origin.y;
                                    weak.update(cx, |this, cx| {
                                        this.drag_moved(pointer_y, content_top, cx)
                                    })
                                    .ok();
                                }
                            })
                            .on_drop::<DynamicListDrag>({
                                let weak = weak.clone();
                                move |drag, window, cx| {
                                    if drag.list_id != list_id {
                                        return;
                                    }
                                    weak.update(cx, |this, cx| this.finish_drag(window, cx))
                                        .ok();
                                }
                            })
                            .children(rows),
                    ),
                )
                .into_any_element()
        };

        let departing_rows = self.render_departing_rows(overscroll_y, window, cx);

        if self.edge_scroll_speed.is_some()
            || self.is_settling()
            || (self.elastic_overscroll && self.overscroll.needs_frame(cx))
        {
            window.request_animation_frame();
        }

        div()
            .relative()
            .size_full()
            .child(body)
            .children(departing_rows)
            .when(scrollbar_visible, |this| {
                this.child(Scrollbar::new(
                    "scrollbar.vertical",
                    &self.scroll_handle,
                    gpui::Axis::Vertical,
                ))
            })
            .into_any_element()
    }
}

pub struct DynamicListDragPreview<D: DynamicListDelegate> {
    state: WeakEntity<DynamicListState<D>>,
    item_id: ElementId,
    width: Pixels,
    height: Pixels,
}

impl<D: DynamicListDelegate> Render for DynamicListDragPreview<D> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self
            .state
            .update(cx, |state, cx| {
                let ix = state.rows.iter().position(|row| row.id == self.item_id)?;
                self.height = px(state.rows[ix].height);
                state.delegate.render_drag_preview(ix, window, cx)
            })
            .ok()
            .flatten();

        div()
            .w(self.width)
            .h(self.height)
            .rounded(px(cx.theme().radii.control))
            .border_1()
            .border_color(cx.theme().colors.accent)
            .bg(cx.theme().colors.canvas)
            .text_color(cx.theme().colors.text)
            .shadow_lg()
            .overflow_hidden()
            .children(content)
    }
}

#[derive(IntoElement)]
pub struct DynamicList<D: DynamicListDelegate> {
    state: Entity<DynamicListState<D>>,
    style: StyleRefinement,
}

impl<D: DynamicListDelegate> DynamicList<D> {
    pub fn new(state: &Entity<DynamicListState<D>>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl<D: DynamicListDelegate> Styled for DynamicList<D> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl<D: DynamicListDelegate> RenderOnce for DynamicList<D> {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .size_full()
            .refine_style(&self.style)
            .child(self.state.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_layout_and_scroll_geometry() {
        for inset in [0.0, 32.0] {
            for padding in [0.0, 12.0] {
                let items = [40.0, 60.0, 20.0]
                    .into_iter()
                    .enumerate()
                    .map(|(ix, height)| (ElementId::from(ix), px(height)));
                let (rows, height) = layout_rows(items, inset, padding, 8.0);
                assert_eq!(rows[0].top, inset + padding);
                assert_eq!(rows[1].top, inset + padding + 48.0);
                assert_eq!(rows[2].top, inset + padding + 116.0);
                assert_eq!(height, inset + padding * 2.0 + 136.0);

                let first = row_scroll_bounds(&rows, 0, padding).unwrap();
                let middle = row_scroll_bounds(&rows, 1, padding).unwrap();
                let last = row_scroll_bounds(&rows, 2, padding).unwrap();
                assert_eq!(first, inset..rows[0].top + rows[0].height);
                assert_eq!(middle, rows[1].top..rows[1].top + rows[1].height);
                assert_eq!(last, rows[2].top..height);
                assert_eq!(first.start - inset, 0.0);

                let bottom_target = reveal_scroll_target(last.clone(), 0.0, 100.0, inset).unwrap();
                let offset = clamp_scroll_offset(-bottom_target, height, 100.0);
                assert_eq!(offset + rows[2].top + rows[2].height + padding, 100.0);
                assert_eq!(
                    reveal_scroll_target(first.clone(), -offset, 100.0, inset),
                    Some(0.0)
                );
                assert_eq!(
                    reveal_scroll_target(middle.clone(), middle.start - inset, 100.0, inset),
                    None
                );

                let padding_target = (padding > 0.0).then_some(0.0);
                assert_eq!(
                    reveal_scroll_target(first, padding / 2.0, 100.0, inset),
                    padding_target
                );
                let padding_target = (padding > 0.0).then_some(height - 100.0);
                assert_eq!(
                    reveal_scroll_target(last, height - padding - 100.0, 100.0, inset),
                    padding_target
                );
            }
        }
    }

    #[test]
    fn padding_without_inter_row_gaps() {
        for inset in [0.0, 32.0] {
            for padding in [0.0, 12.0] {
                let (rows, height) = layout_rows(std::iter::empty(), inset, padding, 8.0);
                assert!(rows.is_empty());
                assert_eq!(height, inset + padding * 2.0);
                assert_eq!(row_scroll_bounds(&rows, 0, padding), None);

                let (rows, height) = layout_rows(
                    std::iter::once((ElementId::from("only"), px(40.))),
                    inset,
                    padding,
                    8.0,
                );
                assert_eq!(height, inset + padding * 2.0 + 40.0);
                let bounds = row_scroll_bounds(&rows, 0, padding).unwrap();
                assert_eq!(bounds, inset..height);
                assert_eq!(reveal_scroll_target(bounds, 0.0, 100.0, inset), None);
                assert_eq!(clamp_scroll_offset(-100.0, height, 100.0), 0.0);
            }
        }
    }

    #[test]
    fn preview_width_excludes_padding() {
        assert_eq!(drag_preview_width(px(200.), px(12.)), px(176.));
        assert_eq!(
            drag_preview_width(px(0.), px(12.)),
            FALLBACK_PREVIEW_WIDTH - px(24.)
        );
        assert_eq!(drag_preview_width(px(20.), px(12.)), px(1.));
        assert_eq!(drag_preview_width(px(0.5), px(0.)), px(0.5));
    }
}
