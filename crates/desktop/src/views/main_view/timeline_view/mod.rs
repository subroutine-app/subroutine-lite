use gpui_kit::foundation::StyledExt as _;
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use crate::components::ext::ElementExt;

use chrono::{DateTime, Duration as ChronoDuration, Local, NaiveDate};
use gpui::{
    App, AsyncApp, Bounds, Context, DragMoveEvent, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Pixels, Point,
    Render, ScrollHandle, Styled, Window, actions, div, prelude::FluentBuilder, px,
};

use crate::{
    components::{
        DragData, DraggedItems, DropZone, ItemCardStates, MarqueeSelection, MarqueeView, marquee,
    },
    item_manager::{DiscardDraft, DraftSubmitted, DraftView, ItemManager},
    keys::{force_click_modifier, key},
    selection::{SelectionManager, SelectionScope},
    settings::{GlobalSettings, Settings, TimelineToolbarPosition},
    views::{DeleteItem, TOP_EDGE_INSET},
};
use subroutine_core::{AnyItem, Marker, Signal, StartPrecision};
use timeline::compute_edge_scroll_speed;

use super::{
    InspectItem,
    tab::{MainViewTab, SelectedMainView},
};

mod divisions;
mod items;
mod timeline;
mod toolbar;

use divisions::*;
use items::*;

pub const HOUR_DIVIDER_HEIGHT: Pixels = px(32.);
pub const EDGE_HORIZON: Pixels = px(200.);

pub(super) const PIXEL_SCALE: ChronoDuration = ChronoDuration::seconds(16);
pub(super) const DEFAULT_PIXEL_DURATION: ChronoDuration = ChronoDuration::seconds(12);
pub(super) const POINTER_FLOOR_BOUNDARY_FRACTION: f64 = 0.5;
const MIN_PIXEL_DURATION: ChronoDuration = ChronoDuration::milliseconds(50);
const MAX_PIXEL_DURATION: ChronoDuration = ChronoDuration::days(2);
const MAX_PROJECTED_WORK_RANGE: ChronoDuration = ChronoDuration::days(90);
const MAX_PROJECTED_WORK_ITEMS: usize = 256;

const REFRESH_RATE: f64 = 25.0;

actions!([
    ZoomIn,
    ZoomOut,
    ZoomReset,
    FitSelection,
    CloseMarkerPanel,
    CloseTimelineBin,
    ZoomToFiveMinutes,
    ZoomToHours,
    ZoomToDays,
    ZoomToMonths,
    ZoomToYears,
    FocusItemUp,
    FocusItemDown,
    FocusItemLeft,
    FocusItemRight,
    ExtendItemPrevious,
    ExtendItemNext,
    NextSubdivision,
    PreivousSubdivision,
    NextDivision,
    PreviousDivision,
    NextOuterDivision,
    PreviousOuterDivision,
]);

pub(super) const IDLE_KEY_CONTEXT: &str =
    "TimelineView && !TextInput && !TextArea && !Editor && !TimelineMarkers && !Overlay";
const VIEW_KEY_CONTEXT: &str = "MainView && active_view == timeline && !ContextMenu";
const VIEW_NAVIGATION_KEY_CONTEXT: &str =
    "MainView && active_view == timeline && !ContextMenu && !TextInput && !TextArea && !Editor";

pub(super) fn init(cx: &mut App) {
    let view = Some(VIEW_KEY_CONTEXT);
    let navigation = Some(VIEW_NAVIGATION_KEY_CONTEXT);
    let idle = Some(IDLE_KEY_CONTEXT);

    cx.bind_keys([
        key(
            "escape",
            CloseMarkerPanel,
            Some("TimelineMarkers && !TextInput"),
        ),
        key(
            "escape",
            CloseTimelineBin,
            Some("TimelineBin && !TextInput && !TextArea && !Editor && !ContextMenu"),
        ),
        key("cmd-=", ZoomIn, view),
        key("cmd--", ZoomOut, view),
        key("cmd-0", ZoomReset, view),
        key("f", FitSelection, navigation),
        key("down", FocusItemDown, idle),
        key("up", FocusItemUp, idle),
        key("left", FocusItemLeft, idle),
        key("right", FocusItemRight, idle),
        key("shift-up", ExtendItemPrevious, idle),
        key("shift-left", ExtendItemPrevious, idle),
        key("shift-down", ExtendItemNext, idle),
        key("shift-right", ExtendItemNext, idle),
        key("j", FocusItemDown, idle),
        key("k", FocusItemUp, idle),
        key("h", FocusItemLeft, idle),
        key("l", FocusItemRight, idle),
        key("cmd-down", NextDivision, navigation),
        key("cmd-up", PreviousDivision, navigation),
        key("cmd-shift-down", NextOuterDivision, navigation),
        key("cmd-shift-up", PreviousOuterDivision, navigation),
    ]);
}

