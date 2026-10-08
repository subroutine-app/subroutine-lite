use gpui::{
    AnyElement, App, Context, ElementId, IntoElement, ParentElement, Pixels, Styled, Window, div,
    prelude::FluentBuilder as _, px, relative, size,
};
use gpui_kit::foundation::StyledExt as _;
use gpui_kit_theme::ActiveTheme;
use subroutine_core::AnyItem;
use uuid::Uuid;

use crate::{
    AppIcon,
    components::{
        Button, ButtonVariants, CardMeta, ItemCard,
        ext::ElementExt as _,
        transition::{self, WindowTransitionExt as _},
    },
};

use super::{
    FocusMode, FocusView,
    temporal::{self, EventMoment, SignalMoment, TemporalSnapshot},
};

pub(super) struct NoticeSlot<T> {
    shown: Option<T>,
    pending: Option<T>,
}

impl<T> Default for NoticeSlot<T> {
    fn default() -> Self {
        Self {
            shown: None,
            pending: None,
        }
    }
}

impl<T> NoticeSlot<T> {
    fn sync(&mut self, desired: Option<T>, id: impl Fn(&T) -> Uuid) -> bool {
        match (self.shown.as_ref(), desired) {
            (None, Some(desired)) => {
                self.shown = Some(desired);
                self.pending = None;
                true
            }
            (Some(shown), Some(desired)) if id(shown) == id(&desired) => {
                self.shown = Some(desired);
                self.pending = None;
                true
            }
            (Some(_), Some(desired)) => {
                self.pending = Some(desired);
                false
            }
            (Some(_), None) => {
                self.pending = None;
                false
            }
            (None, None) => false,
        }
    }

    fn finish_exit(&mut self) {
        self.shown = self.pending.take();
    }

    fn retained_ids(
        &self,
        hidden: Option<Uuid>,
        id: impl Fn(&T) -> Uuid,
    ) -> impl Iterator<Item = Uuid> {
        self.shown
            .iter()
            .chain(self.pending.iter())
            .map(id)
            .chain(hidden)
    }
}

struct NoticeMotion {
    opacity: f32,
    offset_y: f32,
    exited: bool,
}

fn notice_motion(
    opacity_key: &'static str,
    offset_key: &'static str,
    visible: bool,
    window: &mut Window,
    cx: &mut App,
) -> NoticeMotion {
    let opacity = window.keyed_transition(opacity_key, cx, transition::QUICK, || 0.0);
    let offset = window.keyed_transition(offset_key, cx, transition::QUICK, || -12.0);
    opacity.set(if visible { 1.0 } else { 0.0 }, cx);
    offset.set(if visible { 0.0 } else { -8.0 }, cx);
    let opacity_value = opacity.animate(window, cx);
    let offset_y = offset.animate(window, cx);
    let exited = !visible
        && !opacity.is_animating(cx)
        && !offset.is_animating(cx)
        && opacity_value <= f32::EPSILON;
    NoticeMotion {
        opacity: opacity_value,
        offset_y,
        exited,
    }
}

pub(crate) fn event_notice_card(
    id: impl Into<ElementId>,
    moment: &EventMoment,
    window: &mut Window,
    cx: &mut App,
) -> ItemCard {
    let (prefix, duration) = match moment {
        EventMoment::InProgress { remaining, .. } => ("Ends in", *remaining),
        EventMoment::Upcoming { until, .. } => ("Starts in", *until),
    };
    let item = AnyItem::Event(moment.card_event().clone());
    let meta = format!("{prefix} {}", temporal::format_countdown(duration));
    let paint = moment.paint(cx.theme());
    ItemCard::new_with_id(id, &item, None, window, cx)
        .meta(vec![CardMeta::new(meta).color(paint.text)])
        .editable(false)
        .size_full()
}

const COLLAPSED_NOTICE_HEIGHT: Pixels = px(60.);
pub(super) const MIN_CAROUSEL_ROOM: Pixels = px(256.);
pub(super) const NOTICE_GAP: Pixels = px(8.);

fn notice_height_limit(viewport: Pixels, rows: usize) -> Pixels {
    let rows = rows.max(1) as f32;
    let available = (viewport * 0.35)
        .min(px(320.))
        .min(viewport - NOTICE_GAP - MIN_CAROUSEL_ROOM);
    ((available - NOTICE_GAP * (rows - 1.)) / rows).max(COLLAPSED_NOTICE_HEIGHT)
}

