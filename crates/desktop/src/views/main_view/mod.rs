use gpui_kit::foundation::StyledExt as _;
use std::time::Duration;

use anyhow::Ok;
use chrono::{DateTime, Duration as ChronoDuration, Local, NaiveDate, TimeZone, Utc};
use chronoutil::RelativeDuration;
use gpui::{
    App, AppContext, AsyncApp, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString, Styled, Window, div,
    prelude::FluentBuilder,
};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use gpui::{AnyElement, Pixels};
use subroutine_core::{Action, AnyItem, SchedulePoint};

mod calendar_view;
mod focus_view;

mod queue_view;
mod tab;
mod timeline_view;
use calendar_view::*;
use focus_view::*;
pub(crate) use focus_view::{
    event_notice_card,
    temporal::{EventMoment, TemporalSnapshot},
};

use queue_view::*;
use tab::MainViewTab;
pub use tab::SelectedMainView;
pub(crate) use tab::{
    COMMAND_KEY_CONTEXT as MAIN_VIEW_KEY_CONTEXT, GoToNow, NextTab, PreviousTab, RefreshPipeline,
};
use timeline_view::*;
use uuid::Uuid;

pub fn init(cx: &mut App) {
    tab::init(cx);
    calendar_view::init(cx);
    timeline_view::init(cx);
}

#[derive(Clone)]
pub(crate) struct InspectItem(pub AnyItem);

#[derive(Clone, Copy)]
pub(crate) struct SelectedMainViewChanged(pub SelectedMainView);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScheduledItemDestination {
    Timeline,
    Queue,
    Calendar,
}

pub(super) fn item_inspection_matches_press(
    pressed: Option<Uuid>,
    opened: Option<Uuid>,
    item: Uuid,
) -> bool {
    pressed == Some(item) && opened != Some(item)
}

pub(super) fn is_calendar_item_on(item: &AnyItem, date: NaiveDate) -> bool {
    !matches!(item, AnyItem::Marker(_)) && item.start_date() == Some(date)
}

pub(super) fn action_start_on_date(action: &Action, date: NaiveDate) -> SchedulePoint {
    match action.start {
        Some(SchedulePoint::DateTime(start)) => {
            let local = start.with_timezone(&Local);
            SchedulePoint::DateTime(local_time_on(date, local.time()))
        }
        _ => SchedulePoint::Date(date),
    }
}

pub(super) fn local_time_on(date: NaiveDate, time: chrono::NaiveTime) -> DateTime<Utc> {
    let requested = date.and_time(time);
    for minutes in 0..=180 {
        let candidate = requested + ChronoDuration::minutes(minutes);
        if let Some(local) = Local.from_local_datetime(&candidate).earliest() {
            return local.with_timezone(&Utc);
        }
    }

    date.and_time(chrono::NaiveTime::MIN).and_utc()
}

pub(super) fn format_item_time(t: DateTime<Local>) -> String {
    t.format("%-I:%M%P").to_string().replace(":00", "")
}

pub(super) fn format_item_duration(duration: RelativeDuration, from: DateTime<Local>) -> String {
    let total_mins = ((from + duration) - from).num_minutes();
    if total_mins <= 0 {
        return String::new();
    }
    if total_mins % 60 == 0 {
        format!("{}h", total_mins / 60)
    } else if total_mins >= 60 {
        format!("{}h {}m", total_mins / 60, total_mins % 60)
    } else {
        format!("{}m", total_mins)
    }
}

pub(crate) fn ordered_focus_actions(
    actions: &[Action],
    now: DateTime<Utc>,
    horizon: ChronoDuration,
) -> Vec<AnyItem> {
    let today = now.with_timezone(&Local).date_naive();
    let horizon_end = now + horizon;
    let mut ranked = actions
        .iter()
        .filter(|action| !action.is_completed())
        .filter_map(|action| {
            let rank = match action.start {
                Some(SchedulePoint::DateTime(start)) if start < now => 0,
                Some(SchedulePoint::Date(date)) if date < today => 0,
                Some(SchedulePoint::DateTime(start)) if start <= horizon_end => 1,
                Some(SchedulePoint::Date(date)) if date == today => 2,
                None if action.queued => 3,
                None => 4,
                Some(SchedulePoint::DateTime(_)) | Some(SchedulePoint::Date(_)) => return None,
            };
            let scheduled = action.start.map(DateTime::<Utc>::from);
            Some((rank, scheduled, action.id, action.clone()))
        })
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(rank, scheduled, id, _)| (*rank, *scheduled, *id));
    ranked
        .into_iter()
        .map(|(_, _, _, action)| AnyItem::Action(action))
        .collect()
}

