use crate::components::transition::{self, WindowTransitionExt as _};
use gpui_kit::foundation::StyledExt as _;
use std::{collections::HashMap, time::Duration};

use crate::components::CloseOverlay;
use crate::components::ext::ElementExt;
use crate::components::menu::MenuBuilder;

use crate::components::{Button, ButtonVariants, Label};
use chrono::{DateTime, Datelike, Local, Month, NaiveDate, NaiveWeek, Weekday};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use gpui::AnyElement;
use gpui::{
    App, AppContext, AsyncApp, Bounds, Context, DragMoveEvent, Entity, EventEmitter, FocusHandle,
    Focusable, FontWeight, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, MousePressureEvent, MouseUpEvent, ParentElement, Pixels, PressureStage, Render,
    ScrollDelta, ScrollWheelEvent, StatefulInteractiveElement, Styled, Task, Window, actions, div,
    ease_in_out, prelude::FluentBuilder, px, relative,
};
use gpui_kit::foundation::{Sizable, ThemeOverlay};
use gpui_kit::layout::ScrollFade;
use gpui_kit::overlay::{GlassExt as _, GlassPreset};
use gpui_kit_theme::ActiveTheme;
use gpui_kit_theme::{Radius, Surface};
use subroutine_core::{Action, AnyItem, ItemType, Marker, SchedulePoint, Signal};

mod grid;
mod inspector_list;

use inspector_list::{CalendarInspectorDelegate, CalendarInspectorEntry};

use crate::{
    AppIcon,
    components::{
        DragData, DraggedItems, DropZone, DynamicList, DynamicListEvent, DynamicListState,
        MarqueeSelection, MarqueeView, MonthSelect, MonthSelectEvent, MonthSelectState,
        SIDEBAR_ITEM_GAP, marquee,
    },
    dates::InclusiveDateRange,
    item_manager::{DiscardDraft, ItemManager},
    keys::{force_click_modifier, key},
    selection::{
        SelectionManager, SelectionOrder, SelectionScope, focus_item, focus_item_extending,
    },
    stores::AppDatabaseStore,
    views::{
        StartItemCreatorOnDate, StartMarkerCreatorOnRange,
        drop_confirmation::{confirm_drop, resolve_dragged, scheduled_item_count},
    },
};

use super::{
    InspectItem, action_start_on_date, item_inspection_matches_press, local_time_on,
    tab::{MainViewTab, SelectedMainView},
};

fn action_dropped_on_date(action: Action, date: NaiveDate) -> Action {
    let start = action_start_on_date(&action, date);
    action.with_queued(true).with_start(Some(start))
}
const REFRESH_RATE: f64 = 1.0;
const AUTOSCROLL_REFRESH_RATE: f64 = 25.0;
const MIN_WEEK_HEIGHT: Pixels = px(88.);
const MAX_WEEK_HEIGHT: Pixels = px(160.);
pub(super) const DAY_NAMES_HEIGHT: Pixels = px(28.);
const DAY_NAMES_TOP: Pixels = if cfg!(any(target_os = "macos", target_os = "windows")) {
    crate::views::TOP_EDGE_INSET
} else {
    px(0.)
};

const MAX_CALENDAR_WIDTH: Pixels = px(1440.);
const DAY_INSPECTOR_MIN_WIDTH: Pixels = px(360.);
const DAY_INSPECTOR_MAX_WIDTH: Pixels = px(520.);
const DAY_INSPECTOR_MIN_HEIGHT: Pixels = px(240.);
const DAY_INSPECTOR_MAX_HEIGHT: Pixels = px(520.);
const DAY_INSPECTOR_HEADER_HEIGHT: Pixels = px(48.);
const DAY_INSPECTOR_ENTRIES_PADDING: Pixels = px(12.);
const DAY_INSPECTOR_BORDER_WIDTH: Pixels = px(1.);
const DAY_INSPECTOR_VIEWPORT_INSET: Pixels = px(32.);
const EDGE_SCROLL_ZONE: Pixels = px(96.);
const EDGE_SCROLL_MAX_SPEED: f32 = 22.;

fn edge_scroll_speed(local_y: Pixels, height: Pixels, visible_top: Pixels) -> Option<Pixels> {
    let zone = ((height - visible_top) / 2.).min(EDGE_SCROLL_ZONE);
    if zone <= px(0.) || local_y < visible_top {
        None
    } else if local_y < visible_top + zone {
        let t = 1. - ((local_y - visible_top) / zone).clamp(0., 1.);
        Some(px(t * t * EDGE_SCROLL_MAX_SPEED))
    } else if local_y > height - zone {
        let t = 1. - ((height - local_y) / zone).clamp(0., 1.);
        Some(px(-(t * t * EDGE_SCROLL_MAX_SPEED)))
    } else {
        None
    }
}

pub(super) const IDLE_KEY_CONTEXT: &str =
    "CalendarView && !TextInput && !TextArea && !Editor && !Overlay";

actions!(
    calendar,
    [
        OpenSelectedRange,
        MoveDateSelectionLeft,
        MoveDateSelectionRight,
        MoveDateSelectionUp,
        MoveDateSelectionDown,
        ExtendDateSelectionLeft,
        ExtendDateSelectionRight,
        ExtendDateSelectionUp,
        ExtendDateSelectionDown,
    ]
);

