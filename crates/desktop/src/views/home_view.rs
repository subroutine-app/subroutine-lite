use std::time::Duration;

use chrono::{DateTime, Local, NaiveDate, Utc};
use gpui::{
    App, Context, Div, ElementId, EventEmitter, FocusHandle, Focusable, FontWeight,
    InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled,
    Window, div, prelude::FluentBuilder as _, px,
};
use gpui_kit::foundation::{Sizable as _, StyledExt as _};
use gpui_kit_semantics::{NodeSpec, Role, Semantic as _};
use gpui_kit_theme::ActiveTheme as _;
use subroutine_core::{Action, AnyItem, Event, ItemType, SchedulePoint};

use crate::{
    AppIcon,
    app::ToggleSearch,
    components::{Button, ButtonVariants as _, ItemCard, ItemCardStates},
    icons::Icon,
    item_manager::ItemManager,
    selection::{SelectionOrder, SelectionScope},
    settings::{GlobalSettings, Settings},
    stores::{AppDatabaseStore, DataChanged},
};

use super::main_view::{EventMoment, TemporalSnapshot, event_notice_card, ordered_focus_actions};
use super::{StartItemCreator, TOP_EDGE_INSET};

const HOME_CARD_HEIGHT: gpui::Pixels = px(112.);
const HOME_CARD_PREFERRED_WIDTH: gpui::Pixels = px(300.);

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
        let action = ordered_focus_actions(store.actions(), now, settings.focus_action_horizon())
            .into_iter()
            .next();
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
    details: ItemCardStates,
    workspace_generation: u64,
    action: Option<AnyItem>,
    event: Option<EventMoment>,
    draft: Option<AnyItem>,
}