impl FocusView {
    pub(super) fn notices_stacked(&self) -> bool {
        self.viewport_size.width < px(720.)
    }

    fn notice_height(&self, id: Uuid) -> Pixels {
        let rows = if self.notices_stacked() {
            usize::from(self.event_notice.shown.is_some())
                + usize::from(self.signal_notice.shown.is_some())
        } else {
            1
        };
        self.notice_states
            .height(id, COLLAPSED_NOTICE_HEIGHT)
            .min(notice_height_limit(self.viewport_size.height, rows))
    }

    fn notice_card(
        &self,
        id: Uuid,
        card: ItemCard,
        interactive: bool,
        cx: &mut Context<Self>,
    ) -> ItemCard {
        let view = cx.entity();
        card.details(self.notice_states.get(id))
            .tab_stop(interactive)
            .actionable(interactive)
            .draggable(
                interactive,
                self.notice_card_widths
                    .get(&id)
                    .map(|width| size(*width, self.notice_height(id))),
            )
            .flex_1()
            .min_w_0()
            .on_prepaint(move |bounds, _, cx| {
                view.update(cx, |view, cx| {
                    if view.notice_card_widths.insert(id, bounds.size.width)
                        != Some(bounds.size.width)
                    {
                        cx.notify();
                    }
                });
            })
    }

    fn dismiss_event_notice(&mut self, cx: &mut Context<Self>) {
        if let Some(notice) = self.event_notice.shown.as_ref() {
            self.dismissed_event_notice = Some(notice.notice_id());
            cx.notify();
        }
    }

    fn dismiss_signal_notice(&mut self, cx: &mut Context<Self>) {
        if let Some(notice) = self.signal_notice.shown.as_ref() {
            self.dismissed_signal_notice = Some(notice.notice_id());
            cx.notify();
        }
    }

    fn sync_event_notice(&mut self, desired: Option<EventMoment>) -> bool {
        let desired =
            desired.filter(|notice| self.dismissed_event_notice != Some(notice.notice_id()));
        self.event_notice.sync(desired, EventMoment::notice_id)
    }

    fn sync_signal_notice(&mut self, desired: Option<SignalMoment>) -> bool {
        let desired =
            desired.filter(|notice| self.dismissed_signal_notice != Some(notice.notice_id()));
        self.signal_notice.sync(desired, SignalMoment::notice_id)
    }

    fn render_event_notice(
        &self,
        moment: &EventMoment,
        interactive: bool,
        motion: NoticeMotion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let view = cx.entity().downgrade();
        let dismiss = Button::new((
            "focus-event-notice-dismiss",
            moment.notice_id().as_u64_pair().1,
        ))
        .ghost()
        .xsmall()
        .compact()
        .w_6()
        .icon(AppIcon::Close)
        .tooltip("Dismiss event notification")
        .on_click(cx.listener(|view, _, window, cx| {
            cx.stop_propagation();
            view.dismiss_event_notice(cx);
            view.focus_handle.focus(window, cx);
        }));
        let card = event_notice_card(
            (
                "focus-event-notice-card",
                moment.notice_id().as_u64_pair().1,
            ),
            moment,
            window,
            cx,
        );
        let mut card = self.notice_card(moment.notice_id(), card, interactive, cx);
        if interactive {
            card = card.trailing(dismiss).on_click(move |_, window, cx| {
                view.update(cx, |view, cx| {
                    view.set_mode(FocusMode::Event, cx);
                    view.focus_handle.focus(window, cx);
                })
                .ok();
            });
        }

        div()
            .when_else(
                self.notices_stacked(),
                |notice| notice.w_full(),
                |notice| notice.w(relative(0.42)),
            )
            .min_w_0()
            .max_w(px(520.))
            .flex_none()
            .h(self.notice_height(moment.notice_id()))
            .relative()
            .top(px(motion.offset_y))
            .opacity(motion.opacity)
            .row()
            .items_center()
            .gap_1()
            .child(moment.paint(cx.theme()).marker(cx.theme()))
            .child(card)
            .into_any_element()
    }