pub(super) fn init(cx: &mut App) {
    let idle = Some(IDLE_KEY_CONTEXT);
    cx.bind_keys([
        key("enter", OpenSelectedRange, idle),
        key("left", MoveDateSelectionLeft, idle),
        key("right", MoveDateSelectionRight, idle),
        key("up", MoveDateSelectionUp, idle),
        key("down", MoveDateSelectionDown, idle),
        key("shift-left", ExtendDateSelectionLeft, idle),
        key("shift-right", ExtendDateSelectionRight, idle),
        key("shift-up", ExtendDateSelectionUp, idle),
        key("shift-down", ExtendDateSelectionDown, idle),
    ]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DateSelectionCursor {
    anchor: NaiveDate,
    head: NaiveDate,
}

impl DateSelectionCursor {
    fn single(date: NaiveDate) -> Self {
        Self {
            anchor: date,
            head: date,
        }
    }

    fn range(self) -> InclusiveDateRange {
        InclusiveDateRange::new(self.anchor, self.head)
    }

    fn shifted(self, days: i64, extend: bool) -> Option<Self> {
        let delta = chrono::Duration::days(days);
        if extend {
            Some(Self {
                anchor: self.anchor,
                head: self.head.checked_add_signed(delta)?,
            })
        } else {
            Some(Self {
                anchor: self.anchor.checked_add_signed(delta)?,
                head: self.head.checked_add_signed(delta)?,
            })
        }
    }
}

fn scroll_weeks_to_reveal_row(
    row_top: Pixels,
    row_height: Pixels,
    visible_top: Pixels,
    visible_bottom: Pixels,
    current_weeks: f32,
) -> Option<f32> {
    let visible_row_height = row_height.min((visible_bottom - visible_top).max(px(0.)));
    let delta = if row_top < visible_top {
        visible_top - row_top
    } else if row_top + visible_row_height > visible_bottom {
        visible_bottom - (row_top + visible_row_height)
    } else {
        return None;
    };
    Some(current_weeks + delta / row_height)
}

#[derive(Clone, Copy)]
pub(super) struct OpenDateRangeInTimeline(pub InclusiveDateRange);

#[derive(Clone, Copy, Debug)]
pub(super) struct DateRangeDrag {
    anchor: NaiveDate,
    current: NaiveDate,
    origin: gpui::Point<Pixels>,
    engaged: bool,
}

impl DateRangeDrag {
    const THRESHOLD: Pixels = px(4.);

    fn new(anchor: NaiveDate, origin: gpui::Point<Pixels>) -> Self {
        Self {
            anchor,
            current: anchor,
            origin,
            engaged: false,
        }
    }

    fn engage(&mut self, position: gpui::Point<Pixels>) -> bool {
        if !self.engaged {
            let dx = (position.x - self.origin.x).abs();
            let dy = (position.y - self.origin.y).abs();
            self.engaged = dx >= Self::THRESHOLD || dy >= Self::THRESHOLD;
        }
        self.engaged
    }
}

pub(super) fn date_range_context_menu(
    range: InclusiveDateRange,
    view: Entity<CalendarView>,
) -> MenuBuilder {
    let label = format!(
        "{} – {} · {} days",
        range.start().format("%b %-d, %Y"),
        range.end().format("%b %-d, %Y"),
        range.day_count()
    );
    let inspect = view.clone();
    let timeline = view.clone();
    let clear = view;

    MenuBuilder::new()
        .label(label)
        .item("Inspect date range", move |window, cx| {
            inspect.update(cx, |view, cx| {
                view.open_range_inspector(range, range.start(), window, cx)
            });
        })
        .item("New marker for range", move |window, cx| {
            window.dispatch_action(Box::new(StartMarkerCreatorOnRange(range)), cx);
        })
        .separator()
        .item("View range in Timeline", move |window, cx| {
            timeline.update(cx, |view, cx| {
                view.close_day_inspector(window, cx);
                cx.emit(OpenDateRangeInTimeline(range));
                cx.notify();
            });
        })
        .separator()
        .item("Clear date selection", move |_window, cx| {
            clear.update(cx, |view, cx| {
                view.clear_date_selection();
                cx.notify();
            });
        })
}

pub(super) fn date_context_menu(
    date: NaiveDate,
    expanded: bool,
    view: Entity<CalendarView>,
) -> MenuBuilder {
    let label = date.format("%A, %B %-d").to_string();
    let open = view.clone();
    let timeline = view.clone();
    let close = view;

    MenuBuilder::new()
        .label(label)
        .when(!expanded, |menu| {
            menu.item("Open day", move |window, cx| {
                open.update(cx, |view, cx| view.open_day_inspector(date, window, cx));
            })
            .separator()
        })
        .submenu("New item", move |menu| {
            menu.item("Action", move |window, cx| {
                window
                    .dispatch_action(Box::new(StartItemCreatorOnDate(ItemType::Action, date)), cx);
            })
            .item("Event", move |window, cx| {
                window.dispatch_action(Box::new(StartItemCreatorOnDate(ItemType::Event, date)), cx);
            })
            .item("Routine", move |window, cx| {
                window.dispatch_action(
                    Box::new(StartItemCreatorOnDate(ItemType::Routine, date)),
                    cx,
                );
            })
            .item("Marker", move |window, cx| {
                window
                    .dispatch_action(Box::new(StartItemCreatorOnDate(ItemType::Marker, date)), cx);
            })
            .item("Signal", move |window, cx| {
                window
                    .dispatch_action(Box::new(StartItemCreatorOnDate(ItemType::Signal, date)), cx);
            })
        })
        .separator()
        .item("View date in Timeline", move |window, cx| {
            timeline.update(cx, |view, cx| {
                view.close_day_inspector(window, cx);
                cx.emit(OpenDateRangeInTimeline(InclusiveDateRange::single(date)));
                cx.notify();
            });
        })
        .when(expanded, |menu| {
            menu.separator().item("Close day", move |window, cx| {
                close.update(cx, |view, cx| view.close_day_inspector(window, cx));
            })
        })
}

pub struct CalendarView {
    pub(super) focus_handle: FocusHandle,
    items: Vec<AnyItem>,
    draft_items: Vec<AnyItem>,
    markers: Vec<Marker>,
    signals: Vec<Signal>,
    draft_markers: Vec<Marker>,
    current_date: NaiveDate,
    current_weekday: u32,
    loaded: bool,
    drop_target: Option<NaiveDate>,
    item_drag_active: bool,
    scroll_offset: Pixels,

    week_height: Pixels,
    bounds: Option<Bounds<Pixels>>,
    has_horizontal_gutters: bool,
    month_selector_open: bool,
    month_select: Entity<MonthSelectState>,
    inspector_date: Option<NaiveDate>,
    inspector_range: Option<InclusiveDateRange>,
    selected_date_range: Option<InclusiveDateRange>,
    date_selection_cursor: Option<DateSelectionCursor>,
    date_range_drag: Option<DateRangeDrag>,
    suppress_date_click: bool,
    force_click_opened: bool,
    inspector_item_press: Option<uuid::Uuid>,
    inspector_item_opened: Option<uuid::Uuid>,
    inspector_focus: FocusHandle,
    inspector_list: Entity<DynamicListState<CalendarInspectorDelegate>>,
    inspector_item_focus_handles: HashMap<uuid::Uuid, FocusHandle>,
    inspector_bounds: Option<Bounds<Pixels>>,
    inspector_expands_up: bool,
    inspector_expands_left: bool,
    month_in_view: NaiveDate,
    scroll_update: Option<f32>,
    scroll_target: Option<f32>,
    scroll_cancelled: bool,
    marquee: MarqueeSelection,
    edge_scroll_speed: Option<Pixels>,
    edge_scroll_task: Option<Task<()>>,
}

impl CalendarView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let now = Local::now();

        let refresh_interval = Duration::from_secs_f64(1.0 / REFRESH_RATE);
        cx.spawn(async move |view, cx: &mut AsyncApp| {
            loop {
                cx.background_executor().timer(refresh_interval).await;
                let result = view.update(cx, |view, cx| {
                    let now = Local::now();
                    view.current_date = now.date_naive();
                    view.current_weekday = now.weekday().num_days_from_sunday();
                    cx.notify();
                });
                if result.is_err() {
                    break;
                };
            }
        })
        .detach();

        let month_select = cx.new(|cx| MonthSelectState::new(window, cx));
        let inspector_focus = cx.focus_handle();
        cx.on_focus_out(&inspector_focus, window, |view, _, window, cx| {
            if view.item_drag_active
                || cx.has_active_drag()
                || crate::components::menu::context_menu_has_focus(window, cx)
            {
                return;
            }
            view.close_day_inspector(window, cx);
        })
        .detach();
        let inspector_view = cx.entity().downgrade();
        let inspector_list = cx.new(|cx| {
            DynamicListState::new(
                CalendarInspectorDelegate::new(inspector_view, cx),
                window,
                cx,
            )
            .gap(SIDEBAR_ITEM_GAP)
        });
        cx.subscribe(&inspector_list, |view, _, event: &DynamicListEvent, cx| {
            if let DynamicListEvent::LayoutChanged { scroll_delta } = event {
                view.marquee.scrolled_by(gpui::point(px(0.), *scroll_delta));
                cx.notify();
            }
        })
        .detach();

        let item_manager = ItemManager::global(cx);
        cx.observe(&item_manager, |view, manager, cx| {
            let mut changed = false;
            for draft in &mut view.draft_items {
                if let Some(item) = manager.read(cx).draft_item(draft.id())
                    && item.item_type() != draft.item_type()
                {
                    *draft = item.clone();
                    changed = true;
                }
            }
            if changed && let Some(range) = view.inspector_range {
                view.sync_inspector_list(range, cx);
            }
            cx.notify();
        })
        .detach();
        cx.subscribe(&item_manager, |view, _, event: &DiscardDraft, cx| {
            view.draft_markers.retain(|marker| marker.id != event.0);
            view.draft_items.retain(|item| item.id() != event.0);
            if let Some(range) = view.inspector_range {
                view.sync_inspector_list(range, cx);
            }
            cx.notify();
        })
        .detach();

        cx.subscribe(
            &month_select,
            |view, select, event: &MonthSelectEvent, cx| match event {
                MonthSelectEvent::Selected(month) => {
                    let year = select.read(cx).current_year();
                    view.scroll_to_month(year, *month, cx);
                    view.month_selector_open = false;
                    cx.notify();
                }
                MonthSelectEvent::Today => {
                    view.scroll_to_today(cx);
                    view.month_selector_open = false;
                    cx.notify();
                }
            },
        )
        .detach();

        Self {
            focus_handle: cx.focus_handle(),
            items: Vec::new(),
            draft_items: Vec::new(),
            markers: Vec::new(),
            signals: Vec::new(),
            draft_markers: Vec::new(),
            current_date: now.date_naive(),
            current_weekday: now.weekday().num_days_from_sunday(),
            loaded: false,
            drop_target: None,
            item_drag_active: false,
            scroll_offset: px(0.),

            week_height: MAX_WEEK_HEIGHT,
            bounds: None,
            has_horizontal_gutters: false,
            month_selector_open: false,
            month_select,
            inspector_date: None,
            inspector_range: None,
            selected_date_range: None,
            date_selection_cursor: None,
            date_range_drag: None,
            suppress_date_click: false,
            force_click_opened: false,
            inspector_item_press: None,
            inspector_item_opened: None,
            inspector_focus,
            inspector_list,
            inspector_item_focus_handles: HashMap::new(),
            inspector_bounds: None,
            inspector_expands_up: false,
            inspector_expands_left: false,
            month_in_view: now
                .date_naive()
                .with_day(1)
                .unwrap_or_else(|| now.date_naive()),
            scroll_update: None,
            scroll_target: None,
            scroll_cancelled: false,
            marquee: MarqueeSelection::new(SelectionScope::Calendar),
            edge_scroll_speed: None,
            edge_scroll_task: None,
        }
    }

    fn set_edge_scroll_speed(&mut self, speed: Option<Pixels>, cx: &mut Context<Self>) -> bool {
        if self.edge_scroll_speed == speed {
            return false;
        }

        self.edge_scroll_speed = speed;
        if speed.is_some() {
            if self.edge_scroll_task.is_none() {
                let interval = Duration::from_secs_f64(1.0 / AUTOSCROLL_REFRESH_RATE);
                self.edge_scroll_task = Some(cx.spawn(async move |view, cx: &mut AsyncApp| {
                    loop {
                        cx.background_executor().timer(interval).await;
                        if view.update(cx, |_, cx| cx.notify()).is_err() {
                            break;
                        }
                    }
                }));
            }
        } else {
            self.edge_scroll_task = None;
        }
        cx.notify();
        true
    }

    pub fn refresh_items(&mut self, queue: Vec<AnyItem>, cx: &mut Context<Self>) {
        self.items = queue;
        self.items.sort_by_key(|i| i.start().map(|t| t.timestamp()));
        self.draft_items
            .retain(|draft| !self.items.iter().any(|item| item.id() == draft.id()));
        if let Some(range) = self.inspector_range {
            self.sync_inspector_list(range, cx);
        }

        if !self.loaded {
            self.loaded = true;
        }
        cx.notify();
    }

    pub fn refresh_markers(&mut self, markers: Vec<Marker>, cx: &mut Context<Self>) {
        self.markers = markers;
        self.draft_markers
            .retain(|draft| !self.markers.iter().any(|marker| marker.id == draft.id));
        if let Some(range) = self.inspector_range {
            self.sync_inspector_list(range, cx);
        }
        cx.notify();
    }

    pub fn refresh_signals(&mut self, signals: Vec<Signal>, cx: &mut Context<Self>) {
        self.signals = signals;
        if let Some(range) = self.inspector_range {
            self.sync_inspector_list(range, cx);
        }
        cx.notify();
    }

    fn stored_markers(&self) -> impl Iterator<Item = &Marker> {
        self.markers.iter().chain(self.draft_markers.iter())
    }

    pub(super) fn projected_markers(&self, start: NaiveDate, end: NaiveDate) -> Vec<Marker> {
        let mut projected: Vec<Marker> = self
            .stored_markers()
            .flat_map(|marker| marker.projections_between(start, end))
            .collect();

        projected.retain(|ghost| {
            !self
                .stored_markers()
                .any(|m| m.lineage_id == ghost.lineage_id && m.date == ghost.date)
        });
        projected.sort_by_key(|m| (m.date, m.lineage_id));
        projected.dedup_by_key(|m| (m.date, m.lineage_id));
        projected
    }

    pub(super) fn projected_items(&self, start: NaiveDate, end: NaiveDate) -> Vec<AnyItem> {
        subroutine_core::projected_items_between(&self.items, start, end)
    }

    pub(super) fn projected_signals(&self, start: NaiveDate, end: NaiveDate) -> Vec<Signal> {
        let start: DateTime<Local> = SchedulePoint::Date(start).into();
        let next = end.succ_opt().unwrap_or(end);
        let end: DateTime<Local> = SchedulePoint::Date(next).into();
        let (start, end) = (
            start.to_utc(),
            end.to_utc() - chrono::Duration::nanoseconds(1),
        );

        let mut projected: Vec<Signal> = self
            .signals
            .iter()
            .flat_map(|signal| signal.projections_between(start, end))
            .filter(|ghost| {
                !self.signals.iter().any(|stored| {
                    stored.lineage_id == ghost.lineage_id && stored.datetime == ghost.datetime
                })
            })
            .collect();
        projected.sort_by_key(|signal| (signal.datetime, signal.lineage_id));
        projected.dedup_by_key(|signal| (signal.datetime, signal.lineage_id));
        projected
    }

    pub(super) fn open_day_inspector(
        &mut self,
        date: NaiveDate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_range_inspector(InclusiveDateRange::single(date), date, window, cx);
    }

    pub(super) fn open_range_inspector(
        &mut self,
        range: InclusiveDateRange,
        anchor: NaiveDate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.inspector_range != Some(range) {
            self.inspector_list.update(cx, |list, cx| {
                list.reset_items(cx, |delegate, _| delegate.clear());
            });
        }
        self.inspector_date = Some(anchor);
        self.inspector_range = Some(range);
        if self.selected_date_range != Some(range) || self.date_selection_cursor.is_none() {
            self.set_date_selection(range.start(), range.end());
        }
        self.sync_inspector_list(range, cx);
        if let (Some(cell), Some(viewport)) = (self.day_bounds_for(anchor), self.bounds) {
            self.inspector_expands_up = cell.origin.y + cell.size.height / 2.
                > viewport.origin.y + viewport.size.height / 2.;
            self.inspector_expands_left =
                cell.origin.x + cell.size.width / 2. > viewport.origin.x + viewport.size.width / 2.;
        }
        self.inspector_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn begin_date_range_drag(
        &mut self,
        date: NaiveDate,
        position: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.month_selector_open = false;
        self.cancel_scroll();
        self.set_edge_scroll_speed(None, cx);
        if self.inspector_date.is_some() {
            self.close_day_inspector(window, cx);
        }
        self.date_range_drag = Some(DateRangeDrag::new(date, position));
        self.suppress_date_click = false;
    }

    pub(super) fn drag_date_range_to(
        &mut self,
        date: NaiveDate,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.date_range_drag.as_mut() else {
            return;
        };
        if drag.engage(position) {
            drag.current = date;
            let anchor = drag.anchor;
            self.set_date_selection(anchor, date);
            let speed = self.edge_scroll_speed_at(position);
            self.set_edge_scroll_speed(speed, cx);
            cx.notify();
        }
    }

    pub(super) fn finish_date_range_drag(&mut self, date: NaiveDate, cx: &mut Context<Self>) {
        let was_scrolling = self.edge_scroll_speed.is_some();
        self.set_edge_scroll_speed(None, cx);
        if let Some(mut drag) = self.date_range_drag.take()
            && drag.engaged
        {
            drag.current = date;
            self.set_date_selection(drag.anchor, drag.current);
            self.suppress_date_click = true;
            cx.spawn(async move |view, cx: &mut AsyncApp| {
                cx.background_executor()
                    .timer(Duration::from_millis(1))
                    .await;
                let _ = view.update(cx, |view, _| view.suppress_date_click = false);
            })
            .detach();
            cx.notify();
        } else if was_scrolling {
            cx.notify();
        }
    }

    fn finish_date_range_drag_at(&mut self, position: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let date = self
            .date_at_position(position)
            .or_else(|| self.date_range_drag.map(|drag| drag.current));
        if let Some(date) = date {
            self.finish_date_range_drag(date, cx);
        } else {
            self.date_range_drag = None;
            self.set_edge_scroll_speed(None, cx);
        }
    }

    pub(super) fn date_clicked(
        &mut self,
        date: NaiveDate,
        click_count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.date_range_drag = None;
        self.set_edge_scroll_speed(None, cx);
        if self.suppress_date_click {
            self.suppress_date_click = false;
            return;
        }
        let range = InclusiveDateRange::single(date);
        self.set_date_selection(date, date);
        if click_count >= 2 {
            self.open_range_inspector(range, date, window, cx);
            self.add_draft_marker(range, window, cx);
        } else {
            self.focus_handle.focus(window, cx);
            cx.notify();
        }
    }

    pub(super) fn force_clicked_date(
        &mut self,
        date: NaiveDate,
        stage: gpui::PressureStage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if stage != gpui::PressureStage::Force {
            if self.force_click_opened {
                cx.spawn(async move |view, cx: &mut AsyncApp| {
                    cx.background_executor()
                        .timer(Duration::from_millis(1))
                        .await;
                    let _ = view.update(cx, |view, _| view.suppress_date_click = false);
                })
                .detach();
            }
            self.force_click_opened = false;
            return;
        }
        if self.force_click_opened {
            return;
        }
        self.force_click_opened = true;
        self.suppress_date_click = true;
        let range = self
            .selected_date_range
            .filter(|range| range.contains(date))
            .unwrap_or_else(|| InclusiveDateRange::single(date));
        if self.selected_date_range != Some(range) || self.date_selection_cursor.is_none() {
            self.set_date_selection(range.start(), range.end());
        }
        self.close_day_inspector(window, cx);
        cx.emit(OpenDateRangeInTimeline(range));
        cx.notify();

        cx.spawn(async move |view, cx: &mut AsyncApp| {
            cx.background_executor()
                .timer(Duration::from_millis(1))
                .await;
            let _ = view.update(cx, |view, _| {
                view.force_click_opened = false;
                view.suppress_date_click = false;
            });
        })
        .detach();
    }

    fn date_at_position(&self, position: gpui::Point<Pixels>) -> Option<NaiveDate> {
        let viewport = self.bounds?;
        let local = viewport.localize(&position)?;
        if local.y < self.visible_top() {
            return None;
        }

        let day_width = viewport.size.width / 7.;
        if day_width <= px(0.) {
            return None;
        }
        let weekday = ((local.x / day_width).floor() as i64).clamp(0, 6);
        let horizon = viewport.size.height / 2. + grid::EDGE_HORIZON;
        let week = self.offset_to_week(local.y - self.scroll_offset - horizon, self.week_height);
        Some(week.first_day() + chrono::Duration::days(weekday))
    }

    fn edge_scroll_speed_at(&self, position: gpui::Point<Pixels>) -> Option<Pixels> {
        let viewport = self.bounds?;
        let local = viewport.localize(&position)?;
        edge_scroll_speed(local.y, viewport.size.height, self.visible_top())
    }

    fn retarget_edge_drag(&mut self, position: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let date = self.date_at_position(position);
        let mut changed = false;

        if let (Some(date), Some(drag)) = (date, self.date_range_drag.as_mut())
            && drag.engaged
        {
            drag.current = date;
            let range = InclusiveDateRange::new(drag.anchor, drag.current);
            if self.selected_date_range != Some(range) {
                self.selected_date_range = Some(range);
                self.date_selection_cursor = Some(DateSelectionCursor {
                    anchor: drag.anchor,
                    head: drag.current,
                });
                changed = true;
            }
        } else if let Some(date) = date
            && self.drop_target.is_some()
            && self.drop_target != Some(date)
        {
            self.drop_target = Some(date);
            changed = true;
        }

        let next_speed = self.edge_scroll_speed_at(position);
        let speed_changed = self.set_edge_scroll_speed(next_speed, cx);
        if changed && !speed_changed {
            cx.notify();
        }
    }

    pub(super) fn apply_edge_scroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(speed) = self.edge_scroll_speed else {
            return;
        };
        if self.inspector_date.is_some() && self.marquee.is_active() {
            let moved = self
                .inspector_list
                .update(cx, |list, _| list.scroll_by_y(speed));
            if moved == px(0.) {
                self.set_edge_scroll_speed(None, cx);
                return;
            }
            self.marquee.scrolled_by(gpui::point(px(0.), moved));
            self.marquee.drag_to(window.mouse_position(), cx);
            return;
        }
        self.cancel_scroll();
        self.scroll_offset += speed;
        self.retarget_edge_drag(window.mouse_position(), cx);
    }

    fn set_bounds(&mut self, bounds: Bounds<Pixels>) {
        let height = (bounds.size.width / 7. * 1.3).clamp(MIN_WEEK_HEIGHT, MAX_WEEK_HEIGHT);
        if self.bounds.is_some_and(|previous| {
            height != self.week_height || previous.size.height != bounds.size.height
        }) {
            let anchor = self.top_offset_weeks(self.scroll_offset);
            self.cancel_scroll();
            let horizon = bounds.size.height / 2. + grid::EDGE_HORIZON;
            self.scroll_offset = self.visible_top() - horizon - anchor * height;
        } else if self.bounds.is_none() {
            self.scroll_offset = self.scroll_weeks() * height;
        }
        self.week_height = height;
        self.bounds = Some(bounds);
    }

    fn day_width(&self) -> Pixels {
        self.bounds.map_or(px(0.), |bounds| bounds.size.width / 7.)
    }

    fn day_bounds_for(&self, date: NaiveDate) -> Option<Bounds<Pixels>> {
        let viewport = self.bounds?;
        let week = date.week(Weekday::Sun);
        let horizon = viewport.size.height / 2. + grid::EDGE_HORIZON;
        let day_width = viewport.size.width / 7.;
        let weekday = date.weekday().num_days_from_sunday() as f32;
        Some(Bounds {
            origin: gpui::point(
                viewport.origin.x + day_width * weekday,
                viewport.origin.y + self.week_to_offset(week) + self.scroll_offset + horizon,
            ),
            size: gpui::size(day_width, self.week_height),
        })
    }

    fn clear_inspector_selection(&mut self, cx: &mut Context<Self>) {
        self.inspector_item_press = None;
        self.inspector_item_opened = None;
        if self.marquee.end() {
            cx.notify();
        }
        self.set_edge_scroll_speed(None, cx);
        SelectionManager::global(cx).update(cx, |selection, cx| {
            if selection.has_selection_in(SelectionScope::Calendar) {
                selection.clear(cx);
            }
        });
    }
    pub(super) fn close_day_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let restore_calendar_focus = self.inspector_focus.contains_focused(window, cx);
        let destination_focus = window.focused(cx);
        self.clear_inspector_selection(cx);
        self.inspector_bounds = None;
        self.inspector_list.update(cx, |list, cx| {
            list.reset_items(cx, |delegate, _| delegate.clear());
        });
        if self.inspector_date.take().is_none() {
            return;
        }
        self.inspector_range = None;
        ItemManager::global(cx).update(cx, |manager, cx| {
            manager.commit_open_edit(window, cx);
        });
        if restore_calendar_focus {
            self.focus_handle.focus(window, cx);
        } else if let Some(destination) = destination_focus {
            destination.focus(window, cx);
        }
        cx.notify();
    }

    fn begin_inspector_item_press(
        &mut self,
        item: AnyItem,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        self.inspector_item_press = Some(item.id());
        self.inspector_item_opened = None;
        if force_click_modifier(&event.modifiers) {
            self.inspect_pressed_item(item, cx);
        }
    }

    fn pressure_inspector_item(
        &mut self,
        item: AnyItem,
        event: &MousePressureEvent,
        cx: &mut Context<Self>,
    ) {
        if event.stage == PressureStage::Force {
            self.inspect_pressed_item(item, cx);
        }
    }

    fn inspect_pressed_item(&mut self, item: AnyItem, cx: &mut Context<Self>) {
        let item_id = item.id();
        if item_inspection_matches_press(
            self.inspector_item_press,
            self.inspector_item_opened,
            item_id,
        ) {
            self.inspector_item_opened = Some(item_id);
            cx.stop_propagation();
            cx.emit(InspectItem(item));
        }
    }

    fn markers_on(&self, date: NaiveDate) -> Vec<(Marker, bool)> {
        let mut markers: Vec<(Marker, bool)> = self
            .stored_markers()
            .filter(|marker| marker.covers(date))
            .cloned()
            .map(|marker| (marker, false))
            .chain(
                self.projected_markers(date, date)
                    .into_iter()
                    .filter(|marker| marker.covers(date))
                    .map(|marker| (marker, true)),
            )
            .collect();
        markers.sort_by_key(|(marker, projected)| (*projected, marker.date, marker.id));
        markers
    }

    fn markers_in_range(&self, range: InclusiveDateRange) -> Vec<(Marker, bool)> {
        let mut markers: Vec<(Marker, bool)> = Vec::new();
        for offset in 0..range.day_count() {
            let Some(date) = range
                .start()
                .checked_add_signed(chrono::Duration::days(i64::from(offset)))
            else {
                break;
            };
            for entry in self.markers_on(date) {
                if !markers.iter().any(|(marker, _)| marker.id == entry.0.id) {
                    markers.push(entry);
                }
            }
        }
        markers
    }

    fn items_on(&self, date: NaiveDate) -> Vec<(AnyItem, bool)> {
        let mut items: Vec<(AnyItem, bool)> = self
            .items
            .iter()
            .chain(self.draft_items.iter())
            .filter(|item| super::is_calendar_item_on(item, date))
            .cloned()
            .map(|item| (item, false))
            .chain(
                self.projected_items(date, date)
                    .into_iter()
                    .filter(|item| super::is_calendar_item_on(item, date))
                    .map(|item| (item, true)),
            )
            .chain(
                self.signals
                    .iter()
                    .filter(|signal| signal.datetime.with_timezone(&Local).date_naive() == date)
                    .cloned()
                    .map(|signal| (AnyItem::Signal(signal), false)),
            )
            .chain(
                self.projected_signals(date, date)
                    .into_iter()
                    .map(|signal| (AnyItem::Signal(signal), true)),
            )
            .collect();
        items.sort_by_key(|(item, projected)| {
            (
                *projected,
                item.start().map(|start| start.timestamp()),
                item.id(),
            )
        });
        items
    }

    fn items_in_range(&self, range: InclusiveDateRange) -> Vec<(AnyItem, bool)> {
        let mut items: Vec<(AnyItem, bool)> = Vec::new();
        for offset in 0..range.day_count() {
            let Some(date) = range
                .start()
                .checked_add_signed(chrono::Duration::days(i64::from(offset)))
            else {
                break;
            };
            for entry in self.items_on(date) {
                if !items.iter().any(|(item, _)| item.id() == entry.0.id()) {
                    items.push(entry);
                }
            }
        }
        items
    }

    fn inspector_entries(&self, range: InclusiveDateRange) -> Vec<CalendarInspectorEntry> {
        let mut entries: Vec<_> = self
            .markers_in_range(range)
            .into_iter()
            .map(|(marker, projected)| CalendarInspectorEntry {
                item: AnyItem::Marker(marker),
                projected,
            })
            .chain(
                self.items_in_range(range)
                    .into_iter()
                    .map(|(item, projected)| CalendarInspectorEntry { item, projected }),
            )
            .collect();
        entries.sort_by_key(|entry| {
            (
                entry.item.start().map(|start| start.timestamp()),
                entry.item.id(),
            )
        });
        entries
    }

    fn sync_inspector_list(&mut self, range: InclusiveDateRange, cx: &mut Context<Self>) {
        let generation = AppDatabaseStore::global(cx).read(cx).workspace_generation();
        if self
            .inspector_list
            .read(cx)
            .delegate()
            .workspace_generation()
            != generation
        {
            self.draft_items.clear();
            self.draft_markers.clear();
            self.inspector_item_focus_handles.clear();
            self.marquee.end();
        }
        let entries = self.inspector_entries(range);
        let order = SelectionOrder::new(
            SelectionScope::Calendar,
            entries
                .iter()
                .filter(|entry| !entry.projected)
                .map(|entry| entry.item.id()),
        );
        self.inspector_item_focus_handles
            .retain(|id, _| order.ids().contains(id));
        for id in order.ids() {
            self.inspector_item_focus_handles
                .entry(*id)
                .or_insert_with(|| cx.focus_handle());
        }
        let focus_handles = self.inspector_item_focus_handles.clone();
        self.inspector_list.update(cx, |list, cx| {
            list.update_items(cx, |delegate, _| {
                delegate.replace(entries, generation, order, focus_handles)
            });
        });
    }

    fn add_draft_item_on_date(
        &mut self,
        date: NaiveDate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let item = AnyItem::Action(
            Action::new("")
                .with_queued(true)
                .with_start(Some(SchedulePoint::Date(date))),
        );
        let id = item.id();
        self.draft_items.push(item.clone());
        self.sync_inspector_list(InclusiveDateRange::single(date), cx);
        ItemManager::global(cx).update(cx, |manager, cx| {
            manager.begin_edit(&item, true, window, cx);
        });
        let row = self.inspector_list.read(cx).delegate().index_of(id);
        if let Some(row) = row {
            self.inspector_list
                .update(cx, |list, cx| list.scroll_item_into_view(row, cx));
        }
        cx.notify();
    }

    fn add_draft_marker(
        &mut self,
        range: InclusiveDateRange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut marker = Marker::new("", range.start());
        marker.end_date = (range.day_count() > 1).then_some(range.end());
        let item = AnyItem::Marker(marker.clone());
        let id = item.id();
        self.draft_markers.push(marker);
        self.sync_inspector_list(range, cx);
        ItemManager::global(cx).update(cx, |manager, cx| {
            manager.begin_edit(&item, true, window, cx);
        });
        let row = self.inspector_list.read(cx).delegate().index_of(id);
        if let Some(row) = row {
            self.inspector_list
                .update(cx, |list, cx| list.scroll_item_into_view(row, cx));
        }
        cx.notify();
    }

    pub fn get_sunday(&self, date: NaiveDate) -> NaiveDate {
        let since_sunday = date.weekday().num_days_from_sunday();
        date - chrono::Duration::days(since_sunday as i64)
    }

    fn week_at_row(&self, row: i64) -> NaiveWeek {
        let sunday = self.get_sunday(self.current_date);
        (sunday + chrono::Duration::weeks(row + 2)).week(Weekday::Sun)
    }

    pub fn offset_to_week(&self, offset: Pixels, interval: Pixels) -> NaiveWeek {
        self.week_at_row((offset / interval).floor() as i64)
    }

    pub fn week_to_offset(&self, week: NaiveWeek) -> Pixels {
        let interval = self.week_height;
        let sunday = self.get_sunday(self.current_date);
        let weeks = (week.first_day() - sunday).num_days().div_euclid(7);
        interval * (weeks - 2) as f32
    }

    pub fn scroll_weeks(&self) -> f32 {
        self.scroll_offset / self.week_height
    }

    pub fn is_scrolling(&self) -> bool {
        self.scroll_target.is_some()
    }

    pub(super) fn apply_scroll_transition(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let interval = self.week_height;
        let current = self.scroll_weeks();
        let transition =
            window.keyed_transition("calendar-scroll", cx, transition::SCROLL, || current);

        if let Some(update) = self.scroll_update.take() {
            self.scroll_cancelled = false;
            transition.snap(current, cx);
            transition.set(update, cx);
        } else if self.scroll_cancelled {
            transition.snap(current, cx);
            self.scroll_cancelled = false;
        }

        let weeks = transition.animate(window, cx);
        if transition.is_animating(cx) {
            self.scroll_offset = interval * weeks;
        } else {
            self.scroll_target = None;
        }
    }

    pub fn cancel_scroll(&mut self) {
        if self.scroll_update.is_some() || self.is_scrolling() {
            self.scroll_update = None;
            self.scroll_target = None;
            self.scroll_cancelled = true;
        }
    }

    fn visible_top(&self) -> Pixels {
        DAY_NAMES_TOP + DAY_NAMES_HEIGHT
    }

    fn top_offset_weeks(&self, offset: Pixels) -> f32 {
        let horizon = self.bounds.unwrap_or_default().size.height / 2. + grid::EDGE_HORIZON;
        (self.visible_top() - horizon - offset) / self.week_height
    }

    pub fn scroll_to_weeks(&mut self, weeks: f32, cx: &mut Context<Self>) {
        self.scroll_update = Some(weeks);
        self.scroll_target = Some(weeks);
        cx.notify();
    }

    pub fn scroll_to_week(&mut self, week: NaiveWeek, cx: &mut Context<Self>) {
        let top = self.week_to_offset(week);
        self.scroll_to_weeks(self.top_offset_weeks(top), cx);
    }

    pub fn scroll_to_date(&mut self, date: NaiveDate, cx: &mut Context<Self>) {
        self.scroll_to_week(date.week(Weekday::Sun), cx);
    }

    fn reveal_date(&mut self, date: NaiveDate, cx: &mut Context<Self>) {
        let Some(bounds) = self.bounds else {
            return;
        };
        let current_weeks = self.scroll_target.unwrap_or_else(|| self.scroll_weeks());
        let horizon = bounds.size.height / 2. + grid::EDGE_HORIZON;
        let row_top = self.week_to_offset(date.week(Weekday::Sun))
            + self.week_height * current_weeks
            + horizon;
        if let Some(target) = scroll_weeks_to_reveal_row(
            row_top,
            self.week_height,
            self.visible_top(),
            bounds.size.height,
            current_weeks,
        ) {
            self.scroll_to_weeks(target, cx);
        }
    }

    fn set_date_selection(&mut self, anchor: NaiveDate, head: NaiveDate) {
        let cursor = DateSelectionCursor { anchor, head };
        self.selected_date_range = Some(cursor.range());
        self.date_selection_cursor = Some(cursor);
    }

    fn clear_date_selection(&mut self) -> bool {
        self.date_selection_cursor = None;
        self.selected_date_range.take().is_some()
    }

    fn move_date_selection(&mut self, days: i64, extend_selection: bool, cx: &mut Context<Self>) {
        if self.month_selector_open || self.inspector_date.is_some() {
            cx.propagate();
            return;
        }
        let current = self.date_selection_cursor.unwrap_or_else(|| {
            self.selected_date_range
                .map(|range| DateSelectionCursor {
                    anchor: range.start(),
                    head: range.end(),
                })
                .unwrap_or_else(|| DateSelectionCursor::single(self.date_in_view()))
        });
        let Some(next) = current.shifted(days, extend_selection) else {
            return;
        };

        self.date_range_drag = None;
        self.set_edge_scroll_speed(None, cx);
        self.set_date_selection(next.anchor, next.head);
        self.reveal_date(next.head, cx);
        cx.notify();
    }

    pub fn scroll_to_today(&mut self, cx: &mut Context<Self>) {
        self.scroll_to_weeks(0., cx);
        self.reveal_date(self.current_date, cx);
        let year = self.current_date.year();
        let month = Month::try_from(self.current_date.month() as u8).unwrap_or(Month::January);
        self.month_select
            .update(cx, |select, cx| select.set_month(year, month, cx));
    }

    fn today_is_offscreen(&self) -> bool {
        let Some(bounds) = self.bounds else {
            return false;
        };
        let horizon = bounds.size.height / 2. + grid::EDGE_HORIZON;
        let top = self.week_to_offset(self.current_date.week(Weekday::Sun))
            + self.scroll_offset
            + horizon;

        top + self.week_height < self.visible_top() || top > bounds.size.height
    }

    pub fn scroll_to_month(&mut self, year: i32, month: Month, cx: &mut Context<Self>) {
        let Some(first) = NaiveDate::from_ymd_opt(year, month.number_from_month(), 1) else {
            return;
        };
        self.scroll_to_date(first, cx);
    }

    fn date_in_view(&self) -> NaiveDate {
        let Some(bounds) = self.bounds else {
            return self.current_date;
        };
        let middle = (self.visible_top() + bounds.size.height) / 2.;
        let horizon = bounds.size.height / 2. + grid::EDGE_HORIZON;
        let week = self.offset_to_week(middle - self.scroll_offset - horizon, self.week_height);
        week.first_day() + chrono::Duration::days(3)
    }

    pub(super) fn sync_month_in_view(&mut self, cx: &mut Context<Self>) {
        let Some(month_start) = self.date_in_view().with_day(1) else {
            return;
        };
        if month_start == self.month_in_view {
            return;
        }

        self.month_in_view = month_start;
        let year = month_start.year();
        let month = Month::try_from(month_start.month() as u8).unwrap_or(Month::January);
        self.month_select
            .update(cx, |select, cx| select.set_month(year, month, cx));
    }

    pub(super) fn set_drop_target(
        &mut self,
        date: NaiveDate,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let next_speed = self.edge_scroll_speed_at(position);
        let target_changed = self.drop_target != Some(date);
        if target_changed {
            self.drop_target = Some(date);
        }
        let speed_changed = self.set_edge_scroll_speed(next_speed, cx);
        if target_changed && !speed_changed {
            cx.notify();
        }
    }

    fn handle_item_drag_move(&mut self, position: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let drag_started = !self.item_drag_active;
        self.item_drag_active = true;
        if drag_started {
            self.inspector_bounds = None;
        }
        let target_changed = self.drop_target.take().is_some();
        let speed = self.edge_scroll_speed_at(position);
        let speed_changed = self.set_edge_scroll_speed(speed, cx);
        if (drag_started || target_changed) && !speed_changed {
            cx.notify();
        }
    }

    pub(super) fn clear_drop_target(&mut self, cx: &mut Context<Self>) {
        let drag_ended = std::mem::take(&mut self.item_drag_active);
        let target_changed = self.drop_target.take().is_some();
        let speed_changed = self.set_edge_scroll_speed(None, cx);
        if (drag_ended || target_changed) && !speed_changed {
            cx.notify();
        }
    }

    pub(super) fn commit_date_drop(
        &mut self,
        dragged: &DraggedItems,
        date: NaiveDate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dragged = resolve_dragged(dragged, cx);
        let count = scheduled_item_count(&dragged.items);
        if count == 0 {
            self.clear_drop_target(cx);
            return;
        }

        let source_date = dragged.source_anchor().map(NaiveDate::from).unwrap_or(date);
        let day_delta = date - source_date;
        let placements: Vec<_> = dragged
            .items
            .iter()
            .cloned()
            .map(|item| {
                let target_date = item
                    .start_date()
                    .and_then(|item_date| item_date.checked_add_signed(day_delta))
                    .unwrap_or(date);
                (item, target_date)
            })
            .collect();
        let mut detail = format!(
            "Schedule these items for {}. Existing times and multi-day spans stay the same.",
            date.format("%A, %B %-d, %Y")
        );
        if dragged.items.len() > 1 && dragged.source_anchor().is_some() {
            detail.push_str(" Items on different days keep their spacing.");
        }

        self.clear_drop_target(cx);
        confirm_drop(count, "Schedule", detail, window, cx, move |_, cx| {
            AppDatabaseStore::global(cx).update(cx, |store, cx| {
                let mut updated = Vec::new();
                let mut routines = Vec::new();
                for (item, target_date) in placements {
                    match item {
                        AnyItem::Action(action) => {
                            updated
                                .push(AnyItem::Action(action_dropped_on_date(action, target_date)));
                        }
                        AnyItem::Event(mut event) => {
                            let local = event.start.with_timezone(&Local);
                            event.start = local_time_on(target_date, local.time());
                            updated.push(AnyItem::Event(event));
                        }
                        AnyItem::Routine(routine) => {
                            let start = match routine.target {
                                Some(SchedulePoint::DateTime(start)) => {
                                    let local = start.with_timezone(&Local);
                                    local_time_on(target_date, local.time())
                                }
                                Some(SchedulePoint::Date(_)) | None => {
                                    local_time_on(target_date, chrono::NaiveTime::MIN)
                                }
                            };
                            routines.push((routine.id, Some(start)));
                        }
                        AnyItem::Marker(mut marker) => {
                            let span = marker.end_date.map(|end| end - marker.date);
                            marker.set_date(target_date);
                            marker.set_end_date(span.map(|span| target_date + span));
                            updated.push(AnyItem::Marker(marker));
                        }
                        AnyItem::Signal(mut signal) => {
                            let local = signal.datetime.with_timezone(&Local);
                            signal.datetime = local_time_on(target_date, local.time());
                            updated.push(AnyItem::Signal(signal));
                        }
                        AnyItem::ActionTemplate(_) | AnyItem::EventTemplate(_) => {}
                    }
                }
                let _ = store.update_items_with_routines(updated, routines, cx);
            });
        });
    }

    pub fn drop_zone(&self) -> DropZone<DragData<DraggedItems>> {
        DropZone::new("calendar-drop")
            .size_full()
            .active(self.drop_target.is_some())
            .rounded_none()
            .rounded_bl_2xl()
    }

    pub fn toggle_month_selector(&mut self, cx: &mut Context<Self>) {
        self.month_selector_open = !self.month_selector_open;
        cx.notify();
    }

    fn navigate_inspector_items(
        &mut self,
        order: &SelectionOrder,
        delta: isize,
        extend_selection: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if order.ids().is_empty() || ItemManager::global(cx).read(cx).is_editing() {
            return false;
        }
        let current = order.ids().iter().position(|id| {
            self.inspector_item_focus_handles
                .get(id)
                .is_some_and(|handle| handle.contains_focused(window, cx))
        });
        let target = match current {
            Some(current) => current
                .checked_add_signed(delta)
                .filter(|target| *target < order.ids().len()),
            None if delta < 0 => order.ids().len().checked_sub(1),
            None => Some(0),
        };
        let Some(target) = target else {
            return false;
        };
        let target_id = order.ids()[target];
        let Some(handle) = self.inspector_item_focus_handles.get(&target_id).cloned() else {
            return false;
        };
        if extend_selection {
            let origin = current.map(|index| order.ids()[index]).unwrap_or(target_id);
            focus_item_extending(order, origin, target_id, &handle, window, cx);
        } else {
            focus_item(SelectionScope::Calendar, target_id, &handle, window, cx);
        }
        let row = self.inspector_list.read(cx).delegate().index_of(target_id);
        if let Some(row) = row {
            self.inspector_list
                .update(cx, |list, cx| list.scroll_item_into_view(row, cx));
        }
        true
    }

    fn render_day_inspector(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        self.inspector_bounds = None;
        if self.item_drag_active {
            return None;
        }
        let date = self.inspector_date?;
        let range = self
            .inspector_range
            .unwrap_or_else(|| InclusiveDateRange::single(date));
        let date_key = date.num_days_from_ce() as u32;
        self.inspector_list.update(cx, |list, cx| {
            list.set_marquee_active(self.marquee.is_active(), cx);
        });
        let inspector_list = self.inspector_list.read(cx);
        let inspector_order = inspector_list.delegate().order().clone();
        let viewport = self.bounds?;
        let cell = self.day_bounds_for(date)?;
        let available_width =
            (viewport.size.width - DAY_INSPECTOR_VIEWPORT_INSET).max(cell.size.width);
        let target_width = (cell.size.width * 2.5)
            .max(DAY_INSPECTOR_MIN_WIDTH)
            .min(DAY_INSPECTOR_MAX_WIDTH)
            .min(available_width);
        let list_height = inspector_list.content_height();
        let desired_height = DAY_INSPECTOR_HEADER_HEIGHT
            + DAY_INSPECTOR_ENTRIES_PADDING * 2.
            + DAY_INSPECTOR_BORDER_WIDTH * 2.
            + list_height;
        let visible_top = self.visible_top().min(viewport.size.height);
        let visible_height = (viewport.size.height - visible_top).max(px(0.));
        let available_height = (visible_height - DAY_INSPECTOR_VIEWPORT_INSET)
            .max(cell.size.height)
            .min(visible_height);
        let target_height = desired_height
            .max(DAY_INSPECTOR_MIN_HEIGHT)
            .min(DAY_INSPECTOR_MAX_HEIGHT)
            .min(available_height);
        let transition = window.keyed_transition(
            ("calendar-day-inspector-expand", date_key),
            cx,
            transition::QUICK,
            || 0.0,
        );
        transition.set(1.0, cx);
        let reveal = ease_in_out(transition.animate(window, cx));
        let width = cell.size.width + (target_width - cell.size.width) * reveal;
        let height =
            (cell.size.height + (target_height - cell.size.height) * reveal).min(visible_height);
        let cell_left = cell.origin.x - viewport.origin.x;
        let cell_top = cell.origin.y - viewport.origin.y;
        let left = if self.inspector_expands_left {
            cell_left + cell.size.width - width
        } else {
            cell_left
        }
        .clamp(px(0.), (viewport.size.width - width).max(px(0.)));
        let top = if self.inspector_expands_up {
            cell_top + cell.size.height - height
        } else {
            cell_top
        }
        .clamp(
            visible_top,
            (viewport.size.height - height).max(visible_top),
        );
        let title = if range.day_count() == 1 {
            date.format("%A, %B %-d").to_string()
        } else {
            format!(
                "{} – {} · {} days",
                range.start().format("%b %-d"),
                range.end().format("%b %-d, %Y"),
                range.day_count()
            )
        };
        let entity = cx.entity();
        let glass_short_edge = f32::from(cell.size.width).min(f32::from(cell.size.height));

        let frame = div()
            .id(("calendar-day-inspector", date_key))
            .key_context("Overlay")
            .track_focus(&self.inspector_focus)
            .on_action(cx.listener(|view, _: &CloseOverlay, window, cx| {
                view.close_day_inspector(window, cx);
            }))
            .on_mouse_down_out(cx.listener(|view, event: &MouseDownEvent, window, cx| {
                if !view.item_drag_active
                    && !cx.has_active_drag()
                    && !crate::components::menu::context_menu_contains_position(event.position, cx)
                {
                    view.close_day_inspector(window, cx);
                }
            }))
            .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                if event.is_held {
                    return;
                }
                let key = event.keystroke.key.as_str();
                let delta = match key {
                    "up" | "k" => -1,
                    "down" | "j" => 1,
                    _ => return,
                };
                let extend_selection =
                    event.keystroke.modifiers.shift && matches!(key, "up" | "down");
                if view.navigate_inspector_items(
                    &inspector_order,
                    delta,
                    extend_selection,
                    window,
                    cx,
                ) {
                    cx.stop_propagation();
                }
            }))
            .column()
            .w(width)
            .h(height)
            .overflow_hidden()
            .rounded(px(cx.theme().radii.dialog))
            .border_1()
            .border_color(cx.theme().colors.hairline)
            .text_color(cx.theme().colors.text)
            .block_mouse_except_scroll()
            .on_mouse_down(MouseButton::Right, {
                let entity = entity.clone();
                move |event, window, cx| {
                    cx.stop_propagation();
                    let menu = if range.day_count() > 1 {
                        date_range_context_menu(range, entity.clone())
                    } else {
                        date_context_menu(date, true, entity.clone())
                    };
                    crate::components::menu::open_context_menu(menu, event.position, window, cx);
                }
            })
            .child(
                div()
                    .row()
                    .flex_none()
                    .h(DAY_INSPECTOR_HEADER_HEIGHT)
                    .items_center()
                    .justify_between()
                    .gap_1()
                    .min_w_0()
                    .px_3()
                    .border_b_1()
                    .border_color(cx.theme().colors.hairline)
                    .child(
                        Label::new(title)
                            .font_weight(FontWeight::SEMIBOLD)
                            .flex_1()
                            .min_w_0()
                            .truncate(),
                    )
                    .child(
                        div()
                            .row()
                            .flex_none()
                            .items_center()
                            .gap_1()
                            .child(
                                Button::new(("calendar-day-inspector-new", date_key))
                                    .ghost()
                                    .small()
                                    .compact()
                                    .px_2()
                                    .icon(AppIcon::Plus)
                                    .tooltip(if range.day_count() > 1 {
                                        "New marker"
                                    } else {
                                        "New item"
                                    })
                                    .block_mouse_except_scroll()
                                    .on_click(cx.listener(move |view, _, window, cx| {
                                        if range.day_count() > 1 {
                                            view.add_draft_marker(range, window, cx);
                                        } else {
                                            view.add_draft_item_on_date(date, window, cx);
                                        }
                                    })),
                            )
                            .child(
                                Button::new(("calendar-day-inspector-close", date_key))
                                    .ghost()
                                    .small()
                                    .compact()
                                    .px_2()
                                    .icon(AppIcon::Close)
                                    .tooltip("Close")
                                    .block_mouse_except_scroll()
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.close_day_inspector(window, cx)
                                    })),
                            ),
                    ),
            )
            .child(marquee(
                div()
                    .id(("calendar-day-inspector-entries", date_key))
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .p(DAY_INSPECTOR_ENTRIES_PADDING)
                    .child(DynamicList::new(&self.inspector_list).size_full()),
                self,
                cx,
            ));

        let inspector = cx.entity();
        let panel = div()
            .absolute()
            .left(left)
            .top(top)
            .w(width)
            .h(height)
            .opacity(reveal)
            .shadow_lg()
            .on_prepaint(move |bounds, _, cx| {
                inspector.update(cx, |view, _| view.inspector_bounds = Some(bounds));
            })
            .child(ThemeOverlay::new(
                move |theme| crate::views::fixed_glass_bevel_theme(theme, glass_short_edge),
                frame
                    .bg_glass()
                    .glass_surface(Surface::Overlay)
                    .glass_radius(Radius::Dialog)
                    .glass(|glass| glass.protect_text_contrast(false))
                    .when(cfg!(not(target_os = "macos")), |frame| {
                        frame.glass_preset(GlassPreset::Frosted)
                    }),
            ));

        Some(panel)
    }

    fn render_weekday_labels(&self, cx: &App) -> gpui::Div {
        let narrow = self.day_width() < px(60.);
        let days = if narrow {
            ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"]
        } else {
            ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
        };
        div()
            .row()
            .size_full()
            .min_w_0()
            .children(days.into_iter().map(|day| {
                div()
                    .column()
                    .h_full()
                    .w(relative(1. / 7.))
                    .flex_none()
                    .min_w_0()
                    .px_2()
                    .when(narrow, |this| this.px_0p5())
                    .pb_2()
                    .items_end()
                    .overflow_hidden()
                    .child(
                        Label::new(day)
                            .max_w_full()
                            .when(narrow, |this| this.text_xs())
                            .truncate()
                            .text_color(cx.theme().colors.text_muted),
                    )
            }))
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(super) fn render_header_foreground(&self, cx: &App) -> AnyElement {
        div()
            .id("calendar.day-names")
            .debug_selector(|| "calendar.day-names".into())
            .w_full()
            .min_w_0()
            .h(DAY_NAMES_HEIGHT)
            .overflow_hidden()
            .child(
                self.render_weekday_labels(cx)
                    .max_w(MAX_CALENDAR_WIDTH)
                    .mx_auto(),
            )
            .into_any_element()
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    pub fn render_day_names(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .debug_selector(|| "calendar.day-names".into())
            .absolute()
            .top(DAY_NAMES_TOP)
            .left_0()
            .right_0()
            .h(DAY_NAMES_HEIGHT)
            .block_mouse_except_scroll()
            .child(
                self.render_weekday_labels(cx)
                    .id("calendar.day-names")
                    .bg_glass()
                    .glass_surface(Surface::Canvas)
                    .glass_radius_px(0.0)
                    .glass(|glass| glass.protect_text_contrast(false))
                    .glass_preset(GlassPreset::Frosted)
                    .border_b_1()
                    .border_color(cx.theme().colors.hairline),
            )
    }

    pub fn render_today_shortcut(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if !self.today_is_offscreen() {
            return None;
        }
        let compact = self.day_width() < px(80.);
        let label = format!("Today · {}", self.current_date.format("%b %-d"));

        Some(
            div()
                .absolute()
                .top(self.visible_top() + px(12.))
                .left(px(12.))
                .child(
                    Button::new("calendar-today-shortcut")
                        .glass_pill()
                        .small()
                        .icon(AppIcon::Calendar)
                        .tooltip(label.clone())
                        .when(self.day_width() >= px(40.), |this| {
                            this.label(if compact { "Today".to_string() } else { label })
                        })
                        .text_color(cx.theme().colors.text)
                        .block_mouse_except_scroll()
                        .on_click(cx.listener(|view, _, _, cx| view.scroll_to_today(cx))),
                ),
        )
    }

    pub fn render_month_selection(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let compact = self.day_width() < px(80.);
        let top = self.visible_top() + px(12.);
        let button_height = if compact { px(32.) } else { px(40.) };
        let available_width = (self.day_width() * 7. - px(24.)).max(px(0.));
        let available_height = self.bounds.map_or(px(0.), |bounds| {
            (bounds.size.height - top - button_height - px(8.) - px(12.)).max(px(0.))
        });
        let month = self
            .month_in_view
            .format(if compact { "%b" } else { "%B" })
            .to_string();
        let year = self.month_in_view.format("%Y").to_string();

        div()
            .column()
            .absolute()
            .top(top)
            .right(px(12.))
            .gap_2()
            .items_end()
            .child(
                Button::new("calendar-month-select")
                    .glass_pill()
                    .h(button_height)
                    .px_4()
                    .max_w(available_width)
                    .min_w_0()
                    .when(compact, |this| this.px_2())
                    .tooltip(self.month_in_view.format("%B %Y").to_string())
                    .block_mouse_except_scroll()
                    .gap_1()
                    .child(
                        Label::new(month)
                            .text_2xl()
                            .when(compact, |this| this.text_base())
                            .flex_1()
                            .min_w_0()
                            .truncate(),
                    )
                    .child(
                        Label::new(year)
                            .text_2xl()
                            .when(compact, |this| this.text_base())
                            .font_weight(FontWeight::EXTRA_LIGHT),
                    )
                    .on_click(cx.listener(|view, _, _window, cx| {
                        view.toggle_month_selector(cx);
                    })),
            )
            .when(self.month_selector_open, |this| {
                this.child(
                    div()
                        .id("calendar-month-selector-scroll")
                        .max_w(available_width)
                        .max_h(available_height)
                        .min_h_0()
                        .overflow_scroll()
                        .shadow_lg()
                        .child(
                            div()
                                .id("calendar-month-selector")
                                .bg_glass()
                                .glass_surface(Surface::Overlay)
                                .glass_radius(Radius::Dialog)
                                .glass(|glass| glass.protect_text_contrast(false))
                                .when(cfg!(not(target_os = "macos")), |frame| {
                                    frame.glass_preset(GlassPreset::Frosted)
                                })
                                .child(
                                    MonthSelect::new(&self.month_select)
                                        .when(self.day_width() < px(60.), |this| this.small()),
                                ),
                        ),
                )
            })
    }
}

impl Focusable for CalendarView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<OpenDateRangeInTimeline> for CalendarView {}
impl EventEmitter<InspectItem> for CalendarView {}

impl MainViewTab for CalendarView {
    const TAB: SelectedMainView = SelectedMainView::Calendar;

    fn scope() -> Option<SelectionScope> {
        Some(SelectionScope::Calendar)
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.inspector_date.is_some() {
            self.close_day_inspector(window, cx);
            return true;
        }
        if self.month_selector_open {
            self.month_selector_open = false;
            self.focus_handle.focus(window, cx);
            cx.notify();
            return true;
        }
        if !self.clear_date_selection() {
            return false;
        }
        cx.notify();
        true
    }

    fn go_to_now(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.scroll_to_today(cx);
    }
}

impl MarqueeView for CalendarView {
    fn marquee(&self) -> &MarqueeSelection {
        &self.marquee
    }

    fn marquee_mut(&mut self) -> &mut MarqueeSelection {
        &mut self.marquee
    }

    fn marquee_enabled(&self, _cx: &App) -> bool {
        self.inspector_date.is_some()
    }

    fn marquee_dragged(&mut self, position: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let bounds = self.inspector_list.read(cx).scroll_handle().bounds();
        let speed = bounds
            .localize(&position)
            .and_then(|local| edge_scroll_speed(local.y, bounds.size.height, px(0.)));
        self.set_edge_scroll_speed(speed, cx);
    }

    fn marquee_ended(&mut self, cx: &mut Context<Self>) {
        self.set_edge_scroll_speed(None, cx);
    }

    fn marquee_focus(&self, _cx: &App) -> Option<FocusHandle> {
        Some(
            if self.inspector_date.is_some() {
                &self.inspector_focus
            } else {
                &self.focus_handle
            }
            .clone(),
        )
    }
}

impl Render for CalendarView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let body = self
            .drop_zone()
            .absolute()
            .inset_0()
            .on_prepaint(move |bounds, _, cx| {
                entity.update(cx, |view, cx| {
                    if view.bounds != Some(bounds) {
                        view.set_bounds(bounds);
                        cx.notify();
                    }
                });
            })
            .flex()
            .flex_1()
            .overflow_hidden()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, event: &MouseUpEvent, _window, cx| {
                    view.finish_date_range_drag_at(event.position, cx);
                }),
            )
            .on_scroll_wheel(cx.listener(|view, event: &ScrollWheelEvent, window, cx| {
                let delta = event.delta.pixel_delta(window.line_height()).y;
                if delta == px(0.) {
                    return;
                }

                match event.delta {
                    ScrollDelta::Pixels(_) => {
                        view.cancel_scroll();
                        view.scroll_offset += delta;
                        cx.notify();
                    }
                    ScrollDelta::Lines(_) => {
                        let current = view.scroll_target.unwrap_or_else(|| view.scroll_weeks());
                        view.scroll_to_weeks(current + delta / view.week_height, cx);
                    }
                }
            }))
            .child(
                ScrollFade::new("calendar.grid-top-fade")
                    .top(true)
                    .band(f32::from(self.visible_top()))
                    .text_only()
                    .child(self.render_grid(window, cx)),
            );
        let inspector = self.render_day_inspector(window, cx);
        let compact_inspector = inspector.is_some()
            && self.bounds.is_some_and(|bounds| {
                bounds.size.height - self.visible_top() < DAY_INSPECTOR_MIN_HEIGHT
            });
        let calendar = div()
            .relative()
            .w_full()
            .min_w_0()
            .max_w(MAX_CALENDAR_WIDTH)
            .h_full()
            .flex_none()
            .mx_auto()
            .child(body)
            .children(inspector);
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let calendar = calendar.child(self.render_day_names(cx));

        let viewport = cx.entity();

        self.tab_root(div().relative().column(), cx)
            .group("calendar-view")
            .capture_any_mouse_down(cx.listener(|view, _: &MouseDownEvent, _window, _cx| {
                view.inspector_item_press = None;
                view.inspector_item_opened = None;
            }))
            .on_drag_move::<DragData<DraggedItems>>(cx.listener(
                |view, event: &DragMoveEvent<DragData<DraggedItems>>, _, cx| {
                    view.handle_item_drag_move(event.event.position, cx);
                },
            ))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| view.clear_drop_target(cx)),
            )
            .on_action(cx.listener(|view, _: &OpenSelectedRange, window, cx| {
                let Some(range) = view.selected_date_range else {
                    cx.propagate();
                    return;
                };
                if view.month_selector_open || view.inspector_date.is_some() {
                    cx.propagate();
                    return;
                }
                view.open_range_inspector(range, range.start(), window, cx);
            }))
            .on_action(cx.listener(|view, _: &MoveDateSelectionLeft, _, cx| {
                view.move_date_selection(-1, false, cx)
            }))
            .on_action(cx.listener(|view, _: &MoveDateSelectionRight, _, cx| {
                view.move_date_selection(1, false, cx)
            }))
            .on_action(cx.listener(|view, _: &MoveDateSelectionUp, _, cx| {
                view.move_date_selection(-7, false, cx)
            }))
            .on_action(cx.listener(|view, _: &MoveDateSelectionDown, _, cx| {
                view.move_date_selection(7, false, cx)
            }))
            .on_action(cx.listener(|view, _: &ExtendDateSelectionLeft, _, cx| {
                view.move_date_selection(-1, true, cx)
            }))
            .on_action(cx.listener(|view, _: &ExtendDateSelectionRight, _, cx| {
                view.move_date_selection(1, true, cx)
            }))
            .on_action(cx.listener(|view, _: &ExtendDateSelectionUp, _, cx| {
                view.move_date_selection(-7, true, cx)
            }))
            .on_action(cx.listener(|view, _: &ExtendDateSelectionDown, _, cx| {
                view.move_date_selection(7, true, cx)
            }))
            .on_action(cx.listener(|view, _: &CloseOverlay, window, cx| {
                if view.inspector_date.is_some() {
                    view.close_day_inspector(window, cx);
                } else if view.clear_date_selection() {
                    cx.notify();
                } else {
                    cx.propagate();
                }
            }))
            .size_full()
            .min_w_0()
            .min_h_0()
            .child(
                div()
                    .id("calendar-viewport")
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .on_prepaint(move |bounds, _, cx| {
                        let has_horizontal_gutters = bounds.size.width > MAX_CALENDAR_WIDTH;
                        viewport.update(cx, |view, cx| {
                            if view.has_horizontal_gutters != has_horizontal_gutters {
                                view.has_horizontal_gutters = has_horizontal_gutters;
                                cx.notify();
                            }
                        });
                    })
                    .child(calendar),
            )
            .when(!compact_inspector, |this| {
                this.child(self.render_month_selection(cx))
                    .children(self.render_today_shortcut(cx))
            })
    }
}
