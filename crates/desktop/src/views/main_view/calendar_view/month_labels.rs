use chrono::{Datelike, Months, NaiveDate, Weekday};
use gpui::{
    App, Context, Div, FontWeight, InteractiveElement, ParentElement, Pixels, Role, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_kit::foundation::StyledExt as _;
use gpui_kit_theme::ActiveTheme as _;

use crate::{
    color::ColorExt as _,
    components::{Button, ButtonVariants, Label},
};

use super::CalendarView;

pub(super) const INSET: Pixels = px(12.);
const HANDOFF_GAP: Pixels = px(6.);
const LABEL_PARALLAX_RATE: f32 = 0.95;

fn text_layout_width(shaped_width: Pixels, scale: f32) -> Pixels {
    // Text layout rounds each leaf to logical pixels before the layout adapter's device ceiling.
    px((shaped_width.ceil().as_f32() * scale).ceil() / scale)
}

#[derive(Clone, Copy)]
struct LabelStyle {
    height: Pixels,
    border: Pixels,
    padding: Pixels,
    font_size: Pixels,
    gap: Pixels,
    compact: bool,
}

impl LabelStyle {
    fn new(day_width: Pixels, window: &Window) -> Self {
        let compact = day_width < px(80.);
        Self {
            height: window.pixel_snap(sticky_height(day_width)),
            border: window
                .pixel_snap(px(1.))
                .max(px(1. / window.scale_factor())),
            padding: window.pixel_snap(window.rem_size() * if compact { 0.5 } else { 1. }),
            font_size: window.rem_size() * if compact { 1. } else { 1.5 },
            gap: window.pixel_snap(window.rem_size() * 0.25),
            compact,
        }
    }

    fn month_text(self, month: NaiveDate) -> String {
        month
            .format(if self.compact { "%b" } else { "%B" })
            .to_string()
    }

    fn width(self, month: NaiveDate, window: &Window) -> Pixels {
        let measure = |text: String, weight| {
            let mut style = window.text_style();
            style.font_weight = weight;
            let run = style.to_run(text.len());
            let width = window
                .text_system()
                .shape_line(SharedString::from(text), self.font_size, &[run], None)
                .width;
            text_layout_width(width, window.scale_factor())
        };
        measure(self.month_text(month), FontWeight::NORMAL)
            + self.gap
            + measure(month.format("%Y").to_string(), FontWeight::EXTRA_LIGHT)
            + (self.padding + self.border) * 2.
    }

    fn content(self, month: NaiveDate, width: Pixels, prominence: f32, cx: &App) -> Div {
        let colors = &cx.theme().colors;
        div()
            .row()
            .flex_none()
            .justify_end()
            .w(width - self.border * 2.)
            .h(self.height - self.border * 2.)
            .px(self.padding)
            .gap(self.gap)
            .text_size(self.font_size)
            .line_height(self.font_size * 1.25)
            .font_weight(FontWeight::NORMAL)
            .text_color(colors.text.mix(colors.text_muted, prominence))
            .whitespace_nowrap()
            .child(Label::new(self.month_text(month)))
            .child(Label::new(month.format("%Y").to_string()).font_weight(FontWeight::EXTRA_LIGHT))
    }

    fn attached(
        self,
        month: NaiveDate,
        offset: Pixels,
        window: &Window,
        cx: &Context<CalendarView>,
    ) -> Div {
        let width = self.width(month, window);
        let handoff = Handoff::new(offset, self.height);
        let hover = cx.theme().colors.hover.alpha(0.3 * handoff.opacity);
        div()
            .absolute()
            .top(offset - self.height)
            .right(INSET)
            .w(width)
            .h(self.height)
            .child(
                Button::new(("calendar-attached-month", month.num_days_from_ce() as u32))
                    .ghost()
                    .tab_stop(false)
                    .absolute()
                    .inset_0()
                    .h(self.height)
                    .p_0()
                    .rounded_full()
                    .block_mouse_except_scroll()
                    .hover(move |style| style.bg(hover))
                    .role(Role::Button)
                    .aria_label(format!("Go to {}", month.format("%B %Y")))
                    .tooltip(format!("Go to {}", month.format("%B %Y")))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        cx.stop_propagation();
                        view.scroll_to_date(month, cx);
                    })),
            )
            .child(
                self.content(month, width, 1. - handoff.opacity, cx)
                    .absolute()
                    .top(self.border)
                    .right(self.border),
            )
    }
}

fn sticky_height(day_width: Pixels) -> Pixels {
    if day_width < px(80.) {
        px(32.)
    } else {
        px(40.)
    }
}