pub(super) struct TimelineView {
    pub(super) focus_handle: FocusHandle,
    toolbar_position: TimelineToolbarPosition,

    pixel_duration: ChronoDuration,
    now: DateTime<Local>,
    scroll_offset: Pixels,

    scroll_update: Option<DateTime<Local>>,
    scroll_target: Option<DateTime<Local>>,
    scroll_cancelled: bool,
    zoom_update: Option<ChronoDuration>,
    zoom_target: Option<ChronoDuration>,
    pending_fit: Option<(DateTime<Local>, DateTime<Local>)>,
    pending_item_focus: Option<uuid::Uuid>,
    hovered_divider: Option<DateTime<Local>>,
    zoom_anchor: Option<DateTime<Local>>,
    zoom_anchor_offset: Pixels,
    zoom_anchor_since: Option<std::time::Instant>,
    bounds: Option<Bounds<Pixels>>,
    bounds_changed: bool,
    item_y_scale: f32,
    item_y_offset: Pixels,
    item_geometry_now: DateTime<Local>,
    items: Vec<TimelineItem>,
    active_draft: Option<uuid::Uuid>,
    item_details: ItemCardStates,
    item_details_anchors: HashMap<uuid::Uuid, DetailsAnchor>,
    bin_details: ItemCardStates,
    bin_focus_handles: HashMap<u64, FocusHandle>,
    expanded_bin: Option<ExpandedBin>,
    marker_details: ItemCardStates,
    signal_details: ItemCardStates,

    recurring_work: Vec<AnyItem>,
    recurring_work_revision: u64,
    projected_work_key: Option<(NaiveDate, NaiveDate, u64)>,
    projected_work: Vec<AnyItem>,

    markers: Vec<Marker>,
    marker_panel_date: Option<NaiveDate>,
    marker_panel_draft: Option<uuid::Uuid>,
    marker_panel_scroll: ScrollHandle,
    marker_trigger_focus: FocusHandle,
    signals: Vec<Signal>,
    signal_focus_handles: HashMap<uuid::Uuid, FocusHandle>,
    draft_markers: Vec<Marker>,
    draft_signals: Vec<Signal>,
    annotations_revision: u64,
    annotation_cache: AnnotationCache,
    active_drop: Option<ActiveDropState>,
    drop_dragged: Option<DraggedItems>,
    active_resize: Option<ActiveResizeState>,
    force_create: Option<ForceCreateState>,
    force_press: Option<Point<Pixels>>,
    inspect_press_item: Option<uuid::Uuid>,
    inspect_force_item: Option<uuid::Uuid>,
    suppress_force_click: bool,
    marquee: MarqueeSelection,
    loaded: bool,

    drop_active: bool,

    edge_scroll_speed: Option<Pixels>,
}