    fn render_signal_notice(
        &self,
        moment: &SignalMoment,
        interactive: bool,
        motion: NoticeMotion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let timing = match moment {
            SignalMoment::Upcoming { until, .. } => {
                format!("in {}", temporal::format_countdown(*until))
            }
            SignalMoment::Recent { elapsed, .. } => {
                format!("{} ago", temporal::format_countdown(*elapsed))
            }
        };
        let paint = moment.paint(cx.theme());
        let item = AnyItem::Signal(moment.card_signal().clone());
        let dismiss = Button::new((
            "focus-signal-notice-dismiss",
            moment.notice_id().as_u64_pair().1,
        ))
        .ghost()
        .xsmall()
        .compact()
        .w_6()
        .icon(AppIcon::Close)
        .tooltip("Dismiss signal notification")
        .on_click(cx.listener(|view, _, window, cx| {
            cx.stop_propagation();
            view.dismiss_signal_notice(cx);
            view.focus_handle.focus(window, cx);
        }));
        let card = ItemCard::new_with_id(
            (
                "focus-signal-notice-card",
                moment.notice_id().as_u64_pair().1,
            ),
            &item,
            None,
            window,
            cx,
        )
        .meta(vec![CardMeta::new(timing).color(paint.text)])
        .editable(false)
        .size_full();
        let mut card = self.notice_card(moment.notice_id(), card, interactive, cx);
        if interactive {
            card = card.trailing(dismiss);
        }

        div()
            .when_else(
                self.notices_stacked(),
                |notice| notice.w_full(),
                |notice| notice.w(relative(0.42)),
            )
            .min_w_0()
            .max_w(px(520.))
            .flex_none()
            .h(self.notice_height(moment.notice_id()))
            .relative()
            .top(px(motion.offset_y))
            .opacity(motion.opacity)
            .row()
            .items_center()
            .gap_1()
            .child(paint.marker(cx.theme()))
            .child(card)
            .into_any_element()
    }

    pub(super) fn render_notices(
        &mut self,
        temporal: TemporalSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>, Pixels) {
        let retained_event = temporal
            .event_notice()
            .map(EventMoment::notice_id)
            .filter(|id| self.dismissed_event_notice != Some(*id));
        let desired_event_notice = (self.mode == FocusMode::Action)
            .then(|| temporal.event_notice().cloned())
            .flatten();
        let event_notice_visible = self.sync_event_notice(desired_event_notice);
        let signal_notice_visible = self.sync_signal_notice(temporal.signal.clone());
        let event_notice = if let Some(moment) = self.event_notice.shown.clone() {
            let motion = notice_motion(
                "focus-event-notice-opacity",
                "focus-event-notice-offset",
                event_notice_visible,
                window,
                cx,
            );
            if motion.exited {
                self.event_notice.finish_exit();
                cx.notify();
                None
            } else {
                Some((
                    self.render_event_notice(&moment, event_notice_visible, motion, window, cx),
                    self.notice_height(moment.notice_id()),
                ))
            }
        } else {
            None
        };

        let signal_notice = if let Some(moment) = self.signal_notice.shown.clone() {
            let motion = notice_motion(
                "focus-signal-notice-opacity",
                "focus-signal-notice-offset",
                signal_notice_visible,
                window,
                cx,
            );
            if motion.exited {
                self.signal_notice.finish_exit();
                cx.notify();
                None
            } else {
                Some((
                    self.render_signal_notice(&moment, signal_notice_visible, motion, window, cx),
                    self.notice_height(moment.notice_id()),
                ))
            }
        } else {
            None
        };

        let height = event_notice
            .as_ref()
            .map(|(_, height)| *height)
            .into_iter()
            .chain(signal_notice.as_ref().map(|(_, height)| *height))
            .reduce(|height, next| {
                if self.notices_stacked() {
                    height + NOTICE_GAP + next
                } else {
                    height.max(next)
                }
            })
            .unwrap_or(px(0.));
        let retained_ids: Vec<_> = self
            .event_notice
            .retained_ids(retained_event, EventMoment::notice_id)
            .chain(
                self.signal_notice
                    .retained_ids(None, SignalMoment::notice_id),
            )
            .collect();
        self.notice_card_widths
            .retain(|id, _| retained_ids.contains(id));
        self.notice_states.retain(retained_ids);
        (
            event_notice.map(|(notice, _)| notice),
            signal_notice.map(|(notice, _)| notice),
            height,
        )
    }
}
