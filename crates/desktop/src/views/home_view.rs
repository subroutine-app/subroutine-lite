use std::time::Duration;

use chrono::{DateTime, Local, NaiveDate, Utc};
use gpui::{
    App, Context, Div, ElementId, EventEmitter, FocusHandle, Focusable, FontWeight,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_kit::foundation::{Sizable as _, StyledExt as _};
use gpui_kit_semantics::{NodeSpec, Role, Semantic as _};
use gpui_kit_theme::ActiveTheme as _;
use subroutine_core::{AnyItem, SchedulePoint};

use crate::{
    AppIcon,
    app::ToggleSearch,
    branding,
    components::{Button, ButtonVariants as _, ItemCard, ItemCardStates},
    item_manager::ItemManager,
    selection::{SelectionManager, SelectionOrder, SelectionScope, focus_item},
    settings::{GlobalSettings, Settings},
    stores::{AppDatabaseStore, DataChanged},
};

use super::main_view::{EventMoment, TemporalSnapshot, queue_system::QueueSnapshot};
use super::{StartItemCreator, TOP_EDGE_INSET};

const HOME_PREVIEW_HEIGHT: gpui::Pixels = px(64.);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HomeViewEvent {
    OpenFocusEvents,
}

struct HomeData {
    workspace_generation: u64,
    action: Option<AnyItem>,
    event: Option<EventMoment>,
}

impl HomeData {
    fn read(store: &AppDatabaseStore, settings: &Settings, now: DateTime<Utc>) -> Self {
        let queue = QueueSnapshot::new(
            store.actions(),
            store.events(),
            now,
            settings.schedule.granularity,
        );
        let action = if queue.blocker.is_none() {
            queue.candidates.into_iter().next()
        } else {
            None
        };
        let event = TemporalSnapshot::new(
            store.events(),
            &[],
            now,
            settings.focus_timing_threshold(),
            settings.focus_horizon(),
        )
        .events
        .into_iter()
        .next();

        Self {
            workspace_generation: store.workspace_generation(),
            action,
            event,
        }
    }
}

pub struct HomeView {
    focus_handle: FocusHandle,
    action_focus: FocusHandle,
    details: ItemCardStates,
    workspace_generation: u64,
    action: Option<AnyItem>,
    event: Option<EventMoment>,
}

impl HomeView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = AppDatabaseStore::global(cx);
        cx.observe(&store, |view, store, cx| {
            view.sync_workspace(store.read(cx).workspace_generation(), cx);
        })
        .detach();
        cx.subscribe(&store, |view, store, _: &DataChanged, cx| {
            let settings = Settings::global(cx);
            view.refresh(HomeData::read(store.read(cx), &settings, Utc::now()), cx);
        })
        .detach();

        cx.observe(&ItemManager::global(cx), |_, _, cx| cx.notify())
            .detach();

        cx.observe_global::<GlobalSettings>(|view, cx| {
            let store = AppDatabaseStore::global(cx);
            let settings = Settings::global(cx);
            view.refresh(HomeData::read(store.read(cx), &settings, Utc::now()), cx);
        })
        .detach();

        cx.spawn_in(window, async move |view, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if view
                    .update_in(cx, |view, _, cx| {
                        let store = AppDatabaseStore::global(cx);
                        let settings = Settings::global(cx);
                        view.refresh(HomeData::read(store.read(cx), &settings, Utc::now()), cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        let settings = Settings::global(cx);
        let data = HomeData::read(store.read(cx), &settings, Utc::now());
        Self {
            focus_handle: cx.focus_handle(),
            action_focus: cx.focus_handle(),
            details: ItemCardStates::default(),
            workspace_generation: data.workspace_generation,
            action: data.action,
            event: data.event,
        }
    }

    fn sync_workspace(&mut self, generation: u64, cx: &mut Context<Self>) {
        if self.workspace_generation == generation {
            return;
        }
        self.workspace_generation = generation;
        self.details = ItemCardStates::default();
        cx.notify();
    }

    fn refresh(&mut self, data: HomeData, cx: &mut Context<Self>) {
        self.sync_workspace(data.workspace_generation, cx);
        self.action = data.action;
        self.event = data.event;
        self.retain_details();
        cx.notify();
    }

    fn retain_details(&self) {
        self.details.retain(
            self.action
                .iter()
                .map(AnyItem::id)
                .chain(self.event.iter().map(EventMoment::notice_id)),
        );
    }

    fn item_preview(
        &self,
        element_id: impl Into<ElementId>,
        item: &AnyItem,
        meta: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ItemCard {
        ItemCard::new_with_id(element_id, item, meta, window, cx)
            .preview()
            .schedule_navigation()
            .large_title(true)
            .w_full()
            .flex_none()
            .draggable(true, None)
            .block_mouse_except_scroll()
    }

    fn action_preview(
        &self,
        item: &AnyItem,
        order: SelectionOrder,
        now: DateTime<Local>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let meta = item.start().map(|start| match start {
            SchedulePoint::Date(date) => {
                format!("{} · Any time", relative_date(date, now.date_naive()))
            }
            SchedulePoint::DateTime(start) => {
                let start = start.with_timezone(&Local);
                format!(
                    "{} · {}",
                    relative_date(start.date_naive(), now.date_naive()),
                    format_clock_time(start),
                )
            }
        });
        let card = self
            .item_preview(
                ("home-action", item.id_u64()),
                item,
                meta.map(Into::into),
                window,
                cx,
            )
            .selectable(order)
            .with_focus_handle(self.action_focus.clone())
            .details(self.details.get(item.id()))
            .h(self.details.height(item.id(), HOME_PREVIEW_HEIGHT));

        div()
            .column()
            .gap_1()
            .child(home_caption("NEXT IN QUEUE", cx).px(px(10.)))
            .child(card)
    }

    fn event_preview(
        &self,
        moment: &EventMoment,
        order: SelectionOrder,
        now: DateTime<Local>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let item = AnyItem::Event(moment.card_event().clone());
        let card = self
            .item_preview(
                ("home-event", moment.notice_id().as_u64_pair().1),
                &item,
                None,
                window,
                cx,
            )
            .details(self.details.get(moment.notice_id()))
            .h(self.details.height(moment.notice_id(), HOME_PREVIEW_HEIGHT));
        let (label, timing, card) = match moment {
            EventMoment::InProgress { remaining, .. } => {
                let view = cx.entity().downgrade();
                let card = card.editable(false).on_click(move |_, _, cx| {
                    view.update(cx, |_, cx| cx.emit(HomeViewEvent::OpenFocusEvents))
                        .ok();
                });
                (
                    "NOW".to_owned(),
                    format!("Ends in {}", preview_duration(*remaining)),
                    card,
                )
            }
            EventMoment::Upcoming { until, .. } => {
                let start = moment.event().start.with_timezone(&Local);
                let time = format_clock_time(start);
                let label = if start.date_naive() == now.date_naive() {
                    time
                } else {
                    format!(
                        "{} · {time}",
                        relative_date(start.date_naive(), now.date_naive())
                    )
                };
                (
                    label,
                    format!("In {}", preview_duration(*until)),
                    card.selectable(order),
                )
            }
        };
        let paint = moment.paint(cx.theme());
        let current = matches!(moment, EventMoment::InProgress { .. });

        div()
            .column()
            .gap_1()
            .child(
                div()
                    .row()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px(px(10.))
                    .child(
                        home_caption(label, cx)
                            .row()
                            .items_center()
                            .gap_2()
                            .when(current, |label| label.child(paint.marker(cx.theme()))),
                    )
                    .child(home_caption(timing, cx)),
            )
            .child(card)
    }
}

impl EventEmitter<HomeViewEvent> for HomeView {}

impl Focusable for HomeView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for HomeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.retain_details();
        self.details.animate(window, cx);
        let now = Local::now();
        let event_first = matches!(self.event, Some(EventMoment::InProgress { .. }));
        let action_id = self.action.as_ref().map(AnyItem::id);
        let event_id = self.event.as_ref().map(|moment| moment.card_event().id);
        let ids = if event_first {
            [event_id, action_id]
        } else {
            [action_id, event_id]
        };
        let order = SelectionOrder::new(SelectionScope::Home, ids.into_iter().flatten());
        SelectionManager::report_order(&order, cx);
        if let Some(request) =
            SelectionManager::take_completion_focus(SelectionScope::Home, window, cx)
        {
            let replacement = self.action.as_ref().filter(|item| {
                let manager = ItemManager::global(cx);
                let manager = manager.read(cx);
                matches!(item, AnyItem::Action(action) if !action.is_completed())
                    && !manager.is_draft(item.id())
                    && !manager.is_completing(item.id())
                    && request.target.is_none_or(|target| target == item.id())
            });
            if let Some(item) = replacement {
                focus_item(request.scope, item.id(), &self.action_focus, window, cx);
            } else {
                self.focus_handle.focus(window, cx);
            }
        }

        let action = self
            .action
            .as_ref()
            .map(|item| self.action_preview(item, order.clone(), now, window, cx));
        let event = self
            .event
            .as_ref()
            .map(|moment| self.event_preview(moment, order, now, window, cx));
        let previews = if event_first {
            [event, action]
        } else {
            [action, event]
        };
        let previews = previews
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(index, preview)| {
                preview.when(index > 0, |preview| {
                    preview
                        .pt_5()
                        .border_t_1()
                        .border_color(cx.theme().colors.hairline.opacity(0.5))
                })
            })
            .collect::<Vec<_>>();

        div()
            .id("home-view")
            .track_focus(&self.focus_handle)
            .relative()
            .size_full()
            .overflow_hidden()
            .semantic_in(cx, NodeSpec::new("home-view", Role::Region).text("Home"))
            .child(
                div()
                    .id("home-scroll")
                    .relative()
                    .size_full()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .flex()
                            .min_h_full()
                            .w_full()
                            .items_center()
                            .justify_center()
                            .px_8()
                            .pt(px(112.))
                            .pb(px(176.))
                            .child(
                                div()
                                    .column()
                                    .w_full()
                                    .min_w_0()
                                    .max_w(px(560.))
                                    .gap_10()
                                    .child(home_clock(now, cx).px(px(10.)))
                                    .when(!previews.is_empty(), |content| {
                                        content.child(
                                            div()
                                                .id("home-previews")
                                                .column()
                                                .w_full()
                                                .flex_none()
                                                .gap_4()
                                                .children(previews),
                                        )
                                    }),
                            ),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top(TOP_EDGE_INSET)
                    .right_8()
                    .row()
                    .gap_2()
                    .child(home_search_button().compact().size_10().rounded_full())
                    .child(home_new_item_button().compact().size_10().rounded_full()),
            )
    }
}

fn home_caption(label: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.theme().colors.text_muted)
        .child(label.into())
}