fn month_at_boundary(
    candidate: NaiveDate,
    anchor: Pixels,
    boundary: impl Fn(NaiveDate) -> Pixels,
) -> NaiveDate {
    let month = candidate.with_day(1).unwrap();
    if boundary(month) > anchor {
        month.checked_sub_months(Months::new(1)).unwrap_or(month)
    } else if let Some(next) = month.checked_add_months(Months::new(1))
        && boundary(next) <= anchor
    {
        next
    } else {
        month
    }
}

fn parallax_offset(boundary: Pixels, anchor: Pixels) -> Pixels {
    (boundary - anchor) * LABEL_PARALLAX_RATE
}

struct Handoff {
    offset: Pixels,
    opacity: f32,
}

impl Handoff {
    fn new(next_offset: Pixels, height: Pixels) -> Self {
        let travel = height + HANDOFF_GAP;
        let remaining = next_offset.clamp(px(0.), travel);
        Self {
            offset: remaining - travel,
            opacity: remaining / travel,
        }
    }

    fn width(&self, current: Pixels, next: Pixels) -> Pixels {
        current * self.opacity + next * (1. - self.opacity)
    }
}

impl CalendarView {
    pub(super) fn month_label_top(&self) -> Pixels {
        self.visible_top() + INSET
    }

    pub(super) fn month_label_height(&self) -> Pixels {
        sticky_height(self.day_width())
    }

    fn month_boundary(&self, month: NaiveDate) -> Pixels {
        let horizon = self.bounds.unwrap_or_default().size.height / 2. + super::grid::EDGE_HORIZON;
        self.week_to_offset(month.week(Weekday::Sun)) + self.scroll_offset + horizon
    }

    fn month_label_offset(&self, month: NaiveDate, window: &Window) -> Pixels {
        // Both sides of the sticky clip must use the same device-aligned offset.
        window.pixel_snap(parallax_offset(
            self.month_boundary(month),
            self.visible_top(),
        ))
    }

    pub(super) fn sticky_month(&self) -> NaiveDate {
        let horizon = self.bounds.unwrap_or_default().size.height / 2. + super::grid::EDGE_HORIZON;
        let anchor = self.visible_top();
        let week = self.offset_to_week(anchor - self.scroll_offset - horizon, self.week_height);
        // Saturday owns the rightmost segment of a stepped month boundary.
        month_at_boundary(week.last_day(), anchor, |month| self.month_boundary(month))
    }

    fn render_sticky_month(
        &self,
        month: NaiveDate,
        style: LabelStyle,
        window: &Window,
        cx: &Context<Self>,
    ) -> Div {
        let top = window.pixel_snap(self.month_label_top());
        let next = month.checked_add_months(Months::new(1));
        let offset = next.map_or(style.height + HANDOFF_GAP, |month| {
            self.month_label_offset(month, window)
        });
        let handoff = Handoff::new(offset, style.height);
        let width = style.width(month, window);
        let next_width = next.map_or(width, |month| style.width(month, window));

        let sticky = Button::new("calendar-month-select")
            .glass_pill()
            .w(handoff.width(width, next_width))
            .h(style.height)
            .border(style.border)
            .p_0()
            .gap_0()
            .tooltip(month.format("%B %Y").to_string())
            .block_mouse_except_scroll()
            .role(Role::Button)
            .aria_label(format!("Choose month · {}", month.format("%B %Y")))
            .on_click(cx.listener(|view, _, _, cx| {
                cx.stop_propagation();
                view.toggle_month_selector(cx);
            }));
        // Keep the button in flow so its glass wrapper measures the full control.
        let sticky = div().row().absolute().top(top).right(INSET).child(sticky);
        let outgoing = div()
            .absolute()
            .top(top + style.border)
            .right(INSET + style.border)
            .w(width - style.border * 2.)
            .h(style.height - style.border * 2.)
            .overflow_hidden()
            .child(
                style
                    .content(month, width, 1., cx)
                    .absolute()
                    .right_0()
                    .top(handoff.offset)
                    .opacity(handoff.opacity),
            );
        let incoming = next.filter(|_| offset < style.height).map(|month| {
            div()
                .absolute()
                .top(top + style.border)
                .right(INSET + style.border)
                .w(next_width - style.border * 2.)
                .h(style.height - style.border)
                .overflow_hidden()
                .child(
                    style
                        .content(month, next_width, 1. - handoff.opacity, cx)
                        .absolute()
                        .right_0()
                        .top(offset),
                )
        });
        div()
            .absolute()
            .inset_0()
            .child(sticky)
            .child(outgoing)
            .children(incoming)
    }