impl TimelineView {
    fn animate_card_details(&mut self, window: &mut Window, cx: &mut App) {
        self.item_details.retain(
            self.items
                .iter()
                .filter(|entry| {
                    entry
                        .item
                        .content()
                        .is_some_and(|notes| !notes.trim().is_empty())
                })
                .map(|entry| entry.item.id()),
        );
        self.marker_details.retain(
            self.markers
                .iter()
                .chain(&self.draft_markers)
                .filter(|marker| {
                    marker
                        .content
                        .as_deref()
                        .is_some_and(|notes| !notes.trim().is_empty())
                })
                .map(|marker| marker.id),
        );
        self.signal_details.retain(
            self.signals
                .iter()
                .chain(&self.draft_signals)
                .filter(|signal| {
                    signal
                        .content
                        .as_deref()
                        .is_some_and(|notes| !notes.trim().is_empty())
                })
                .map(|signal| signal.id),
        );

        self.bin_details
            .retain(self.items.iter().map(|entry| entry.item.id()));
        self.bin_details.animate(window, cx);
        self.item_details.animate(window, cx);
        self.marker_details.animate(window, cx);
        self.signal_details.animate(window, cx);
    }

    pub(super) fn reset_pointer_intent(&mut self) {
        self.force_press = None;
        self.inspect_press_item = None;
        self.inspect_force_item = None;
        self.hovered_divider = None;
        self.active_drop = None;
        self.drop_dragged = None;
        self.drop_active = false;
        self.pending_item_focus = None;
        self.marker_panel_date = None;
        self.marker_panel_draft = None;
        self.expanded_bin = None;
        self.edge_scroll_speed = None;
    }

    pub(super) fn finish_item_drag(&mut self, cx: &mut Context<Self>) {
        self.active_drop = None;
        self.drop_dragged = None;
        self.drop_active = false;
        self.edge_scroll_speed = None;
        cx.notify();
    }

    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let toolbar_position = Settings::global(cx).timeline_toolbar_position;
        cx.observe_global::<GlobalSettings>(|view, cx| {
            let position = Settings::global(cx).timeline_toolbar_position;
            if view.toolbar_position != position {
                view.toolbar_position = position;
                view.bounds_changed = true;
                cx.notify();
            }
        })
        .detach();
        let pixel_duration = DEFAULT_PIXEL_DURATION;
        let now = Local::now();

        let refresh_interval = Duration::from_secs_f64(1.0 / REFRESH_RATE);
        cx.spawn(async move |view, cx: &mut AsyncApp| {
            loop {
                cx.background_executor().timer(refresh_interval).await;
                let result = view.update(cx, |view, cx| {
                    view.now = Local::now();
                    cx.notify();
                });
                if result.is_err() {
                    break;
                };
            }
        })
        .detach();

        let selection = SelectionManager::global(cx);
        cx.observe(&selection, |view, selection, cx| {
            if view.pending_item_focus.is_some_and(|id| {
                !selection
                    .read(cx)
                    .is_selected_in(SelectionScope::Timeline, id)
            }) {
                view.pending_item_focus = None;
            }
            cx.notify();
        })
        .detach();

        let item_manager = ItemManager::global(cx);
        cx.observe(&item_manager, |view, manager, cx| {
            if let Some(item) = view
                .active_draft
                .and_then(|id| manager.read(cx).draft_item(id))
                && let Some(entry) = view
                    .items
                    .iter_mut()
                    .find(|entry| entry.item.id() == item.id())
                && entry.item.item_type() != item.item_type()
            {
                entry.refresh(item.clone());
                view.items
                    .sort_by_key(|entry| entry.item.start().map(|start| start.timestamp()));
            }
            cx.notify();
        })
        .detach();
        cx.subscribe(&item_manager, |view, _, event: &DiscardDraft, cx| {
            let id = event.0;
            view.items.retain(|item| item.item.id() != id);
            if view.active_draft == Some(id) {
                view.active_draft = None;
            }
            view.draft_markers.retain(|marker| marker.id != id);
            view.draft_signals.retain(|signal| signal.id != id);
            view.signal_focus_handles.remove(&id);
            if view.pending_item_focus == Some(id) {
                view.pending_item_focus = None;
            }
            view.invalidate_annotations();
            cx.notify();
        })
        .detach();

        cx.subscribe_in(
            &item_manager,
            window,
            |view, _, event: &DraftSubmitted, window, cx| {
                if event.view == DraftView::Timeline {
                    view.continue_batch(&event.item, window, cx);
                }
            },
        )
        .detach();