fn home_clock(now: DateTime<Local>, cx: &App) -> Div {
    let date = now.format("%A, %B %-d").to_string();
    let time = format_clock_time(now);
    div()
        .column()
        .font_family(branding::FONT_FAMILY)
        .gap_1()
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().colors.text_muted)
                .child(date.clone()),
        )
        .child(
            div()
                .id("home-heading")
                .text_size(px(36.))
                .line_height(px(44.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(time.clone())
                .semantic_in(
                    cx,
                    NodeSpec::new("home-heading", Role::Heading)
                        .level(1)
                        .text(format!("{date}, {time}")),
                ),
        )
}

fn home_new_item_button() -> Button {
    Button::new("home-new-item")
        .primary()
        .small()
        .icon(AppIcon::Plus)
        .tooltip("New item")
        .on_click(|_, window, cx| {
            window.dispatch_action(Box::new(StartItemCreator), cx);
        })
}

fn home_search_button() -> Button {
    Button::new("home-search")
        .ghost()
        .small()
        .icon(AppIcon::Search)
        .tooltip("Search all items")
        .on_click(|_, window, cx| {
            window.dispatch_action(Box::new(ToggleSearch), cx);
        })
}

fn preview_duration(duration: chrono::Duration) -> String {
    let minutes = duration.num_minutes().max(0);
    match (minutes / 60, minutes % 60) {
        (0, ..=0) => "<1 min".to_owned(),
        (0, minutes) => format!("{minutes} min"),
        (hours, 0) => format!("{hours} hr"),
        (hours, minutes) => format!("{hours} hr {minutes} min"),
    }
}

fn relative_date(date: NaiveDate, today: NaiveDate) -> String {
    if date == today {
        "Today".to_owned()
    } else if date == today.succ_opt().unwrap_or(today) {
        "Tomorrow".to_owned()
    } else {
        date.format("%A, %b %-d").to_string()
    }
}

fn format_clock_time(time: DateTime<Local>) -> String {
    time.format("%-I:%M %P").to_string()
}
