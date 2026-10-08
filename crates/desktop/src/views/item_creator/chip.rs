use std::rc::Rc;

use gpui_kit::foundation::StyledExt as _;

use crate::AppIcon;
use crate::components::Label;
use crate::icons::Icon;
use crate::presentation::RecognitionPaint;
use gpui::{
    App, ClickEvent, ElementId, FontWeight, Hsla, InteractiveElement, IntoElement, ParentElement,
    Pixels, RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder, px,
};
use gpui_kit_theme::{ActiveTheme, Theme};

type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

fn sub_id(id: &ElementId, part: &'static str) -> ElementId {
    ElementId::from((id.clone(), SharedString::new_static(part)))
}

pub const CHIP_HEIGHT: Pixels = px(30.);

const CHIP_BORDER: Pixels = px(1.);

pub const CHIP_WIDTH: Pixels = px(146.);

pub const TOGGLE_CHIP_WIDTH: Pixels = px(92.);

pub const COMPACT_CHIP_WIDTH: Pixels = px(132.);

const ARROW_WIDTH: Pixels = px(28.);

const PROPERTY_ARROW_SIZE: Pixels = px(24.);

const PROPERTY_ROW_HEIGHT: Pixels = px(38.);

const STAGGER: f32 = 0.12;

pub fn stagger(progress: f32, ix: usize) -> f32 {
    let delay = ix as f32 * STAGGER;
    let span = (1.0 - delay).max(0.05);
    ((progress - delay) / span).clamp(0.0, 1.0)
}

#[derive(IntoElement)]
pub struct CreatorChip {
    id: ElementId,
    icon: Option<Icon>,
    label: Option<SharedString>,
    value: Option<SharedString>,
    active: bool,
    selected: bool,
    disabled: bool,
    reveal: f32,
    flash: f32,
    width: Pixels,
    property_row: bool,
    disclosure: Option<bool>,
    on_click: Option<Handler>,
    on_decrement: Option<Handler>,
    on_increment: Option<Handler>,
}

