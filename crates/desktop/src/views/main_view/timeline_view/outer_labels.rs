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
    BaseTimeDivision, HOUR_DIVIDER_HEIGHT, OuterPeriodLabel, STICKY_OUTER_LABEL_GAP,
    STICKY_OUTER_LABEL_HEIGHT, STICKY_OUTER_LABEL_INSET, TimelineView, geometry::TimelineGeometry,
};

const BORDER: Pixels = px(1.);

fn sticky_center() -> Pixels {
    TOP_EDGE_INSET + STICKY_OUTER_LABEL_HEIGHT / 2.
}

fn boundary_center(geometry: &TimelineGeometry, time: DateTime<Local>) -> Pixels {
    // Use the divider's snapped row, including its half-height, as the attachment anchor.
    geometry.y(geometry.offset(time) - HOUR_DIVIDER_HEIGHT / 2.) + HOUR_DIVIDER_HEIGHT / 2.
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

    pub(super) fn spacer(&self, text: &str, window: &Window) -> Div {
        div()
            .flex_none()
            .w(self.width(text, window))
            .h(self.height)
            .mr(STICKY_OUTER_LABEL_INSET)
    }

    fn attached(
        &self,
        period: &OuterPeriodLabel,
        center: Pixels,
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
        let handoff = Handoff::new(boundary_center(&self.geometry, next.start), layout.height);
        let width = layout.width(&period.text, window);
        let shell_width = handoff.width(width, layout.width(&next.text, window));
        let sticky_top = sticky_center() - layout.height / 2.;
        let sticky_bottom = sticky_top + layout.height;
        let target = if boundary_center(&self.geometry, next.start) <= sticky_bottom {
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

        let mut attached = Vec::new();
        let mut period = next;
        loop {
            let center = boundary_center(&self.geometry, period.start);
            if center - layout.height / 2. > bounds.size.height {
                break;
            }
            attached.push(layout.attached(&period, center, window, cx));
            period = division.outer_period_label_at(period.end)?;
        }

        let sticky = div()
            .row()
            .absolute()
            .top(sticky_top)
            .right(STICKY_OUTER_LABEL_INSET)
            .child(sticky);

        // Paint attached text after the glass so entering glyphs are never part of its backdrop.
        Some(
            div()
                .absolute()
                .inset_0()
                .child(sticky)
                .child(outgoing)
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
                }
            }
        }
    }
}