    pub(super) fn render_month_labels(&self, window: &Window, cx: &Context<Self>) -> (Div, Div) {
        if self.compact_inspector() {
            return (div(), div());
        }
        let style = LabelStyle::new(self.day_width(), window);
        let top = window.pixel_snap(self.month_label_top());
        let month = self.sticky_month();
        let bottom = self.bounds.unwrap_or_default().size.height + super::grid::EDGE_HORIZON;
        let mut attached = Vec::new();
        let mut next = month.checked_add_months(Months::new(1));
        while let Some(month) = next {
            let offset = self.month_label_offset(month, window);
            if top + offset > bottom {
                break;
            }
            attached.push(style.attached(month, offset, window, cx));
            next = month.checked_add_months(Months::new(1));
        }
        (
            div()
                .absolute()
                .top(top + style.height)
                .bottom_0()
                .left_0()
                .right_0()
                .overflow_hidden()
                .children(attached),
            self.render_sticky_month(month, style, window, cx),
        )
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn text_width_reserves_each_independently_rounded_leaf() {
        for scale in [1., 1.25, 1.5, 2.] {
            let month = text_layout_width(px(80.2), scale);
            let year = text_layout_width(px(28.6), scale);
            assert!(month >= px(81.));
            assert!(year >= px(29.));
            assert!((month.as_f32() * scale).fract().abs() < 0.001);
            assert!((year.as_f32() * scale).fract().abs() < 0.001);
        }
        assert_eq!(
            text_layout_width(px(80.2), 2.) + text_layout_width(px(28.6), 2.),
            px(110.)
        );
    }

    #[test]
    fn parallax_tracks_month_boundaries_without_drift_or_directional_jumps() {
        let first = NaiveDate::from_ymd_opt(2023, 12, 1).unwrap();
        for height in [px(88.), px(160.)] {
            let boundary = |date: NaiveDate| {
                height
                    * ((date.week(Weekday::Sun).first_day() - first.week(Weekday::Sun).first_day())
                        .num_days()
                        / 7) as f32
            };
            for month_index in 0..15 {
                let month = first.checked_add_months(Months::new(month_index)).unwrap();
                let next = month.checked_add_months(Months::new(1)).unwrap();
                let start = boundary(month);
                let end = boundary(next);
                assert_eq!(parallax_offset(start, start), px(0.));
                assert_eq!(parallax_offset(end, end), px(0.));
                for step in (0..=128).chain((0..=128).rev()) {
                    let anchor = start + (end - start) * (step as f32 / 128.);
                    let pinned =
                        month_at_boundary(month.week(Weekday::Sun).last_day(), anchor, boundary);
                    assert_eq!(pinned, if step == 128 { next } else { month });
                    let current_offset = parallax_offset(start, anchor);
                    let next_offset = parallax_offset(end, anchor);
                    assert!(
                        ((next_offset - current_offset) - (end - start) * LABEL_PARALLAX_RATE)
                            .abs()
                            < px(0.001)
                    );
                    for delta in [px(-40.), px(40.)] {
                        let shifted = parallax_offset(end + delta, anchor);
                        assert!(
                            (shifted - next_offset - delta * LABEL_PARALLAX_RATE).abs() < px(0.001)
                        );
                        assert!((shifted - next_offset).abs() < delta.abs());
                    }
                }
            }
        }
    }

    #[test]
    fn upcoming_month_waits_until_its_boundary_approaches_the_top() {
        let october = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let september = october.checked_sub_months(Months::new(1)).unwrap();
        let october_week = october.week(Weekday::Sun).first_day();
        let top = px(40.);
        for bottom in [px(440.), px(840.), px(1240.)] {
            let october_top = top + (bottom - top) * 0.3;
            let boundary = |month: NaiveDate| {
                october_top
                    + px(160.)
                        * ((month.week(Weekday::Sun).first_day() - october_week).num_days() / 7)
                            as f32
            };
            assert_eq!(month_at_boundary(september, top, boundary), september);
            assert_eq!(month_at_boundary(october, top, boundary), september);
            let offset = parallax_offset(october_top, top);
            assert_eq!(Handoff::new(offset, px(40.)).opacity, 1.);
            assert_eq!(month_at_boundary(october, october_top, boundary), october);
            assert_eq!(parallax_offset(october_top, october_top), px(0.));
        }
    }

    #[test]
    fn handoff_keeps_fixed_size_labels_apart() {
        for height in [px(32.), px(40.)] {
            let travel = height + HANDOFF_GAP;
            for step in (0..=128).chain((0..=128).rev()) {
                let next_offset = travel * (step as f32 / 128.);
                let handoff = Handoff::new(next_offset, height);
                assert_eq!(next_offset - handoff.offset, travel);
                assert_eq!(handoff.opacity, step as f32 / 128.);
                assert_eq!(
                    handoff.width(px(180.), px(100.)),
                    px(100.) + px(80.) * handoff.opacity
                );
            }
        }
    }
}