impl CreatorChip {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            icon: None,
            label: None,
            value: None,
            active: false,
            selected: false,
            disabled: false,
            reveal: 1.0,
            flash: 0.0,
            width: CHIP_WIDTH,
            property_row: false,
            disclosure: None,
            on_click: None,
            on_decrement: None,
            on_increment: None,
        }
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = width;
        self
    }

    pub fn property_row(mut self) -> Self {
        self.property_row = true;
        self
    }

    pub fn disclosure(mut self, expanded: bool) -> Self {
        self.disclosure = Some(expanded);
        self
    }

    pub fn icon(mut self, icon: AppIcon) -> Self {
        self.icon = Some(Icon::new(icon));
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn value(mut self, value: impl Into<SharedString>) -> Self {
        self.value = Some(value.into());
        self
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn reveal(mut self, reveal: f32) -> Self {
        self.reveal = reveal.clamp(0.0, 1.0);
        self
    }

    pub fn flash(mut self, flash: f32) -> Self {
        self.flash = flash.clamp(0.0, 1.0);
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    pub fn stepper(
        mut self,
        decrement: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        increment: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_decrement = Some(Rc::new(decrement));
        self.on_increment = Some(Rc::new(increment));
        self
    }
}

#[derive(Debug, PartialEq)]
struct ChipPaint {
    background: Hsla,
    border: Hsla,
    icon: Hsla,
    label: Hsla,
    label_weight: FontWeight,
    hover: Hsla,
}

impl CreatorChip {
    fn paint(&self, theme: &Theme) -> ChipPaint {
        let inactive_text = super::creator_secondary_text(theme);
        let (bg, border, icon_color, label_color) = if self.disabled {
            (
                if self.property_row {
                    theme.colors.raised.alpha(0.28)
                } else {
                    gpui::transparent_black()
                },
                theme.colors.hairline.alpha(0.35),
                theme.colors.text_muted.alpha(0.4),
                theme.colors.text_muted.alpha(0.4),
            )
        } else if self.selected {
            (
                theme.colors.selected,
                theme.colors.focus,
                theme.colors.accent,
                theme.colors.text,
            )
        } else if self.property_row {
            (
                theme
                    .colors
                    .raised
                    .alpha(if self.active { 0.72 } else { 0.52 }),
                gpui::transparent_black(),
                if self.active {
                    theme.colors.text
                } else {
                    inactive_text
                },
                theme.colors.text_muted,
            )
        } else if self.active {
            (
                theme.colors.raised,
                theme.colors.hairline,
                theme.colors.text,
                theme.colors.text,
            )
        } else {
            (
                gpui::transparent_black(),
                theme.colors.hairline,
                inactive_text,
                inactive_text,
            )
        };

        let (bg, border) = if self.flash > 0.0 && !self.disabled {
            let recognition = RecognitionPaint::new(theme);
            (
                bg.blend(recognition.fill.opacity(self.flash)),
                border.blend(recognition.border.opacity(self.flash)),
            )
        } else {
            (bg, border)
        };
        ChipPaint {
            background: bg,
            border,
            icon: icon_color,
            label: label_color,
            label_weight: if self.selected && !self.disabled {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::NORMAL
            },
            hover: if self.selected && !self.disabled {
                theme.colors.raised.blend(theme.colors.hover)
            } else {
                theme.colors.hover
            },
        }
    }
}

impl RenderOnce for CreatorChip {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let has_steppers = self.on_decrement.is_some();
        let split_value = self.property_row && self.label.is_some() && self.value.is_some();
        let ChipPaint {
            background: bg,
            border,
            icon: icon_color,
            label: label_color,
            label_weight,
            hover: hover_bg,
        } = self.paint(theme);
        let chip_height = if self.property_row {
            PROPERTY_ROW_HEIGHT
        } else {
            CHIP_HEIGHT
        };
        let enabled = !self.disabled;
        let arrows_shown = has_steppers && enabled;

        let body = div()
            .id(sub_id(&self.id, "body"))
            .flex()
            .flex_row()
            .items_center()
            .gap_1p5()
            .h_full()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .when_else(
                self.property_row,
                |this| this.px_3(),
                |this| this.px_2p5().when(arrows_shown, |this| this.px_1()),
            )
            .when(enabled && self.on_click.is_some(), |this| {
                this.cursor_pointer()
                    .when(self.property_row, |this| {
                        this.rounded(px(theme.radii.control))
                    })
                    .when(!self.property_row && !arrows_shown, |this| {
                        this.rounded_full()
                    })
                    .hover(|s| s.bg(hover_bg))
            })
            .when_some(self.icon, |this, icon| {
                this.child(icon.size_3p5().flex_none().text_color(icon_color))
            })
            .when_some(self.label.clone(), |this, label| {
                this.child(
                    Label::new(label)
                        .flex_none()
                        .text_xs()
                        .when(self.property_row, |this| this.text_sm())
                        .font_weight(label_weight)
                        .text_color(label_color),
                )
            })
            .when(split_value, |this| this.child(div().flex_1()))
            .when_some(self.value.clone(), |this, value| {
                this.child(
                    Label::new(value)
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .when(self.property_row, |this| this.text_sm())
                        .font_weight(if self.property_row {
                            FontWeight::MEDIUM
                        } else {
                            FontWeight::SEMIBOLD
                        })
                        .text_color(if self.disabled {
                            theme.colors.text_muted.alpha(0.4)
                        } else {
                            theme.colors.text
                        }),
                )
            })
            .when_some(self.disclosure, |this, expanded| {
                this.child(
                    Icon::new(if expanded {
                        AppIcon::ChevronUp
                    } else {
                        AppIcon::ChevronDown
                    })
                    .size_3()
                    .flex_none()
                    .text_color(label_color),
                )
            })
            .when_some(self.on_click.filter(|_| enabled), |this, handler| {
                this.on_click(move |event, window, cx| handler(event, window, cx))
            });

        let decrement = self.on_decrement.filter(|_| enabled);
        let increment = self.on_increment.filter(|_| enabled);

        div()
            .id(self.id.clone())
            .relative()
            .top(px((1.0 - self.reveal) * 10.))
            .when(self.reveal < 1.0, |this| this.opacity(self.reveal))
            .when_else(
                self.property_row,
                |this| this.w_full(),
                |this| this.w(self.width).flex_none(),
            )
            .child(
                div()
                    .row()
                    .w_full()
                    .h(chip_height)
                    .when_else(
                        self.property_row,
                        |this| this.rounded(px(theme.radii.control)),
                        |this| this.rounded_full(),
                    )
                    .when(!self.property_row, |this| {
                        this.border(CHIP_BORDER).border_color(border)
                    })
                    .bg(bg)
                    .overflow_hidden()
                    .when_some(
                        (!self.property_row).then_some(decrement.clone()).flatten(),
                        |this, handler| {
                            this.child(stepper_arrow(
                                sub_id(&self.id, "dec"),
                                ArrowSide::Leading,
                                icon_color,
                                hover_bg,
                                false,
                                handler,
                            ))
                        },
                    )
                    .child(body)
                    .when_some(
                        (!self.property_row).then_some(increment.clone()).flatten(),
                        |this, handler| {
                            this.child(stepper_arrow(
                                sub_id(&self.id, "inc"),
                                ArrowSide::Trailing,
                                icon_color,
                                hover_bg,
                                false,
                                handler,
                            ))
                        },
                    )
                    .when(self.property_row && arrows_shown, |this| {
                        this.child(
                            div()
                                .row()
                                .flex_none()
                                .mr_2()
                                .p_0p5()
                                .gap_0p5()
                                .rounded_full()
                                .bg(theme.colors.canvas.alpha(0.28))
                                .when_some(decrement, |this, handler| {
                                    this.child(stepper_arrow(
                                        sub_id(&self.id, "dec"),
                                        ArrowSide::Leading,
                                        theme.colors.text_muted,
                                        hover_bg,
                                        true,
                                        handler,
                                    ))
                                })
                                .when_some(increment, |this, handler| {
                                    this.child(stepper_arrow(
                                        sub_id(&self.id, "inc"),
                                        ArrowSide::Trailing,
                                        theme.colors.text_muted,
                                        hover_bg,
                                        true,
                                        handler,
                                    ))
                                }),
                        )
                    }),
            )
    }
}

#[derive(Clone, Copy)]
enum ArrowSide {
    Leading,
    Trailing,
}

impl ArrowSide {
    fn icon(self) -> AppIcon {
        match self {
            ArrowSide::Leading => AppIcon::ChevronLeft,
            ArrowSide::Trailing => AppIcon::ChevronRight,
        }
    }
}

fn stepper_arrow(
    id: ElementId,
    side: ArrowSide,
    color: Hsla,
    hover_bg: Hsla,
    property_row: bool,
    handler: Handler,
) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .when_else(
            property_row,
            |this| this.size(PROPERTY_ARROW_SIZE).rounded_full(),
            |this| {
                this.h_full().w(ARROW_WIDTH).map(|this| match side {
                    ArrowSide::Leading => this.rounded_l_full(),
                    ArrowSide::Trailing => this.rounded_r_full(),
                })
            },
        )
        .flex_none()
        .cursor_pointer()
        .hover(|s| s.bg(hover_bg))
        .child(Icon::new(side.icon()).size_3().text_color(color))
        .on_click(move |event, window, cx| handler(event, window, cx))
}
