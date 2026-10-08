use gpui::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, ParentElement, ScrollHandle,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_kit::{
    controls::button::IconButton,
    foundation::{Disableable as _, Sizable as _, StyledExt as _},
    layout::ScrollArea,
    overlay::{GlassExt as _, GlassPreset, Tooltip},
};
use gpui_kit_assets::Icon as KitIcon;
use gpui_kit_semantics::{NodeSpec, Role, Semantic as _};
use gpui_kit_theme::{ActiveTheme, Radius, Surface};

use crate::{
    components::{Button, ButtonVariants},
    settings::{FocusCarouselOrientation, Settings},
};

use super::{FocusView, temporal};

const FOCUS_TIMING_CHOICES_SECONDS: [i64; 7] =
    [60, 5 * 60, 10 * 60, 15 * 60, 30 * 60, 60 * 60, 2 * 60 * 60];
const FOCUS_HORIZON_CHOICES_HOURS: [u16; 8] = [1, 3, 6, 12, 24, 48, 72, 7 * 24];

fn previous_timing_choice(current: i64) -> Option<i64> {
    FOCUS_TIMING_CHOICES_SECONDS
        .into_iter()
        .rev()
        .find(|choice| *choice < current)
}

fn next_timing_choice(current: i64) -> Option<i64> {
    FOCUS_TIMING_CHOICES_SECONDS
        .into_iter()
        .find(|choice| *choice > current)
}

fn previous_horizon_choice(current: u16) -> Option<u16> {
    FOCUS_HORIZON_CHOICES_HOURS
        .into_iter()
        .rev()
        .find(|choice| *choice < current)
}

fn next_horizon_choice(current: u16) -> Option<u16> {
    FOCUS_HORIZON_CHOICES_HOURS
        .into_iter()
        .find(|choice| *choice > current)
}

