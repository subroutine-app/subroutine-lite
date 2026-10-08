use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Pixels, Styled, div,
    prelude::FluentBuilder as _, px,
};
use gpui_kit::foundation::{Disableable, StyledExt as _};
use gpui_kit::overlay::{GlassExt as _, GlassPreset};
use gpui_kit_theme::{ActiveTheme, Radius, Surface};

use crate::components::{Button, ButtonVariants, Label};
use crate::icons::AppIcon;
use crate::settings::TimelineToolbarPosition;
use crate::views::TOP_EDGE_INSET;

use super::{
    TimelineView,
    items::{ATTACHED_ITEM_LEFT, MIN_ANNOTATION_GUTTER, MIN_ITEM_WIDTH},
};

pub(super) const TIMELINE_TOOLBAR_WIDTH: f32 = 32. + 2. * FOCUS_RING_INSET + 2. + 2. * 8.;
const CONTROL_SIZE: Pixels = px(32.);
const CONTROL_GAP: Pixels = px(4.);
const FOCUS_RING_INSET: f32 = 3.;
const EDGE_INSET: Pixels = px(12.);
const BOTTOM_TOOLBAR_WIDTH: Pixels = px(7. * 32. + 2. * 9. + 8. * 4. + 2. * FOCUS_RING_INSET + 2.);

pub(super) fn toolbar_right_clearance(position: TimelineToolbarPosition) -> Pixels {
    match position {
        TimelineToolbarPosition::Bottom => px(0.),
        TimelineToolbarPosition::Right => px(TIMELINE_TOOLBAR_WIDTH),
    }
}

fn bottom_toolbar_layout(viewport_width: Pixels, item_left: Pixels) -> (Pixels, Pixels) {
    let width = (viewport_width - item_left - EDGE_INSET)
        .max(px(0.))
        .min(BOTTOM_TOOLBAR_WIDTH);
    let left = ((viewport_width - width) / 2.).max(item_left);
    (left, width)
}

fn control(id: &'static str, icon: AppIcon, tooltip: &'static str, enabled: bool) -> Button {
    Button::new(id)
        .ghost()
        .compact()
        .size(CONTROL_SIZE)
        .flex_none()
        .rounded_full()
        .icon(icon)
        .tooltip(tooltip)
        .disabled(!enabled)
}

impl TimelineView {
    pub(super) fn effective_toolbar_position(&self) -> TimelineToolbarPosition {
        let right_min_width = ATTACHED_ITEM_LEFT
            + MIN_ITEM_WIDTH
            + MIN_ANNOTATION_GUTTER
            + px(TIMELINE_TOOLBAR_WIDTH);
        if self.bounds.is_some_and(|bounds| {
            bounds.size.width < right_min_width
                || bounds.size.height - TOP_EDGE_INSET < BOTTOM_TOOLBAR_WIDTH + px(16.)
        }) {
            TimelineToolbarPosition::Bottom
        } else {
            self.toolbar_position
        }
    }

    pub(super) fn render_timeline_toolbar(&self, cx: &Context<Self>) -> impl IntoElement {
        let horizontal = self.effective_toolbar_position() == TimelineToolbarPosition::Bottom;
        let division = self.current_division_state().base_division;
        let can_zoom_in = self.can_zoom_in();
        let can_zoom_out = self.can_zoom_out();
        let is_zoomed = self.is_zoomed();

        let selection_count = self.selection_fit_count(cx);

        let fit_tooltip = match selection_count {
            0 => "Select timeline items to fit".to_string(),
            1 => "Fit selected item (F)".to_string(),
            count => format!("Fit {count} selected items (F)"),
        };
        let separator = || {
            div().flex_none().bg(cx.theme().colors.hairline).when_else(
                horizontal,
                |rule| rule.w(px(1.)).h(px(20.)).mx(CONTROL_GAP),
                |rule| rule.w(px(20.)).h(px(1.)).my(CONTROL_GAP),
            )
        };

        let frame = div()
            .id("timeline.toolbar")
            .w_full()
            .bg_glass()
            .glass_surface(Surface::Overlay)
            .glass_radius(Radius::Pill)
            .glass(|glass| glass.protect_text_contrast(false))
            .when(cfg!(not(target_os = "macos")), |frame| {
                frame.glass_preset(GlassPreset::Frosted)
            })
            .when_else(
                horizontal,
                |frame| frame.row().flex_wrap(),
                |frame| frame.column(),
            )
            .items_center()
            .gap(CONTROL_GAP)
            .p(px(FOCUS_RING_INSET))
            .rounded(px(cx.theme().radius(Radius::Pill)))
            .border_1()
            .border_color(cx.theme().colors.hairline)
            .child(
                Label::new(division.short_label())
                    .text_xs()
                    .text_color(cx.theme().colors.text_muted)
                    .w(CONTROL_SIZE)
                    .flex_none()
                    .text_center(),
            )
            .child(separator())
            .child(
                control("timeline.zoom-in", AppIcon::ZoomIn, "Zoom in", can_zoom_in)
                    .on_click(cx.listener(|view, _, _, cx| view.zoom_in(cx))),
            )
            .child(
                control(
                    "timeline.zoom-reset",
                    AppIcon::ZoomReset,
                    "Reset zoom",
                    is_zoomed,
                )
                .on_click(cx.listener(|view, _, _, cx| view.zoom_reset(cx))),
            )
            .child(
                control(
                    "timeline.zoom-out",
                    AppIcon::ZoomOut,
                    "Zoom out",
                    can_zoom_out,
                )
                .on_click(cx.listener(|view, _, _, cx| view.zoom_out(cx))),
            )
            .child(
                Button::new("timeline.fit-selection")
                    .ghost()
                    .compact()
                    .size(CONTROL_SIZE)
                    .flex_none()
                    .rounded_full()
                    .label("Fit")
                    .tooltip(fit_tooltip)
                    .disabled(selection_count == 0)
                    .on_click(cx.listener(|view, _, _, cx| view.zoom_to_selection(cx))),
            )
            .child(separator())
            .child(
                control("timeline.scroll-back", AppIcon::MoveUp, "Earlier", true)
                    .on_click(cx.listener(|view, _, _, cx| view.scroll_previous_division(cx))),
            )
            .child(
                control("timeline.scroll-forward", AppIcon::MoveDown, "Later", true)
                    .on_click(cx.listener(|view, _, _, cx| view.scroll_next_division(cx))),
            );

        let viewport_width = self.bounds.map_or(px(800.), |bounds| bounds.size.width);
        div()
            .absolute()
            .column()
            .when_else(
                horizontal,
                |dock| {
                    let (left, width) =
                        bottom_toolbar_layout(viewport_width, self.item_area_left());
                    dock.left(left).bottom(EDGE_INSET).w(width)
                },
                |dock| {
                    dock.top(TOP_EDGE_INSET)
                        .right_0()
                        .bottom_0()
                        .w(px(TIMELINE_TOOLBAR_WIDTH))
                        .justify_center()
                },
            )
            .child(
                div()
                    .w_full()
                    .when(!horizontal, |bar| bar.p_2())
                    .block_mouse_except_scroll()
                    .child(frame),
            )
    }
}
