use chrono::{DateTime, Duration, Local, Utc};
use gpui::{
    AnyElement, App, Div, FontWeight, Hsla, InteractiveElement, IntoElement, ParentElement, Styled,
    Window, div, prelude::FluentBuilder as _, px, relative,
};
use gpui_kit::{display::badge::Badge, foundation::StyledExt as _};
use gpui_kit_semantics::{NodeSpec, Role, Semantic as _};
use gpui_kit_theme::{ActiveTheme as _, Space, Theme, Variant};
use subroutine_core::{AnyItem, Event, Signal};

use crate::{components::ItemCard, presentation::UxColor};

pub(super) const SIGNAL_RECENT_DURATION: Duration = Duration::minutes(2);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MomentPaint {
    role: UxColor,
    pub(crate) marker: Hsla,
    pub(crate) text: Hsla,
    wash: Hsla,
}

impl MomentPaint {
    fn new(role: UxColor, theme: &Theme) -> Self {
        Self {
            role,
            marker: role.color(theme),
            text: role.text(theme),
            wash: role.wash(theme),
        }
    }

    pub(crate) fn marker(self, theme: &Theme) -> Div {
        div()
            .flex_none()
            .size(px(theme.measures.status_mark * 0.5))
            .rounded_full()
            .bg(self.marker)
    }

    pub(crate) fn badge(self, label: &'static str, theme: &Theme) -> Div {
        div()
            .row()
            .items_center()
            .rounded_full()
            .bg(self.wash)
            .when(self.role == UxColor::Attention, |badge| {
                badge
                    .pl(px(theme.space(Space::Xs)))
                    .child(self.marker(theme))
            })
            .child(
                Badge::new(label)
                    .tint(self.text)
                    .variant(Variant::Transparent),
            )
    }
}

#[derive(Clone, Debug)]
pub(crate) enum EventMoment {
    InProgress {
        event: Event,
        card_event: Event,
        progress: f32,
        remaining: Duration,
    },
    Upcoming {
        event: Event,
        card_event: Event,
        until: Duration,
        within_threshold: bool,
    },
}

impl EventMoment {
    pub(crate) fn paint(&self, theme: &Theme) -> MomentPaint {
        let role = match self {
            Self::InProgress { .. } => UxColor::Action,
            Self::Upcoming {
                within_threshold: true,
                ..
            } => UxColor::Attention,
            Self::Upcoming {
                within_threshold: false,
                ..
            } => UxColor::Neutral,
        };
        MomentPaint::new(role, theme)
    }

    pub(crate) fn event(&self) -> &Event {
        match self {
            Self::InProgress { event, .. } | Self::Upcoming { event, .. } => event,
        }
    }

    pub(crate) fn card_event(&self) -> &Event {
        match self {
            Self::InProgress { card_event, .. } | Self::Upcoming { card_event, .. } => card_event,
        }
    }

    pub(crate) fn notice_id(&self) -> uuid::Uuid {
        self.event().id
    }
}

#[derive(Clone, Debug)]
pub(super) enum SignalMoment {
    Upcoming {
        notice_id: uuid::Uuid,
        card_signal: Signal,
        until: Duration,
    },
    Recent {
        notice_id: uuid::Uuid,
        card_signal: Signal,
        elapsed: Duration,
    },
}

impl SignalMoment {
    pub(super) fn paint(&self, theme: &Theme) -> MomentPaint {
        let role = match self {
            Self::Upcoming { .. } => UxColor::Attention,
            Self::Recent { .. } => UxColor::Context,
        };
        MomentPaint::new(role, theme)
    }

    pub(super) fn card_signal(&self) -> &Signal {
        match self {
            Self::Upcoming { card_signal, .. } | Self::Recent { card_signal, .. } => card_signal,
        }
    }

