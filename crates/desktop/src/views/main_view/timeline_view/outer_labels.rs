use chrono::{DateTime, Duration, Local};
use gpui::{
    Context, Div, FontWeight, InteractiveElement, IntoElement, ParentElement, Pixels, Role,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_kit::foundation::StyledExt as _;
use gpui_kit_theme::ActiveTheme as _;

use crate::{
    components::{Button, ButtonVariants, Label},
    views::TOP_EDGE_INSET,
};

use super::{
    ANNOTATION_CHIP_HEIGHT, BaseTimeDivision, EDGE_HORIZON, HOUR_DIVIDER_HEIGHT, OuterPeriodLabel,
    STICKY_OUTER_LABEL_GAP, STICKY_OUTER_LABEL_HEIGHT, STICKY_OUTER_LABEL_INSET, TimelineView,
    geometry::TimelineGeometry,
};

const BORDER: Pixels = px(1.);
const LABEL_PARALLAX_RATE: f32 = 0.82;

fn sticky_center() -> Pixels {
    TOP_EDGE_INSET + STICKY_OUTER_LABEL_HEIGHT / 2.
}

fn boundary_center(geometry: &TimelineGeometry, time: DateTime<Local>) -> Pixels {
    // Use the divider's snapped row, including its half-height, as the period boundary.
    geometry.y(geometry.offset(time) - HOUR_DIVIDER_HEIGHT / 2.) + HOUR_DIVIDER_HEIGHT / 2.
}

fn label_offset(geometry: &TimelineGeometry, time: DateTime<Local>) -> Pixels {
    (boundary_center(geometry, time) - sticky_center()) * LABEL_PARALLAX_RATE
}

fn sticky_period(
    division: BaseTimeDivision,
    geometry: &TimelineGeometry,
) -> Option<OuterPeriodLabel> {
    let period = division.outer_period_label_at(geometry.time_at_y(sticky_center()))?;
    // The inverse time mapping is continuous; ownership follows the displayed, snapped boundary.
    if boundary_center(geometry, period.start) > sticky_center() {
        division.outer_period_label_at(period.start - Duration::nanoseconds(1))
    } else if boundary_center(geometry, period.end) <= sticky_center() {
        division.outer_period_label_at(period.end)
    } else {
        Some(period)
    }
}

fn label_parts(text: &str) -> (&str, Option<&str>) {
    text.split_once(' ')
        .map_or((text, None), |(primary, secondary)| {
            (primary, Some(secondary))
        })
}

#[derive(Clone, Copy)]
pub(super) struct OuterLabelLayout {
    height: Pixels,
    padding: Pixels,
    font_size: Pixels,
    gap: Pixels,
}

impl OuterLabelLayout {
    fn new(viewport_width: Pixels, window: &Window) -> Self {
        let compact = viewport_width < px(560.);
        Self {
            height: if compact {
                px(32.)
            } else {
                STICKY_OUTER_LABEL_HEIGHT
            },
            padding: window.rem_size() * if compact { 0.5 } else { 1.0 },
            font_size: window.rem_size() * if compact { 1.0 } else { 1.5 },
            gap: window.rem_size() * 0.25,
        }
    }

    fn width(&self, text: &str, window: &Window) -> Pixels {
        let measure = |text: &str, weight| {
            let mut style = window.text_style();
            style.font_weight = weight;
            window
                .text_system()
                .shape_line(
                    SharedString::from(text.to_owned()),
                    self.font_size,
                    &[style.to_run(text.len())],
                    None,
                )
                .width
        };
        let (primary, secondary) = label_parts(text);
        let width = measure(primary, FontWeight::NORMAL)
            + secondary.map_or(px(0.), |text| {
                self.gap + measure(text, FontWeight::EXTRA_LIGHT)
            })
            + (self.padding + BORDER) * 2.;
        px((width.as_f32() * window.scale_factor()).ceil() / window.scale_factor())
    }

    fn summary_width(&self, label_width: Pixels, viewport_width: Pixels) -> Pixels {
        (viewport_width / 2. - STICKY_OUTER_LABEL_INSET - label_width - self.gap)
            .clamp(px(0.), px(240.))
    }

    fn marker_context(
        &self,
        summary: Button,
        label_width: Pixels,
        summary_width: Pixels,
        cx: &Context<TimelineView>,
    ) -> Div {
        div()
            .absolute()
            .top_0()
            .right_0()
            .w(label_width + self.gap + summary_width)
            .h(self.height)
            .child(
                summary
                    .absolute()
                    .left_0()
                    .top((self.height - ANNOTATION_CHIP_HEIGHT) / 2.)
                    .w(summary_width)
                    .h(ANNOTATION_CHIP_HEIGHT)
                    .justify_end(),
            )
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left(self.padding)
                    .right(self.padding)
                    .h(BORDER)
                    .bg(cx.theme().colors.hairline),
            )
    }

    fn attached(
        &self,
        period: &OuterPeriodLabel,
        center: Pixels,
        summary: Option<Div>,
        window: &Window,
        cx: &Context<TimelineView>,
    ) -> Div {
        let start = period.start;
        let end = period.end;
        let width = self.width(&period.text, window);
        let hover = cx
            .theme()
            .colors
            .hover
            .alpha(0.3 * Handoff::new(center, self.height).opacity);
        let top = center - self.height / 2.;
        let hit_top = top.max(sticky_center() + self.height / 2.);
        let control = Button::new(("timeline-fit-outer-period", start.timestamp() as u64))
            .ghost()
            .tab_stop(false)
            .absolute()
            .top(top - hit_top)
            .w(width)
            .h(self.height)
            .p_0()
            .border(BORDER)
            .border_color(gpui::transparent_black())
            .rounded_full()
            .block_mouse_except_scroll()
            .hover(move |style| style.bg(hover))
            .role(Role::Button)
            .aria_label(format!("Zoom to {}", period.text))
            .on_click(cx.listener(move |view, _, _, cx| {
                cx.stop_propagation();
                view.zoom_to_span(start, end, cx);
            }));

        div()
            .absolute()
            .top(top)
            .right(STICKY_OUTER_LABEL_INSET)
            .w(width)
            .h(self.height)
            .child(
                // Only the fixed button owns input over the glass during a handoff.
                div()
                    .absolute()
                    .top(hit_top - top)
                    .w_full()
                    .h(top + self.height - hit_top)
                    .overflow_hidden()
                    .child(control),
            )
            .child(
                self.content(&period.text, width, cx)
                    .absolute()
                    .top(BORDER)
                    .right(BORDER),
            )
            .children(summary)
    }

    fn content(&self, text: &str, width: Pixels, cx: &Context<TimelineView>) -> Div {
        let (primary, secondary) = label_parts(text);
        div()
            .row()
            .flex_none()
            .justify_end()
            .w(width - BORDER * 2.)
            .h(self.height - BORDER * 2.)
            .px(self.padding)
            .gap(self.gap)
            .text_size(self.font_size)
            .line_height(self.font_size * 1.25)
            .font_weight(FontWeight::NORMAL)
            .text_color(cx.theme().colors.text)
            .whitespace_nowrap()
            .child(Label::new(primary.to_owned()))
            .children(
                secondary
                    .map(|text| Label::new(text.to_owned()).font_weight(FontWeight::EXTRA_LIGHT)),
            )
    }
}