        Self {
            focus_handle,
            toolbar_position,
            scroll_offset: px(0.),
            scroll_update: None,
            scroll_target: None,
            scroll_cancelled: false,
            zoom_update: None,
            zoom_target: None,
            pending_fit: None,
            pending_item_focus: None,
            hovered_divider: None,
            zoom_anchor: None,
            zoom_anchor_offset: px(0.),
            zoom_anchor_since: None,
            pixel_duration,
            now,

            drop_active: false,
            bounds: None,
            bounds_changed: false,
            item_y_scale: 1.0,
            item_y_offset: px(0.),
            item_geometry_now: now,
            items: vec![],
            active_draft: None,
            item_details: ItemCardStates::default(),
            item_details_anchors: HashMap::new(),
            bin_details: ItemCardStates::default(),
            bin_focus_handles: HashMap::new(),
            expanded_bin: None,
            marker_details: ItemCardStates::default(),
            signal_details: ItemCardStates::default(),

            recurring_work: vec![],
            recurring_work_revision: 0,
            projected_work_key: None,
            projected_work: vec![],
            markers: vec![],
            marker_panel_date: None,
            marker_panel_draft: None,
            marker_panel_scroll: ScrollHandle::new(),
            marker_trigger_focus: cx.focus_handle(),
            signals: vec![],
            signal_focus_handles: HashMap::new(),
            draft_markers: vec![],
            draft_signals: vec![],
            annotations_revision: 0,
            annotation_cache: AnnotationCache::default(),
            active_drop: None,
            drop_dragged: None,
            active_resize: None,
            force_create: None,
            force_press: None,
            inspect_press_item: None,
            inspect_force_item: None,
            suppress_force_click: false,
            marquee: MarqueeSelection::new(SelectionScope::Timeline),
            loaded: false,
            edge_scroll_speed: None,
        }
    }

    pub fn refresh_items(&mut self, queue: Vec<AnyItem>, cx: &mut Context<Self>) {
        let scheduled: Vec<AnyItem> = queue
            .into_iter()
            .filter(|item| item.start().is_some() && !item.is_completed())
            .collect();

        let draft_id = ItemManager::global(cx)
            .read(cx)
            .editing_item
            .as_ref()
            .and_then(|e| e.is_draft().then_some(e.id()));

        self.recurring_work = scheduled
            .iter()
            .filter(|item| {
                item.recurrence().is_some() && item.start_precision() == StartPrecision::DateTime
            })
            .cloned()
            .collect();
        self.recurring_work_revision = self.recurring_work_revision.wrapping_add(1);
        self.projected_work_key = None;

        sync_timeline_items(&mut self.items, scheduled, draft_id, cx);
        self.items
            .sort_by_key(|entry| entry.item.start().map(|start| start.timestamp()));
        if self.pending_item_focus.is_some_and(|id| {
            !self.items.iter().any(|entry| entry.item.id() == id)
                && !self.signal_focus_handles.contains_key(&id)
        }) {
            self.pending_item_focus = None;
        }

        if !self.loaded {
            self.loaded = true;
        }

        cx.notify();
    }

    fn sync_projected_work(&mut self) {
        let (start, end) = self.drawn_range();
        let key = (
            start.date_naive(),
            end.date_naive(),
            self.recurring_work_revision,
        );
        if self.projected_work_key == Some(key) {
            return;
        }

        self.projected_work = if end - start > MAX_PROJECTED_WORK_RANGE {
            Vec::new()
        } else {
            let center = self.scroll_position().timestamp();
            let mut projected =
                subroutine_core::projected_items_between(&self.recurring_work, key.0, key.1);
            projected.retain(|item| item.start_datetime().is_some());
            projected.sort_by_key(|item| {
                item.start()
                    .map(|point| point.timestamp().abs_diff(center))
                    .unwrap_or(u64::MAX)
            });
            projected.truncate(MAX_PROJECTED_WORK_ITEMS);
            projected.sort_by_key(|item| (item.start().map(|point| point.timestamp()), item.id()));
            projected
        };
        self.projected_work_key = Some(key);
    }

    pub fn drop_zone(&self) -> DropZone<DragData<DraggedItems>> {
        DropZone::new("timeline-drop")
            .size_full()
            .active(self.drop_active)
            .rounded_none()
            .rounded_bl_2xl()
    }
}