    pub(super) fn notice_id(&self) -> uuid::Uuid {
        match self {
            Self::Upcoming { notice_id, .. } | Self::Recent { notice_id, .. } => *notice_id,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TemporalSnapshot {
    pub(crate) events: Vec<EventMoment>,
    pub(super) signal: Option<SignalMoment>,
}

impl TemporalSnapshot {
    pub(crate) fn new(
        events: &[Event],
        signals: &[Signal],
        now: DateTime<Utc>,
        threshold: Duration,
        horizon: Duration,
    ) -> Self {
        Self {
            events: event_moments(events, now, threshold, horizon),
            signal: signal_moment(signals, now, threshold),
        }
    }

    pub(super) fn event_notice(&self) -> Option<&EventMoment> {
        if let Some(moment) = self
            .events
            .iter()
            .filter(|moment| matches!(moment, EventMoment::InProgress { .. }))
            .min_by_key(|moment| moment.event().end_time())
        {
            return Some(moment);
        }

        match self.events.first()? {
            moment @ EventMoment::Upcoming {
                within_threshold: true,
                ..
            } => Some(moment),
            EventMoment::Upcoming { .. } | EventMoment::InProgress { .. } => None,
        }
    }
}

fn event_moments(
    events: &[Event],
    now: DateTime<Utc>,
    threshold: Duration,
    horizon: Duration,
) -> Vec<EventMoment> {
    let horizon_end = now + horizon;
    let candidates = event_candidates(events, now, horizon_end);
    let mut active: Vec<_> = candidates
        .iter()
        .filter(|event| event.start <= now && now < event.end_time())
        .collect();
    active.sort_by_key(|event| (event.start, event.end_time(), event.id));

    if !active.is_empty() {
        return active
            .into_iter()
            .map(|event| in_progress_moment(events, event, now))
            .collect();
    }

    let Some(anchor) = candidates
        .iter()
        .filter(|event| event.start > now && event.start <= horizon_end)
        .min_by_key(|event| (event.start, event.id))
    else {
        return Vec::new();
    };

    let mut simultaneous: Vec<_> = candidates
        .iter()
        .filter(|event| {
            event.start > now
                && event.start <= horizon_end
                && (event.start == anchor.start
                    || (event.start < anchor.end_time() && event.end_time() > anchor.start))
        })
        .collect();
    simultaneous.sort_by_key(|event| (event.start, event.end_time(), event.id));
    simultaneous
        .into_iter()
        .map(|event| upcoming_moment(events, event, now, threshold))
        .collect()
}

pub(super) fn event_carousel_moments(
    events: &[Event],
    now: DateTime<Utc>,
    threshold: Duration,
    horizon: Duration,
) -> Vec<EventMoment> {
    let horizon_end = now + horizon;
    let mut candidates = event_candidates(events, now, horizon_end);
    candidates.retain(|event| {
        (event.start <= now && now < event.end_time())
            || (event.start > now && event.start <= horizon_end)
    });
    candidates.sort_by_key(|event| (event.start, event.end_time(), event.id));
    candidates
        .iter()
        .map(|event| {
            if event.start <= now {
                in_progress_moment(events, event, now)
            } else {
                upcoming_moment(events, event, now, threshold)
            }
        })
        .collect()
}

fn in_progress_moment(events: &[Event], event: &Event, now: DateTime<Utc>) -> EventMoment {
    let total = (event.end_time() - event.start).num_milliseconds();
    let elapsed = (now - event.start).num_milliseconds();
    let progress = if total <= 0 {
        1.0
    } else {
        (elapsed as f32 / total as f32).clamp(0.0, 1.0)
    };
    EventMoment::InProgress {
        event: event.clone(),
        card_event: stored_event_for_occurrence(events, event),
        progress,
        remaining: event.end_time() - now,
    }
}

fn upcoming_moment(
    events: &[Event],
    event: &Event,
    now: DateTime<Utc>,
    threshold: Duration,
) -> EventMoment {
    let until = event.start - now;
    EventMoment::Upcoming {
        event: event.clone(),
        card_event: stored_event_for_occurrence(events, event),
        until,
        within_threshold: until <= threshold,
    }
}

fn stored_event_for_occurrence(events: &[Event], occurrence: &Event) -> Event {
    events
        .iter()
        .find(|stored| stored.id == occurrence.id)
        .or_else(|| {
            events.iter().find(|stored| {
                stored.source_provider.is_none()
                    && stored.lineage_id == occurrence.lineage_id
                    && stored.recurrence.is_some()
            })
        })
        .cloned()
        .unwrap_or_else(|| occurrence.clone())
}

fn event_candidates(
    events: &[Event],
    now: DateTime<Utc>,
    horizon_end: DateTime<Utc>,
) -> Vec<Event> {
    let start_date = now.with_timezone(&Local).date_naive();
    let end_date = horizon_end.with_timezone(&Local).date_naive();
    let mut candidates = events.to_vec();

    for event in events
        .iter()
        .filter(|event| event.source_provider.is_none() && event.recurrence.is_some())
    {
        candidates.extend(
            AnyItem::Event(event.clone())
                .projections_between(start_date, end_date)
                .into_iter()
                .filter_map(|item| match item {
                    AnyItem::Event(event) => Some(event),
                    _ => None,
                })
                .filter(|projected| {
                    !events.iter().any(|stored| {
                        stored.lineage_id == projected.lineage_id && stored.start == projected.start
                    })
                }),
        );
    }

    candidates.sort_by_key(|event| (event.start, event.lineage_id, event.id));
    candidates.dedup_by_key(|event| (event.start, event.lineage_id));
    candidates
}

fn signal_moment(
    signals: &[Signal],
    now: DateTime<Utc>,
    threshold: Duration,
) -> Option<SignalMoment> {
    let start = now - SIGNAL_RECENT_DURATION;
    let end = now + threshold;
    let mut candidates: Vec<Signal> = signals
        .iter()
        .filter(|signal| signal.datetime >= start && signal.datetime <= end)
        .cloned()
        .collect();

    for signal in signals {
        candidates.extend(
            signal
                .projections_between(start, end)
                .into_iter()
                .filter(|projected| {
                    !signals.iter().any(|stored| {
                        stored.lineage_id == projected.lineage_id
                            && stored.datetime == projected.datetime
                    })
                }),
        );
    }

    candidates.sort_by_key(|signal| (signal.datetime, signal.lineage_id));
    candidates.dedup_by_key(|signal| (signal.datetime, signal.lineage_id));

    if let Some(signal) = candidates
        .iter()
        .filter(|signal| signal.datetime <= now)
        .max_by_key(|signal| (signal.datetime, signal.id))
    {
        return Some(SignalMoment::Recent {
            notice_id: signal.id,
            card_signal: stored_signal_for_occurrence(signals, signal),
            elapsed: now - signal.datetime,
        });
    }

    candidates
        .into_iter()
        .filter(|signal| signal.datetime > now)
        .min_by_key(|signal| (signal.datetime, signal.id))
        .map(|signal| {
            let card_signal = stored_signal_for_occurrence(signals, &signal);
            SignalMoment::Upcoming {
                notice_id: signal.id,
                until: signal.datetime - now,
                card_signal,
            }
        })
}

fn stored_signal_for_occurrence(signals: &[Signal], occurrence: &Signal) -> Signal {
    signals
        .iter()
        .find(|stored| stored.id == occurrence.id)
        .or_else(|| {
            signals.iter().find(|stored| {
                stored.lineage_id == occurrence.lineage_id && stored.recurrence.is_some()
            })
        })
        .cloned()
        .unwrap_or_else(|| occurrence.clone())
}

pub(crate) fn format_countdown(duration: Duration) -> String {
    let milliseconds = duration.num_milliseconds().max(0);
    let seconds = (milliseconds + 999) / 1_000;
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

pub(super) fn format_threshold(seconds: i64) -> String {
    let minutes = seconds / 60;
    if minutes >= 60 && minutes % 60 == 0 {
        let hours = minutes / 60;
        format!("{hours} {}", if hours == 1 { "hour" } else { "hours" })
    } else {
        format!("{minutes} min")
    }
}

pub(super) fn format_horizon(hours: u16) -> String {
    if hours >= 24 && hours.is_multiple_of(24) {
        let days = hours / 24;
        format!("{days} {}", if days == 1 { "day" } else { "days" })
    } else {
        format!("{hours} {}", if hours == 1 { "hour" } else { "hours" })
    }
}

fn format_event_schedule(event: &Event, now: DateTime<Utc>) -> String {
    let start = event.start.with_timezone(&Local);
    let end = event.end_time().with_timezone(&Local);
    let today = now.with_timezone(&Local).date_naive();
    let date = if start.date_naive() == today {
        "Today".to_owned()
    } else if start.date_naive() == today.succ_opt().unwrap_or(today) {
        "Tomorrow".to_owned()
    } else {
        start.format("%a, %b %-d").to_string()
    };
    let start_time = super::super::format_item_time(start);
    let end_time = super::super::format_item_time(end);
    if start.date_naive() == end.date_naive() {
        format!("{date} · {start_time}–{end_time}")
    } else {
        format!(
            "{date} at {start_time} – {} at {end_time}",
            end.format("%a, %b %-d")
        )
    }
}

pub(super) fn event_carousel_card(
    moment: &EventMoment,
    now: DateTime<Utc>,
    window: &mut Window,
    cx: &mut App,
) -> ItemCard {
    let event = moment.event();
    let schedule = format_event_schedule(event, now);
    let item = AnyItem::Event(moment.card_event().clone());
    ItemCard::new_with_id(
        format!("focus-event-card.{}", event.id),
        &item,
        Some(schedule.into()),
        window,
        cx,
    )
    .editable(false)
    .display_title()
    .size_full()
}

pub(super) fn event_carousel_status(
    moment: &EventMoment,
    stacked: bool,
    cx: &mut App,
) -> AnyElement {
    let event = moment.event();
    let event_id = event.id;
    let paint = moment.paint(cx.theme());
    let (label, countdown, progress) = match moment {
        EventMoment::InProgress {
            progress,
            remaining,
            ..
        } => (
            "In progress",
            Some(("Ends in", format_countdown(*remaining))),
            Some(*progress),
        ),
        EventMoment::Upcoming {
            until,
            within_threshold,
            ..
        } => (
            "Upcoming",
            within_threshold.then(|| ("Starts in", format_countdown(*until))),
            None,
        ),
    };
    let semantic_id = format!("focus-event-status.{event_id}");
    let semantic_text = format!(
        "{label} event. {}.{}",
        event.title,
        countdown
            .as_ref()
            .map(|(prefix, countdown)| format!(" {prefix} {countdown}."))
            .unwrap_or_default(),
    );

    div()
        .id(semantic_id.clone())
        .w_full()
        .min_w_0()
        .column()
        .gap_2()
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex_none()
                .when_else(
                    stacked,
                    |status| {
                        status
                            .h(px(56.))
                            .column()
                            .items_start()
                            .justify_center()
                            .gap_1()
                    },
                    |status| {
                        status
                            .h(px(32.))
                            .row()
                            .items_center()
                            .justify_between()
                            .gap_2()
                    },
                )
                .child(paint.badge(label, cx.theme()).flex_none())
                .when_some(countdown, |this, (prefix, countdown)| {
                    this.child(
                        div()
                            .row()
                            .items_center()
                            .gap_2()
                            .text_sm()
                            .whitespace_nowrap()
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(cx.theme().colors.text_muted)
                                    .child(prefix),
                            )
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(paint.text)
                                    .child(countdown),
                            ),
                    )
                }),
        )
        .when_some(progress, |this, progress| {
            this.child(event_progress(event_id, progress, paint, cx))
        })
        .semantic_in(
            cx,
            NodeSpec::new(semantic_id, Role::Status).text(semantic_text),
        )
        .into_any_element()
}

fn event_progress(
    event_id: uuid::Uuid,
    progress: f32,
    paint: MomentPaint,
    cx: &mut App,
) -> impl IntoElement {
    let theme = cx.theme();
    let progress = progress.clamp(0.0, 1.0);
    div()
        .id(format!("focus-event-progress.{event_id}"))
        .w_full()
        .h(px(theme.measures.progress_track_height))
        .flex_none()
        .rounded_full()
        .overflow_hidden()
        .bg(theme.colors.hairline)
        .child(
            div()
                .h_full()
                .w(relative(progress))
                .rounded_full()
                .bg(paint.marker),
        )
        .semantic_in(
            cx,
            NodeSpec::new(format!("focus-event-progress.{event_id}"), Role::Progress)
                .text("Event progress")
                .busy(true)
                .range(0.0, 1.0, progress),
        )
}