pub(super) fn format_item_meta(item: &AnyItem) -> Option<SharedString> {
    let start = item.start_datetime();
    let time_str = match item.start() {
        Some(SchedulePoint::Date(_)) => Some("Any time".to_string()),
        Some(SchedulePoint::DateTime(_)) => start.map(format_item_time),
        None => None,
    };
    let dur_str = item
        .duration()
        .map(|d| format_item_duration(d, start.unwrap_or_else(Local::now)))
        .filter(|s| !s.is_empty());
    match (time_str, dur_str) {
        (Some(t), Some(d)) => Some(format!("{t} · {d}").into()),
        (Some(t), None) => Some(t.into()),
        (None, Some(d)) => Some(d.into()),
        (None, None) => None,
    }
}

pub struct DeleteItem {
    pub _item: AnyItem,
}

use crate::{
    item_manager::ItemManager,
    notifications,
    selection::{SelectionManager, SelectionScope},
    settings::Settings,
    stores::{AppDatabaseStore, DataChanged},
};

struct MainViewData {
    queue: Vec<AnyItem>,
    focus_actions: Vec<Action>,
    agenda: Vec<AnyItem>,
    timeline: Vec<AnyItem>,
    events: Vec<subroutine_core::Event>,
    markers: Vec<subroutine_core::Marker>,
    signals: Vec<subroutine_core::Signal>,
}

impl MainViewData {
    fn read(store: &AppDatabaseStore, settings: &Settings) -> Self {
        let pipeline = store.pipeline(settings);
        let agenda = pipeline.queue_items();
        let events = agenda
            .iter()
            .filter_map(|item| match item {
                AnyItem::Event(event) => Some(event.clone()),
                _ => None,
            })
            .collect();
        Self {
            queue: pipeline
                .queue()
                .into_iter()
                .cloned()
                .map(AnyItem::Action)
                .collect(),
            focus_actions: store.actions().to_vec(),
            agenda,
            timeline: pipeline.timeline_items(),
            events,
            markers: store
                .markers()
                .iter()
                .filter(|marker| {
                    marker.source_provider.is_some()
                        || marker.id == marker.lineage_id
                        || marker.recurrence.is_none()
                })
                .cloned()
                .collect(),
            signals: store
                .signals()
                .iter()
                .filter(|signal| signal.id == signal.lineage_id || signal.recurrence.is_none())
                .cloned()
                .collect(),
        }
    }
}

pub struct MainView {
    pub(crate) focus_handle: FocusHandle,
    queue: Vec<AnyItem>,
    selected_view: SelectedMainView,
    timeline_view: Entity<TimelineView>,
    calendar_view: Entity<CalendarView>,
    queue_view: Entity<QueueView>,
    focus_view: Entity<FocusView>,
}