impl Focusable for TimelineView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl MarqueeView for TimelineView {
    fn marquee(&self) -> &MarqueeSelection {
        &self.marquee
    }

    fn marquee_mut(&mut self) -> &mut MarqueeSelection {
        &mut self.marquee
    }

    fn marquee_enabled(&self, _cx: &App) -> bool {
        self.active_resize.is_none() && self.force_create.is_none()
    }

    fn marquee_focus(&self, _cx: &App) -> Option<FocusHandle> {
        Some(self.focus_handle.clone())
    }

    fn marquee_dragged(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(bounds) = self.bounds else {
            return;
        };
        self.edge_scroll_speed = if self.marquee.has_scroll_region() {
            None
        } else {
            compute_edge_scroll_speed(position.y - bounds.origin.y, bounds.size.height)
        };
        cx.notify();
    }

    fn marquee_ended(&mut self, cx: &mut Context<Self>) {
        self.edge_scroll_speed = None;
        cx.notify();
    }
}

impl Render for TimelineView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.animate_card_details(window, cx);
        self.marquee.scroll_region(window);
        let entity = cx.entity();
        let hook_entity = entity.clone();

        let body = self
            .drop_zone()
            .absolute()
            .inset_0()
            .capture_any_mouse_down(cx.listener(|view, event: &MouseDownEvent, _window, cx| {
                view.inspect_force_item = None;
                if event.button == MouseButton::Left {
                    view.begin_force_press(event.position, cx);
                } else {
                    view.force_press = None;
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, event: &MouseDownEvent, _, cx| {
                    if force_click_modifier(&event.modifiers) {
                        view.pressure_changed(gpui::PressureStage::Force, event.position, cx);
                    }
                }),
            )
            .on_prepaint(move |bounds, _, cx| {
                entity.update(cx, |view, cx| {
                    if view.bounds != Some(bounds) {
                        view.bounds = Some(bounds);
                        view.bounds_changed = true;
                        cx.notify();
                    } else if view.bounds_changed {
                        view.bounds_changed = false;
                    }
                    if let Some((start, end)) = view.pending_fit.take() {
                        view.zoom_to_span(start, end, cx);
                    }
                });
            })
            .flex()
            .flex_1()
            .overflow_hidden()
            .on_scroll_wheel(
                cx.listener(|view, event, window, cx| view.handle_scroll_wheel(event, window, cx)),
            )
            .on_pinch(cx.listener(|view, event, _window, cx| view.handle_pinch(event, cx)))
            .on_drag_move(cx.listener(
                |this, event: &DragMoveEvent<DragData<DraggedItems>>, window, cx| {
                    let position = window.mouse_position();
                    let is_over = event.bounds.contains(&position)
                        && position.y >= event.bounds.origin.y + TOP_EDGE_INSET;
                    if is_over != this.drop_active {
                        this.drop_active = is_over;
                        cx.notify();
                    }
                    this.handle_drag_move(event, window, cx);
                },
            ))
            .on_drag_move(
                cx.listener(|this, event: &DragMoveEvent<ResizeDragData>, window, cx| {
                    this.handle_resize_move(event, window, cx);
                }),
            )
            .on_drop(
                cx.listener(|this, data: &DragData<DraggedItems>, window, cx| {
                    this.handle_drop(data, window, cx);
                    this.drop_active = false;
                    cx.notify();
                }),
            )
            .child(self.render_backdrop(window, cx))
            .child(self.render_now_cursor(cx))
            .when(self.loaded, |this| {
                this.children(self.render_signal_ticks(window, cx))
            })
            .when(self.loaded && self.should_render_items(), |this| {
                this.children(self.render_timeline_cards(window, cx))
            })
            .when(!self.loaded && self.should_render_items(), |this| {
                this.children(self.render_skeleton_items())
            })
            .when(self.active_resize.is_some(), |this| {
                this.child(ResizeDragMouseUpHook(hook_entity.clone()))
            })
            .child(force_create_hook(hook_entity.clone()))
            .children(self.render_sticky_outer_label(cx))
            .map(|this| {
                let markers = self.render_marker_header(window, cx);
                let shortcut_top = TOP_EDGE_INSET
                    + if markers.is_some() {
                        STICKY_OUTER_LABEL_HEIGHT + STICKY_OUTER_LABEL_GAP
                    } else {
                        STICKY_OUTER_LABEL_INSET
                    };
                this.children(self.render_now_shortcut(shortcut_top, cx))
                    .children(markers)
            });
        let body = marquee(body, self, cx);

        self.tab_root(div().column(), cx)
            .on_action(cx.listener(|this, _: &FocusItemDown, window, cx| {
                if !this.navigate_items(ItemNavigation::Next, false, window, cx) {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &FocusItemRight, window, cx| {
                if !this.navigate_items(ItemNavigation::Next, false, window, cx) {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &FocusItemUp, window, cx| {
                if !this.navigate_items(ItemNavigation::Previous, false, window, cx) {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &FocusItemLeft, window, cx| {
                if !this.navigate_items(ItemNavigation::Previous, false, window, cx) {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &ExtendItemPrevious, window, cx| {
                if !this.navigate_items(ItemNavigation::Previous, true, window, cx) {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &ExtendItemNext, window, cx| {
                if !this.navigate_items(ItemNavigation::Next, true, window, cx) {
                    cx.propagate();
                }
            }))
            .size_full()
            .items_center()
            .child(body)
            .child(self.render_timeline_toolbar(cx))
    }
}

impl EventEmitter<DeleteItem> for TimelineView {}
impl EventEmitter<InspectItem> for TimelineView {}

impl MainViewTab for TimelineView {
    const TAB: SelectedMainView = SelectedMainView::Timeline;

    fn scope() -> Option<SelectionScope> {
        Some(SelectionScope::Timeline)
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.expanded_bin.is_some() {
            self.close_bin(window, cx);
            return true;
        }
        let closed = self.item_details.collapse_all();
        if closed {
            cx.notify();
        }
        closed
    }

    fn go_to_now(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.scroll_reset(cx);
    }

    fn bind_view_actions<E: InteractiveElement>(&self, element: E, cx: &mut Context<Self>) -> E {
        element
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.zoom_in(cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.zoom_out(cx)))
            .on_action(cx.listener(|this, _: &ZoomReset, _, cx| this.zoom_reset(cx)))
            .on_action(cx.listener(|this, _: &FitSelection, _, cx| this.zoom_to_selection(cx)))
            .on_action(
                cx.listener(|this, _: &NextSubdivision, _, cx| this.scroll_next_subdivision(cx)),
            )
            .on_action(cx.listener(|this, _: &PreivousSubdivision, _, cx| {
                this.scroll_previous_subdivision(cx)
            }))
            .on_action(cx.listener(|this, _: &NextDivision, _, cx| this.scroll_next_division(cx)))
            .on_action(
                cx.listener(|this, _: &PreviousDivision, _, cx| this.scroll_previous_division(cx)),
            )
            .on_action(
                cx.listener(|this, _: &NextOuterDivision, _, cx| {
                    this.scroll_next_outer_division(cx)
                }),
            )
            .on_action(cx.listener(|this, _: &PreviousOuterDivision, _, cx| {
                this.scroll_previous_outer_division(cx)
            }))
    }
}

fn sync_timeline_items(
    entries: &mut Vec<TimelineItem>,
    incoming: Vec<AnyItem>,
    draft_id: Option<uuid::Uuid>,
    cx: &App,
) {
    let incoming_ids: HashSet<u64> = incoming.iter().map(|item| item.id_u64()).collect();
    entries.retain(|entry| {
        incoming_ids.contains(&entry.item.id_u64()) || draft_id == Some(entry.item.id())
    });

    for item in incoming {
        match entries
            .iter_mut()
            .find(|entry| entry.item.id_u64() == item.id_u64())
        {
            Some(existing) => existing.refresh(item),
            None => entries.push(TimelineItem::new(item, cx)),
        }
    }
}
