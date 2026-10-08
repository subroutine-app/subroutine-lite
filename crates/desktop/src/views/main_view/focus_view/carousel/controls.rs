use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Styled, div,
    prelude::FluentBuilder as _, px, relative,
};
use gpui_kit::{
    foundation::{Disableable as _, StyledExt as _},
    overlay::{GlassExt as _, GlassPreset},
};
use gpui_kit_semantics::{NodeSpec, Role, Semantic as _};
use gpui_kit_theme::{ActiveTheme, Radius, Surface};

use crate::{
    AppIcon,
    components::{Button, ButtonVariants},
    presentation::UxColor,
    settings::FocusCarouselOrientation,
};

use super::super::{FocusMode, FocusView};

pub(super) struct CarouselNavigation {
    pub(super) has_previous: bool,
    pub(super) has_next: bool,
}

impl FocusView {
    pub(super) fn render_controls(
        &self,
        orientation: FocusCarouselOrientation,
        navigation: CarouselNavigation,
        is_completing: bool,
        progress: f32,
        position: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let action_mode = self.mode == FocusMode::Action;
        let event_position = || {
            div()
                .id("focus-event-position")
                .text_sm()
                .text_color(cx.theme().colors.text_muted)
                .child(position.clone())
                .semantic_in(
                    cx,
                    NodeSpec::new("focus-event-position", Role::Status).text(position.clone()),
                )
        };
        let previous = || {
            Button::new("focus-previous")
                .ghost()
                .compact()
                .size_9()
                .rounded_full()
                .icon(match orientation {
                    FocusCarouselOrientation::Horizontal => AppIcon::ArrowLeft,
                    FocusCarouselOrientation::Vertical => AppIcon::MoveUp,
                })
                .tooltip("Previous")
                .disabled(!navigation.has_previous)
                .on_click(
                    cx.listener(|view, _, window, cx| view.move_active(-1, false, window, cx)),
                )
        };
        let complete_labeled = || {
            Button::new("focus-complete")
                .ghost()
                .h_9()
                .px_3()
                .rounded_full()
                .icon(AppIcon::Check)
                .label(if is_completing {
                    "Completing…"
                } else {
                    "Complete"
                })
                .text_color(UxColor::Action.text(cx.theme()))
                .disabled(is_completing)
                .on_click(cx.listener(|view, _, window, cx| view.complete_active(window, cx)))
        };
        let complete_icon = || {
            Button::new("focus-complete")
                .ghost()
                .compact()
                .size_9()
                .rounded_full()
                .icon(AppIcon::Check)
                .tooltip(if is_completing {
                    "Completing…"
                } else {
                    "Complete"
                })
                .text_color(UxColor::Action.text(cx.theme()))
                .disabled(is_completing)
                .on_click(cx.listener(|view, _, window, cx| view.complete_active(window, cx)))
        };
        let next = || {
            Button::new("focus-next")
                .ghost()
                .compact()
                .size_9()
                .rounded_full()
                .icon(match orientation {
                    FocusCarouselOrientation::Horizontal => AppIcon::ArrowRight,
                    FocusCarouselOrientation::Vertical => AppIcon::MoveDown,
                })
                .tooltip("Next")
                .disabled(!navigation.has_next)
                .on_click(cx.listener(|view, _, window, cx| view.move_active(1, false, window, cx)))
        };
        let frame = match orientation {
            FocusCarouselOrientation::Horizontal => div()
                .column()
                .w(px(216.))
                .items_center()
                .gap_1()
                .p_1()
                .rounded(px(cx.theme().radius(Radius::Control)))
                .border_1()
                .border_color(cx.theme().colors.hairline)
                .child(
                    div()
                        .row()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .child(previous())
                        .when_else(
                            action_mode,
                            |row| row.child(complete_labeled()),
                            |row| row.child(event_position()),
                        )
                        .child(next()),
                )
                .child(
                    div()
                        .relative()
                        .w(px(184.))
                        .h(px(3.))
                        .rounded_full()
                        .overflow_hidden()
                        .bg(cx.theme().colors.hairline)
                        .child(
                            div()
                                .h_full()
                                .w(relative(progress))
                                .rounded_full()
                                .bg(UxColor::Selected.color(cx.theme())),
                        ),
                ),
            FocusCarouselOrientation::Vertical => div()
                .column()
                .w(px(44.))
                .items_center()
                .gap_1()
                .p_1()
                .rounded(px(cx.theme().radius(Radius::Control)))
                .border_1()
                .border_color(cx.theme().colors.hairline)
                .child(previous())
                .when_else(
                    action_mode,
                    |column| column.child(complete_icon()),
                    |column| column.child(event_position()),
                )
                .child(next())
                .child(
                    div()
                        .relative()
                        .w(px(3.))
                        .h(px(112.))
                        .rounded_full()
                        .overflow_hidden()
                        .bg(cx.theme().colors.hairline)
                        .child(
                            div()
                                .absolute()
                                .bottom_0()
                                .w_full()
                                .h(relative(progress))
                                .rounded_full()
                                .bg(UxColor::Selected.color(cx.theme())),
                        ),
                ),
        };

        frame
            .id("focus.controls")
            .bg_glass()
            .glass_surface(Surface::Overlay)
            .glass_radius(Radius::Control)
            .glass(|glass| glass.protect_text_contrast(false))
            .when(cfg!(not(target_os = "macos")), |frame| {
                frame.glass_preset(GlassPreset::Frosted)
            })
            .block_mouse_except_scroll()
            .into_any_element()
    }
}