impl HomeView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = AppDatabaseStore::global(cx);
        cx.observe_in(&store, window, |view, store, window, cx| {
            view.sync_workspace(store.read(cx).workspace_generation(), window, cx);
        })
        .detach();
        cx.subscribe_in(
            &store,
            window,
            |view, store, _: &DataChanged, window, cx| {
                let settings = Settings::global(cx);
                view.refresh(
                    HomeData::read(store.read(cx), &settings, Utc::now()),
                    window,
                    cx,
                );
            },
        )
        .detach();

        cx.observe(&ItemManager::global(cx), |view, manager, cx| {
            if let Some(draft) = view.draft.as_ref() {
                match manager.read(cx).draft_item(draft.id()) {
                    Some(item) if item.item_type() != draft.item_type() => {
                        view.draft = Some(item.clone());
                    }
                    None => view.draft = None,
                    _ => {}
                }
            }
            cx.notify();
        })
        .detach();

        cx.observe_global_in::<GlobalSettings>(window, |view, window, cx| {
            let store = AppDatabaseStore::global(cx);
            let settings = Settings::global(cx);
            view.refresh(
                HomeData::read(store.read(cx), &settings, Utc::now()),
                window,
                cx,
            );
        })
        .detach();

        cx.spawn_in(window, async move |view, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if view
                    .update_in(cx, |view, window, cx| {
                        let store = AppDatabaseStore::global(cx);
                        let settings = Settings::global(cx);
                        view.refresh(
                            HomeData::read(store.read(cx), &settings, Utc::now()),
                            window,
                            cx,
                        );
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
            details: ItemCardStates::default(),
            workspace_generation: data.workspace_generation,
            action: data.action,
            event: data.event,
            draft: None,
        }
    }

    fn sync_workspace(&mut self, generation: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace_generation == generation {
            return;
        }
        if let Some(draft) = self.draft.take() {
            ItemManager::global(cx).update(cx, |manager, cx| {
                if manager.is_draft(draft.id()) {
                    manager.discard_edit(window, cx);
                }
            });
        }
        self.workspace_generation = generation;
        self.details = ItemCardStates::default();
        cx.notify();
    }

    fn refresh(&mut self, data: HomeData, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_workspace(data.workspace_generation, window, cx);
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
                .chain(self.event.iter().map(EventMoment::notice_id))
                .chain(self.draft.iter().map(AnyItem::id)),
        );
    }

    fn item_card(
        element_id: impl Into<ElementId>,
        item: AnyItem,
        meta: String,
        order: SelectionOrder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ItemCard {
        ItemCard::new_with_id(element_id, &item, Some(meta.into()), window, cx)
            .schedule_navigation()
            .selectable(order)
            .large_title(true)
            .w_full()
            .flex_none()
            .border(false)
            .draggable(true, None)
            .block_mouse_except_scroll()
    }

    fn event_notice(
        &self,
        moment: &EventMoment,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ItemCard {
        let view = cx.entity().downgrade();
        event_notice_card(
            ("home-event-notice", moment.notice_id().as_u64_pair().1),
            moment,
            window,
            cx,
        )
        .large_title(true)
        .w_full()
        .flex_none()
        .border(false)
        .draggable(true, None)
        .on_click(move |_, _, cx| {
            view.update(cx, |_view, cx| {
                cx.emit(HomeViewEvent::OpenFocusEvents);
            })
            .ok();
        })
    }

    fn begin_draft(&mut self, item: AnyItem, window: &mut Window, cx: &mut Context<Self>) {
        let manager = ItemManager::global(cx);
        manager.update(cx, |manager, cx| {
            manager.commit_open_edit(window, cx);
        });
        if manager.read(cx).is_editing() {
            return;
        }
        self.focus_handle.focus(window, cx);
        manager.update(cx, |manager, cx| {
            manager.begin_edit(&item, true, window, cx);
        });
        self.draft = Some(item);
        cx.notify();
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
        let action = self
            .draft
            .as_ref()
            .filter(|item| item.item_type() == ItemType::Action)
            .or(self.action.as_ref())
            .cloned();
        let event_draft = self
            .draft
            .as_ref()
            .filter(|item| item.item_type() == ItemType::Event)
            .cloned();
        let action_height = action.as_ref().map_or(HOME_CARD_HEIGHT, |item| {
            self.details.height(item.id(), HOME_CARD_HEIGHT)
        });
        let event_height = event_draft
            .as_ref()
            .map(AnyItem::id)
            .or_else(|| self.event.as_ref().map(EventMoment::notice_id))
            .map_or(HOME_CARD_HEIGHT, |id| {
                self.details.height(id, HOME_CARD_HEIGHT)
            });
        let now = Local::now();
        let root = div()
            .id("home-view")
            .track_focus(&self.focus_handle)
            .size_full()
            .overflow_y_scroll()
            .semantic_in(cx, NodeSpec::new("home-view", Role::Region).text("Home"));

        let order = SelectionOrder::new(
            SelectionScope::Home,
            self.action
                .iter()
                .map(AnyItem::id)
                .chain(self.event.iter().map(|moment| moment.card_event().id)),
        );
        let action_card = action.map(|item| {
            let meta = format_item_meta(&item, now);
            let details = self.details.get(item.id());
            Self::item_card(
                ("home-action", item.id_u64()),
                item,
                meta,
                order.clone(),
                window,
                cx,
            )
            .details(details)
            .h(action_height)
            .into_any_element()
        });

        let event_badge = self
            .event
            .as_ref()
            .filter(|_| event_draft.is_none())
            .and_then(|moment| {
                event_context_label(moment)
                    .map(|label| moment.paint(cx.theme()).badge(label, cx.theme()))
            });
        let event_card = if let Some(item) = event_draft {
            let meta = format_item_meta(&item, now);
            Some(
                Self::item_card(
                    ("home-event", item.id_u64()),
                    item,
                    meta,
                    order.clone(),
                    window,
                    cx,
                )
                .h(event_height)
                .into_any_element(),
            )
        } else {
            self.event.clone().map(|moment| {
                let card = match &moment {
                    EventMoment::InProgress { .. } => self.event_notice(&moment, window, cx),
                    EventMoment::Upcoming { .. } => {
                        let item = AnyItem::Event(moment.card_event().clone());
                        let occurrence = AnyItem::Event(moment.event().clone());
                        let meta = format_item_meta(&occurrence, now);
                        Self::item_card(
                            ("home-event", moment.notice_id().as_u64_pair().1),
                            item,
                            meta,
                            order.clone(),
                            window,
                            cx,
                        )
                    }
                };
                card.details(self.details.get(moment.notice_id()))
                    .h(event_height)
                    .into_any_element()
            })
        };

        let cards = div()
            .row()
            .flex_wrap()
            .w_full()
            .items_start()
            .gap_6()
            .child(
                div()
                    .column()
                    .flex_1()
                    .flex_basis(HOME_CARD_PREFERRED_WIDTH)
                    .min_w_0()
                    .gap_3()
                    .child(
                        div()
                            .h_6()
                            .flex_none()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(cx.theme().colors.text_muted)
                            .row()
                            .items_center()
                            .gap_2()
                            .child(div().flex_none().child("Next action"))
                            .child(home_section_rule(cx))
                            .when(action_card.is_none(), |header| {
                                header.child(home_section_add_button(
                                    "home-new-action",
                                    "New action",
                                    cx.listener(|view, _, window, cx| {
                                        view.begin_draft(
                                            AnyItem::Action(Action::new("").with_queued(true)),
                                            window,
                                            cx,
                                        );
                                    }),
                                    cx,
                                ))
                            }),
                    )
                    .children(action_card),
            )
            .child(
                div()
                    .column()
                    .flex_1()
                    .flex_basis(HOME_CARD_PREFERRED_WIDTH)
                    .min_w_0()
                    .gap_3()
                    .child(
                        div()
                            .h_6()
                            .flex_none()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(cx.theme().colors.text_muted)
                            .row()
                            .items_center()
                            .gap_2()
                            .child(div().flex_none().child("Next event"))
                            .children(event_badge)
                            .child(home_section_rule(cx))
                            .when(event_card.is_none(), |header| {
                                header.child(home_section_add_button(
                                    "home-new-event",
                                    "New event",
                                    cx.listener(|view, _, window, cx| {
                                        view.begin_draft(
                                            AnyItem::Event(Event::new(
                                                "",
                                                Utc::now(),
                                                chrono::Duration::minutes(60),
                                            )),
                                            window,
                                            cx,
                                        );
                                    }),
                                    cx,
                                ))
                            }),
                    )
                    .children(event_card),
            );

        root.child(
            div()
                .relative()
                .flex()
                .min_h_full()
                .w_full()
                .items_center()
                .justify_center()
                .px_8()
                .pt(px(128.))
                .pb(px(192.))
                .child(home_clock(now, cx).absolute().top(TOP_EDGE_INSET).left_8())
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
                .child(
                    div()
                        .column()
                        .w_full()
                        .max_w(px(720.))
                        .gap_8()
                        .child(
                            div()
                                .id("home-heading")
                                .text_center()
                                .text_size(px(42.))
                                .line_height(px(48.))
                                .font_weight(FontWeight::MEDIUM)
                                .child("Welcome back")
                                .semantic_in(
                                    cx,
                                    NodeSpec::new("home-heading", Role::Heading)
                                        .level(1)
                                        .text("Welcome back"),
                                ),
                        )
                        .child(div().id("home-cards").w_full().flex_none().child(cards)),
                ),
        )
    }
}

fn home_section_rule(cx: &App) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .h(px(1.))
        .bg(cx.theme().colors.hairline.opacity(0.5))
}