struct Handoff {
    offset: Pixels,
    opacity: f32,
}

impl Handoff {
    fn new(next_center: Pixels, height: Pixels) -> Self {
        let travel = height + STICKY_OUTER_LABEL_GAP;
        let remaining = (next_center - sticky_center()).clamp(px(0.), travel);
        Self {
            offset: remaining - travel,
            opacity: remaining / travel,
        }
    }

    fn width(&self, current: Pixels, next: Pixels) -> Pixels {
        current * self.opacity + next * (1. - self.opacity)
    }
}

impl TimelineView {
    pub(super) fn outer_label_layout(&self, window: &Window) -> OuterLabelLayout {
        OuterLabelLayout::new(self.bounds.unwrap_or_default().size.width, window)
    }

    pub(super) fn render_outer_labels(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> Option<impl IntoElement> {
        let bounds = self.bounds?;
        let division = self.current_division_state().base_division;
        let period = sticky_period(division, &self.geometry)?;
        let layout = self.outer_label_layout(window);
        let next = division.outer_period_label_at(period.end)?;
        let label_center =
            |time| sticky_center() + window.pixel_snap(label_offset(&self.geometry, time));
        let next_center = label_center(next.start);
        let handoff = Handoff::new(next_center, layout.height);
        let width = layout.width(&period.text, window);
        let shell_width = handoff.width(width, layout.width(&next.text, window));
        let sticky_top = sticky_center() - layout.height / 2.;
        let sticky_bottom = sticky_top + layout.height;
        let target = if next_center <= sticky_bottom {
            &next
        } else {
            &period
        };
        let start = target.start;
        let end = target.end;

        let sticky = Button::new("timeline-fit-sticky-period")
            .glass_pill()
            .w(shell_width)
            .h(layout.height)
            .p_0()
            .gap_0()
            .block_mouse_except_scroll()
            .role(Role::Button)
            .aria_label(format!("Zoom to {}", target.text))
            .on_click(cx.listener(move |view, _, _, cx| {
                cx.stop_propagation();
                view.zoom_to_span(start, end, cx);
            }));

        let day_context = |period: &OuterPeriodLabel| {
            if division != BaseTimeDivision::Hour {
                return None;
            }
            let label_width = layout.width(&period.text, window);
            let summary_width = layout.summary_width(label_width, bounds.size.width);
            if summary_width <= px(0.) {
                return None;
            }
            let summary = self.marker_summary(period.start, period.end)?;
            Some(
                layout.marker_context(
                    self.render_marker_summary(
                        summary,
                        (
                            "timeline-outer-day-markers",
                            period.start.timestamp() as u64,
                        ),
                        summary_width,
                        cx,
                    )
                    .tab_stop(period.start == start),
                    label_width,
                    summary_width,
                    cx,
                ),
            )
        };

        let partial_day = division == BaseTimeDivision::Day
            && self.duration_to_height(Duration::days(1)) > bounds.size.height - TOP_EDGE_INSET;
        let sticky_context = match division {
            BaseTimeDivision::Minute | BaseTimeDivision::FiveMinutes | BaseTimeDivision::Day
                if division != BaseTimeDivision::Day || partial_day =>
            {
                let time = if partial_day {
                    self.geometry.time_at_y(sticky_center())
                } else {
                    start
                };
                let day = BaseTimeDivision::Day.floor_boundary(time);
                let end = BaseTimeDivision::Day.next_boundary(day);
                let summary_width = layout.summary_width(shell_width, bounds.size.width);
                if summary_width > px(0.) {
                    self.marker_summary(day, end).map(|summary| {
                        layout.marker_context(
                            self.render_marker_summary(
                                summary.with_date(),
                                ("timeline-sticky-day-markers", day.timestamp() as u64),
                                summary_width,
                                cx,
                            ),
                            shell_width,
                            summary_width,
                            cx,
                        )
                    })
                } else {
                    None
                }
            }
            _ => None,
        };

        // Keep the text's horizontal envelope independent of the changing glass width.
        let outgoing = div()
            .absolute()
            .top(sticky_top + BORDER)
            .right(STICKY_OUTER_LABEL_INSET + BORDER)
            .w(width - BORDER * 2.)
            .h(layout.height - BORDER * 2.)
            .overflow_hidden()
            .child(
                layout
                    .content(&period.text, width, cx)
                    .absolute()
                    .right_0()
                    .top(handoff.offset)
                    .opacity(handoff.opacity),
            );

        // Share the date's handoff without widening its text clip or zoom target.
        let outgoing_context = day_context(&period).map(|summary| {
            div()
                .absolute()
                .top(sticky_top)
                .right(STICKY_OUTER_LABEL_INSET)
                .w(width + layout.gap + layout.summary_width(width, bounds.size.width))
                .h(layout.height)
                .overflow_hidden()
                .child(summary.top(handoff.offset).opacity(handoff.opacity))
        });

        let mut attached = Vec::new();
        let mut period = next;
        loop {
            let center = label_center(period.start);
            if center - layout.height / 2. > bounds.size.height + EDGE_HORIZON {
                break;
            }
            attached.push(layout.attached(&period, center, day_context(&period), window, cx));
            period = division.outer_period_label_at(period.end)?;
        }

        let sticky = div()
            .row()
            .absolute()
            .top(sticky_top)
            .right(STICKY_OUTER_LABEL_INSET)
            .child(sticky)
            .children(sticky_context);

        // Paint attached text after the glass so entering glyphs are never part of its backdrop.
        Some(
            div()
                .absolute()
                .inset_0()
                .child(sticky)
                .child(outgoing)
                .children(outgoing_context)
                .children(attached),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone as _;

    #[test]
    fn handoff_reverses_without_changing_the_text_anchor_or_jumping_width() {
        for height in [px(32.), STICKY_OUTER_LABEL_HEIGHT] {
            let travel = height + STICKY_OUTER_LABEL_GAP;
            let current_width = px(146.);
            let next_width = px(82.);
            for step in (0..=128).chain((0..=128).rev()) {
                let remaining = travel * (step as f32 / 128.);
                let incoming_center = sticky_center() + remaining;
                let handoff = Handoff::new(incoming_center, height);
                let outgoing_center = sticky_center() + handoff.offset;
                assert_eq!(incoming_center - outgoing_center, travel);
                assert_eq!(handoff.opacity, step as f32 / 128.);
                assert_eq!(
                    handoff.width(current_width, next_width),
                    next_width + (current_width - next_width) * handoff.opacity,
                );
            }
            let before = Handoff::new(sticky_center() + travel + px(100.), height);
            assert_eq!((before.offset, before.opacity), (px(0.), 1.));
            assert_eq!(before.width(current_width, next_width), current_width);
            let after = Handoff::new(sticky_center(), height);
            assert_eq!((after.offset, after.opacity), (-travel, 0.));
            assert_eq!(after.width(current_width, next_width), next_width);
            assert_eq!(sticky_center() - height / 2. + height / 2., sticky_center());
        }
    }

    #[test]
    fn parallax_tracks_scroll_without_drifting_across_rebases() {
        let epoch = Local
            .with_ymd_and_hms(2026, 10, 13, 12, 0, 0)
            .single()
            .unwrap();
        let duration = Duration::seconds(1);
        for scale in [1., 1.25, 1.33, 1.5, 2.] {
            let mut geometry = TimelineGeometry::new(epoch, duration, scale);
            geometry.update(epoch, duration, px(0.), sticky_center(), scale);
            let time = epoch + Duration::seconds(240);
            let initial_boundary = boundary_center(&geometry, time);
            let initial_offset = label_offset(&geometry, time);
            for step in (-160..=160).chain((-160..=160).rev()) {
                geometry.update(
                    epoch,
                    duration,
                    px(step as f32 / 2.),
                    sticky_center(),
                    scale,
                );
                let movement = boundary_center(&geometry, time) - initial_boundary;
                let label_movement = label_offset(&geometry, time) - initial_offset;
                assert!((label_movement - movement * LABEL_PARALLAX_RATE).abs() < px(0.001));
            }
            let later = epoch + Duration::seconds(20_000);
            geometry.update(later, duration, px(0.), sticky_center(), scale);
            assert!(
                (label_offset(&geometry, later + Duration::seconds(240)) - initial_offset).abs()
                    < px(0.001)
            );
        }
    }

    #[test]
    fn ownership_tracks_the_painted_boundary_in_both_scroll_directions() {
        let date = Local
            .with_ymd_and_hms(2026, 10, 13, 12, 0, 0)
            .single()
            .unwrap();
        let duration = Duration::seconds(7);
        for division in [
            BaseTimeDivision::Minute,
            BaseTimeDivision::FiveMinutes,
            BaseTimeDivision::Hour,
            BaseTimeDivision::Day,
            BaseTimeDivision::Month,
            BaseTimeDivision::Year,
        ] {
            let Some(period) = division.outer_period_label_at(date) else {
                let geometry = TimelineGeometry::new(date, duration, 1.);
                assert!(sticky_period(division, &geometry).is_none());
                continue;
            };
            for scale in [1., 1.25, 1.33, 1.5, 2.] {
                let mut geometry = TimelineGeometry::new(period.start, duration, scale);
                for step in (-128..=128).chain((-128..=128).rev()) {
                    geometry.update(
                        period.start,
                        duration,
                        px(step as f32 / 64.),
                        sticky_center(),
                        scale,
                    );
                    let pinned = sticky_period(division, &geometry).unwrap();
                    let center = boundary_center(&geometry, period.start);
                    if center <= sticky_center() {
                        assert_eq!(pinned.start, period.start);
                    } else {
                        assert_eq!(pinned.end, period.start);
                    }
                    assert!(boundary_center(&geometry, pinned.start) <= sticky_center());
                    assert!(boundary_center(&geometry, pinned.end) > sticky_center());
                    assert!(label_offset(&geometry, pinned.start) <= px(0.));
                    assert!(label_offset(&geometry, pinned.end) > px(0.));
                }
            }
        }
    }
}