impl MainView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let queue = Vec::new();

        let timeline_view = cx.new(|cx| TimelineView::new(window, cx));
        let calendar_view = cx.new(|cx| CalendarView::new(window, cx));
        let queue_view = cx.new(|cx| QueueView::new(window, cx));
        let focus_view = cx.new(FocusView::new);

        let selected_view = Settings::global(cx).desktop_layout.selected_main_view;

        #[cfg(any(target_os = "macos", target_os = "windows"))]
        cx.observe(&calendar_view, |view, _, cx| {
            if view.selected_view == SelectedMainView::Calendar {
                cx.notify();
            }
        })
        .detach();

        cx.subscribe_in(
            &calendar_view,
            window,
            |view, _, request: &OpenDateRangeInTimeline, window, cx| {
                view.timeline_view.update(cx, |timeline, cx| {
                    timeline.zoom_to_date_range(request.0, cx);
                });
                view.select_view(SelectedMainView::Timeline, window, cx);
            },
        )
        .detach();

        cx.subscribe_in(
            &timeline_view,
            window,
            |_, _, request: &InspectItem, _, cx| cx.emit(request.clone()),
        )
        .detach();

        cx.subscribe_in(
            &calendar_view,
            window,
            |_, _, request: &InspectItem, _, cx| cx.emit(request.clone()),
        )
        .detach();

        let db_store = AppDatabaseStore::global(cx);
        cx.subscribe(&db_store, |view, store, _: &DataChanged, cx| {
            let settings = Settings::global(cx);
            let data = MainViewData::read(store.read(cx), &settings);
            view.refresh_from_store(data, cx);
        })
        .detach();

        cx.spawn(async move |view, cx: &mut AsyncApp| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                let result = view.update_in(cx, |_, window, cx| {
                    notifications::update_upcoming(!window.is_window_active(), cx);
                    Ok(())
                });
                if result.is_err() {
                    break;
                };
            }
        })
        .detach();

        let mut view = Self {
            focus_handle,
            queue,
            selected_view,
            timeline_view,
            calendar_view,
            queue_view,
            focus_view,
        };
        let settings = Settings::global(cx);
        let data = MainViewData::read(db_store.read(cx), &settings);
        view.refresh_from_store(data, cx);
        view
    }

    fn refresh_from_store(&mut self, data: MainViewData, cx: &mut Context<Self>) {
        let MainViewData {
            queue,
            focus_actions,
            agenda,
            timeline,
            events,
            markers,
            signals,
        } = data;
        self.queue = queue.clone();
        self.calendar_view.update(cx, |calendar, cx| {
            calendar.refresh_items(timeline.clone(), cx);
            calendar.refresh_markers(markers.clone(), cx);
            calendar.refresh_signals(signals.clone(), cx);
        });
        self.timeline_view.update(cx, |timeline_view, cx| {
            timeline_view.refresh_items(timeline, cx);
            timeline_view.refresh_markers(markers.clone(), cx);
            timeline_view.refresh_signals(signals.clone(), cx);
        });
        self.queue_view.update(cx, |queue_view, cx| {
            queue_view.refresh_items(agenda, cx);
            queue_view.refresh_markers(markers, cx);
            queue_view.refresh_signals(signals.clone(), cx);
        });
        self.focus_view.update(cx, |focus_view, cx| {
            focus_view.refresh_actions(focus_actions, cx);
            focus_view.refresh_temporal(events, signals, cx);
        });
    }

    pub fn selected_view(&self) -> SelectedMainView {
        self.selected_view
    }

    pub(crate) fn view_command_scope<E: InteractiveElement>(
        &self,
        element: E,
        cx: &mut Context<Self>,
    ) -> E {
        match self.selected_view {
            SelectedMainView::Timeline => self
                .timeline_view
                .update(cx, |view, cx| view.view_command_scope(element, cx)),
            SelectedMainView::Calendar => self
                .calendar_view
                .update(cx, |view, cx| view.view_command_scope(element, cx)),
            SelectedMainView::Queue => self
                .queue_view
                .update(cx, |view, cx| view.view_command_scope(element, cx)),
            SelectedMainView::Focus => self
                .focus_view
                .update(cx, |view, cx| view.view_command_scope(element, cx)),
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(crate) fn render_calendar_header(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<(Pixels, AnyElement)> {
        if self.selected_view != SelectedMainView::Calendar {
            return None;
        }
        Some((
            DAY_NAMES_HEIGHT,
            self.calendar_view.read(cx).render_header_foreground(cx),
        ))
    }

    pub fn select_view(
        &mut self,
        view: SelectedMainView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_view_inner(view, window, cx);
    }

    pub(crate) fn select_focus_events(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_view
            .update(cx, |focus, cx| focus.set_mode(FocusMode::Event, cx));
        self.select_view_inner(SelectedMainView::Focus, window, cx);
    }

    pub(crate) fn finish_item_drag(&mut self, cx: &mut Context<Self>) {
        self.timeline_view
            .update(cx, |timeline, cx| timeline.finish_item_drag(cx));
        self.calendar_view
            .update(cx, |calendar, cx| calendar.clear_drop_target(cx));
        self.queue_view
            .update(cx, |queue, cx| queue.clear_drop_target(cx));
        self.focus_view
            .update(cx, |focus, cx| focus.clear_drop_target(cx));
    }

    fn select_view_inner(
        &mut self,
        view: SelectedMainView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_view != view && ItemManager::global(cx).read(cx).is_editing() {
            ItemManager::global(cx).update(cx, |manager, cx| {
                manager.commit_open_edit(window, cx);
            });
        }

        if self.selected_view != view
            && (self.selected_view == SelectedMainView::Timeline
                || view == SelectedMainView::Timeline)
        {
            self.timeline_view.update(cx, |timeline, _| {
                timeline.reset_pointer_intent();
            });
        }
        if self.selected_view == SelectedMainView::Calendar && view != SelectedMainView::Calendar {
            self.calendar_view.update(cx, |calendar, cx| {
                calendar.close_day_inspector(window, cx);
                calendar.clear_drop_target(cx);
            });
        }
        if self.selected_view == SelectedMainView::Queue && view != SelectedMainView::Queue {
            self.queue_view
                .update(cx, |queue, cx| queue.clear_drop_target(cx));
        }
        if self.selected_view == SelectedMainView::Focus && view != SelectedMainView::Focus {
            self.focus_view
                .update(cx, |focus, cx| focus.clear_drop_target(cx));
        }

        if self.selected_view != view {
            let leaving_scope = match self.selected_view {
                SelectedMainView::Timeline => Some(SelectionScope::Timeline),
                SelectedMainView::Calendar => Some(SelectionScope::Calendar),
                SelectedMainView::Queue => Some(SelectionScope::Queue),
                SelectedMainView::Focus => Some(SelectionScope::Focus),
            };
            if leaving_scope.is_some_and(|scope| {
                SelectionManager::global(cx)
                    .read(cx)
                    .has_selection_in(scope)
            }) {
                SelectionManager::clear_global(cx);
            }
        }

        self.selected_view = view;
        cx.emit(SelectedMainViewChanged(view));
        let handle = self.selected_view_focus_handle(cx);
        handle.focus(window, cx);
        if view == SelectedMainView::Focus {
            self.focus_view
                .update(cx, |focus, cx| focus.focus_current(window, cx));
        }
        cx.notify();
    }

    pub(crate) fn selected_view_focus_handle(&self, cx: &App) -> FocusHandle {
        match self.selected_view {
            SelectedMainView::Timeline => self.timeline_view.read(cx).focus_handle(cx),
            SelectedMainView::Calendar => self.calendar_view.read(cx).focus_handle(cx),
            SelectedMainView::Queue => self.queue_view.read(cx).focus_handle(cx),
            SelectedMainView::Focus => self.focus_view.read(cx).focus_handle(cx),
        }
    }

    pub(crate) fn view_scheduled_item(
        &mut self,
        destination: ScheduledItemDestination,
        item_id: Uuid,
        start: SchedulePoint,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected_view = match destination {
            ScheduledItemDestination::Timeline => {
                let item = AppDatabaseStore::global(cx).read(cx).get_item(item_id);
                self.timeline_view.update(cx, |timeline, cx| match item {
                    Some(AnyItem::Signal(_)) | None => {
                        timeline.scroll_to(DateTime::<Local>::from(start), cx)
                    }
                    Some(ref item) => timeline.zoom_to_item(item, cx),
                });
                SelectedMainView::Timeline
            }
            ScheduledItemDestination::Queue => {
                self.queue_view.update(cx, |queue, cx| {
                    queue.scroll_to_item_or_date(item_id, NaiveDate::from(start), cx);
                });
                SelectedMainView::Queue
            }
            ScheduledItemDestination::Calendar => {
                self.calendar_view.update(cx, |calendar, cx| {
                    calendar.scroll_to_date(NaiveDate::from(start), cx);
                });
                SelectedMainView::Calendar
            }
        };
        self.select_view(selected_view, window, cx);
    }

    pub(crate) fn complete_focus_item(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.selected_view != SelectedMainView::Focus
            || !self.focus_view.read(cx).is_action_mode()
        {
            return false;
        }
        self.focus_view
            .update(cx, |focus, cx| focus.complete_active(window, cx));
        true
    }
}

impl EventEmitter<InspectItem> for MainView {}
impl EventEmitter<SelectedMainViewChanged> for MainView {}

impl Focusable for MainView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for MainView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .column()
            .track_focus(&self.focus_handle)
            .size_full()
            .child(div().size_full().map(|this| match self.selected_view {
                SelectedMainView::Timeline => this.child(self.timeline_view.clone()),
                SelectedMainView::Calendar => this.child(self.calendar_view.clone()),
                SelectedMainView::Queue => this.child(self.queue_view.clone()),
                SelectedMainView::Focus => this.child(self.focus_view.clone()),
            }))
    }
}