fn home_clock(now: DateTime<Local>, cx: &App) -> Div {
    div()
        .column()
        .gap_1()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().colors.text_muted)
                .child(now.format("%A, %B %-d").to_string()),
        )
        .child(
            div()
                .text_size(px(32.))
                .line_height(px(36.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(now.format("%-I:%M %p").to_string()),
        )
}

fn home_section_add_button(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    div()
        .flex_none()
        .size_6()
        .child(
            Button::new(id)
                .ghost()
                .compact()
                .size_6()
                .rounded_full()
                .text_color(cx.theme().colors.text_muted)
                .icon(Icon::new(AppIcon::Plus).size_4())
                .tooltip(label)
                .on_click(on_click),
        )
        .semantic_in(cx, NodeSpec::new(id, Role::Button).text(label))
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

fn event_context_label(moment: &EventMoment) -> Option<&'static str> {
    match moment {
        EventMoment::InProgress { .. } => Some("In progress"),
        EventMoment::Upcoming {
            within_threshold: true,
            ..
        } => Some("Starts soon"),
        EventMoment::Upcoming {
            within_threshold: false,
            ..
        } => None,
    }
}

fn format_item_meta(item: &AnyItem, now: DateTime<Local>) -> String {
    if let AnyItem::Event(event) = item {
        let start = event.start.with_timezone(&Local);
        let end = match event.end_time() {
            Ok(end) => end.with_timezone(&Local),
            Err(_) => return String::new(),
        };
        if start <= now && now < end {
            return format!("Now · ends {}", format_clock_time(end));
        }
    }

    match item.start() {
        Some(SchedulePoint::Date(date)) => {
            format!("{} · Any time", relative_date(date, now.date_naive()))
        }
        Some(SchedulePoint::DateTime(start)) => {
            let start = start.with_timezone(&Local);
            format!(
                "{} · {}",
                relative_date(start.date_naive(), now.date_naive()),
                format_clock_time(start)
            )
        }
        None => "Unscheduled".to_owned(),
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
    time.format("%-I:%M%P").to_string().replace(":00", "")
}