impl FocusView {
    pub(super) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.settings_open {
            return;
        }
        self.settings_open = false;
        self.settings_button_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_open {
            self.close_settings(window, cx);
        } else {
            self.settings_open = true;
            cx.notify();
        }
    }

    pub(super) fn render_settings(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let scroll = window
            .use_keyed_state("focus-settings-scroll", cx, |_, _| ScrollHandle::new())
            .read(cx)
            .clone();
        let settings = Settings::global(cx);
        let orientation = settings.focus_carousel_orientation;

        let timing_threshold = settings.focus_timing_threshold_seconds();
        let previous_timing = previous_timing_choice(timing_threshold);
        let next_timing = next_timing_choice(timing_threshold);
        let action_horizon = settings.focus_action_horizon_hours();
        let previous_action_horizon = previous_horizon_choice(action_horizon);
        let next_action_horizon = next_horizon_choice(action_horizon);
        let action_horizon_label = temporal::format_horizon(action_horizon);
        let event_horizon = settings.focus_horizon_hours();
        let previous_event_horizon = previous_horizon_choice(event_horizon);
        let next_event_horizon = next_horizon_choice(event_horizon);
        let event_horizon_label = temporal::format_horizon(event_horizon);

        let panel = div()
            .id("focus-settings")
            .bg_glass()
            .glass_surface(Surface::Overlay)
            .glass_radius(Radius::Dialog)
            .glass(|glass| glass.protect_text_contrast(false))
            .when(cfg!(not(target_os = "macos")), |frame| {
                frame.glass_preset(GlassPreset::Frosted)
            })
            .column()
            .w_80()
            .max_h((self.viewport_size.height - px(32.)).max(px(0.)))
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .gap_4()
            .border_1()
            .border_color(cx.theme().colors.hairline)
            .rounded(px(cx.theme().radii.dialog))
            .block_mouse_except_scroll()
            .p_4()
            .child(
                div()
                    .flex_none()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Focus settings"),
            )
            .child(
                div()
                    .column()
                    .flex_none()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Direction"),
                    )
                    .child(
                        div()
                            .row()
                            .gap_2()
                            .child(
                                Button::new("focus-settings-horizontal")
                                    .outline()
                                    .small()
                                    .flex_1()
                                    .label("Horizontal")
                                    .current(orientation == FocusCarouselOrientation::Horizontal)
                                    .on_click(|_, _, cx| {
                                        Settings::update(cx, |settings| {
                                            settings.focus_carousel_orientation =
                                                FocusCarouselOrientation::Horizontal;
                                        });
                                    }),
                            )
                            .child(
                                Button::new("focus-settings-vertical")
                                    .outline()
                                    .small()
                                    .flex_1()
                                    .label("Vertical")
                                    .current(orientation == FocusCarouselOrientation::Vertical)
                                    .on_click(|_, _, cx| {
                                        Settings::update(cx, |settings| {
                                            settings.focus_carousel_orientation =
                                                FocusCarouselOrientation::Vertical;
                                        });
                                    }),
                            ),
                    ),
            )
            .child(
                div()
                    .row()
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .id("focus-settings-action-horizon-label")
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Action horizon")
                            .tooltip(|_, cx| {
                                Tooltip::new(
                                    "focus-action-horizon-help",
                                    "Show scheduled actions due within this period",
                                )
                                .view(cx)
                            })
                            .semantic_in(
                                cx,
                                NodeSpec::new("focus-settings-action-horizon-label", Role::Text)
                                    .text("Action horizon")
                                    .description("Show scheduled actions due within this period"),
                            ),
                    )
                    .child(
                        div()
                            .row()
                            .items_center()
                            .gap_1()
                            .child(
                                IconButton::new(
                                    "focus-settings-action-horizon-decrease",
                                    KitIcon::Minus,
                                    "Shorten action horizon",
                                )
                                .small()
                                .disabled(previous_action_horizon.is_none())
                                .when_some(
                                    previous_action_horizon,
                                    |button, hours| {
                                        button.on_click(move |_window, cx| {
                                            Settings::update(cx, |settings| {
                                                settings.set_focus_action_horizon_hours(hours)
                                            });
                                        })
                                    },
                                ),
                            )
                            .child(
                                div()
                                    .id("focus-settings-action-horizon-value")
                                    .w(px(64.))
                                    .text_center()
                                    .text_sm()
                                    .child(action_horizon_label.clone())
                                    .semantic_in(
                                        cx,
                                        NodeSpec::new(
                                            "focus-settings-action-horizon-value",
                                            Role::Status,
                                        )
                                        .text(format!("Action horizon: {action_horizon_label}")),
                                    ),
                            )
                            .child(
                                IconButton::new(
                                    "focus-settings-action-horizon-increase",
                                    KitIcon::Plus,
                                    "Lengthen action horizon",
                                )
                                .small()
                                .disabled(next_action_horizon.is_none())
                                .when_some(
                                    next_action_horizon,
                                    |button, hours| {
                                        button.on_click(move |_window, cx| {
                                            Settings::update(cx, |settings| {
                                                settings.set_focus_action_horizon_hours(hours)
                                            });
                                        })
                                    },
                                ),
                            ),
                    ),
            )
            .child(
                div()
                    .row()
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .id("focus-settings-event-horizon-label")
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Event horizon")
                            .tooltip(|_, cx| {
                                Tooltip::new(
                                    "focus-event-horizon-help",
                                    "Show events starting within this period",
                                )
                                .view(cx)
                            })
                            .semantic_in(
                                cx,
                                NodeSpec::new("focus-settings-event-horizon-label", Role::Text)
                                    .text("Event horizon")
                                    .description("Show events starting within this period"),
                            ),
                    )
                    .child(
                        div()
                            .row()
                            .items_center()
                            .gap_1()
                            .child(
                                IconButton::new(
                                    "focus-settings-event-horizon-decrease",
                                    KitIcon::Minus,
                                    "Shorten event horizon",
                                )
                                .small()
                                .disabled(previous_event_horizon.is_none())
                                .when_some(
                                    previous_event_horizon,
                                    |button, hours| {
                                        button.on_click(move |_window, cx| {
                                            Settings::update(cx, |settings| {
                                                settings.set_focus_horizon_hours(hours)
                                            });
                                        })
                                    },
                                ),
                            )
                            .child(
                                div()
                                    .id("focus-settings-event-horizon-value")
                                    .w(px(64.))
                                    .text_center()
                                    .text_sm()
                                    .child(event_horizon_label.clone())
                                    .semantic_in(
                                        cx,
                                        NodeSpec::new(
                                            "focus-settings-event-horizon-value",
                                            Role::Status,
                                        )
                                        .text(format!("Event horizon: {event_horizon_label}")),
                                    ),
                            )
                            .child(
                                IconButton::new(
                                    "focus-settings-event-horizon-increase",
                                    KitIcon::Plus,
                                    "Lengthen event horizon",
                                )
                                .small()
                                .disabled(next_event_horizon.is_none())
                                .when_some(
                                    next_event_horizon,
                                    |button, hours| {
                                        button.on_click(move |_window, cx| {
                                            Settings::update(cx, |settings| {
                                                settings.set_focus_horizon_hours(hours)
                                            });
                                        })
                                    },
                                ),
                            ),
                    ),
            )
            .child(
                div()
                    .row()
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .id("focus-settings-timing-label")
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Timing alerts")
                            .tooltip(|_, cx| {
                                Tooltip::new(
                                    "focus-timing-help",
                                    "Count down to events and signals within this period",
                                )
                                .view(cx)
                            })
                            .semantic_in(
                                cx,
                                NodeSpec::new("focus-settings-timing-label", Role::Text)
                                    .text("Timing alerts")
                                    .description(
                                        "Count down to events and signals within this period",
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .row()
                            .items_center()
                            .gap_1()
                            .child(
                                IconButton::new(
                                    "focus-settings-timing-decrease",
                                    KitIcon::Minus,
                                    "Shorten alert threshold",
                                )
                                .small()
                                .disabled(previous_timing.is_none())
                                .when_some(
                                    previous_timing,
                                    |button, seconds| {
                                        button.on_click(move |_window, cx| {
                                            Settings::update(cx, |settings| {
                                                settings.set_focus_timing_threshold_seconds(seconds)
                                            });
                                        })
                                    },
                                ),
                            )
                            .child(
                                div()
                                    .w(px(64.))
                                    .text_center()
                                    .text_sm()
                                    .child(temporal::format_threshold(timing_threshold)),
                            )
                            .child(
                                IconButton::new(
                                    "focus-settings-timing-increase",
                                    KitIcon::Plus,
                                    "Lengthen alert threshold",
                                )
                                .small()
                                .disabled(next_timing.is_none())
                                .when_some(
                                    next_timing,
                                    |button, seconds| {
                                        button.on_click(move |_window, cx| {
                                            Settings::update(cx, |settings| {
                                                settings.set_focus_timing_threshold_seconds(seconds)
                                            });
                                        })
                                    },
                                ),
                            ),
                    ),
            )
            .child(
                div().row().flex_none().justify_end().child(
                    Button::new("focus-settings-done")
                        .primary()
                        .small()
                        .label("Done")
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.close_settings(window, cx);
                        })),
                ),
            );
        ScrollArea::new("focus-settings-scroll")
            .vertical()
            .width(320. + cx.theme().measures.scrollbar_track)
            .fit_height()
            .label("Focus settings")
            .bound_to(scroll)
            .child(panel)
            .into_any_element()
    }
}
