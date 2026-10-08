use crate::easing::ease_out_cubic;
use std::{rc::Rc, time::Duration};

use crate::AppIcon;
use crate::color::ColorExt;
use crate::icons::Icon;
use crate::icons::IconNamed;
use gpui::{
    Animation, AnimationExt, App, Div, ElementId, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, StatefulInteractiveElement, StyleRefinement, Styled, Transformation, Window, div,
    prelude::FluentBuilder as _, px, relative, rems, size as gpui_size, svg,
};
use gpui_kit::foundation::Disableable;
use gpui_kit::foundation::Selectable;
use gpui_kit::foundation::Sizable;
use gpui_kit::foundation::StyledExt as _;
use gpui_kit_theme::ActiveTheme;
use gpui_kit_theme::ControlSize;

use crate::components::FocusableExt;
use crate::components::ext::StyledRefineExt as _;

const CHECK_ANIMATION_DURATION: Duration = Duration::from_millis(320);

type ClickHandler = dyn Fn(&bool, &mut Window, &mut App) + 'static;

#[derive(IntoElement)]
pub struct Checkbox {
    id: ElementId,
    base: Div,
    style: StyleRefinement,
    checked: bool,
    disabled: bool,
    size: ControlSize,
    tab_stop: bool,
    on_click: Option<Rc<ClickHandler>>,
}

impl Checkbox {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            base: div(),
            style: StyleRefinement::default(),
            checked: false,
            disabled: false,
            size: ControlSize::default(),
            on_click: None,
            tab_stop: true,
        }
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    fn handle_click(
        on_click: &Option<Rc<ClickHandler>>,
        checked: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        let new_checked = !checked;
        if let Some(f) = on_click {
            (f)(&new_checked, window, cx);
        }
    }
}

impl InteractiveElement for Checkbox {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.base.interactivity()
    }
}
impl StatefulInteractiveElement for Checkbox {}

impl Styled for Checkbox {
    fn style(&mut self) -> &mut gpui::StyleRefinement {
        &mut self.style
    }
}

impl Disableable for Checkbox {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl Selectable for Checkbox {
    fn selected(self, selected: bool) -> Self {
        self.checked(selected)
    }
}

impl Sizable for Checkbox {
    fn control_size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }
}

pub(crate) fn checkbox_check_icon(
    id: ElementId,
    size: ControlSize,
    checked: bool,
    disabled: bool,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let toggle_state = window.use_keyed_state(id, cx, |_, _| checked);
    let color = if disabled {
        cx.theme().colors.text.opacity(0.5)
    } else {
        cx.theme().colors.text
    };

    svg()
        .absolute()
        .top_px()
        .left_px()
        .map(|this| match size {
            ControlSize::Xs => this.size_2(),
            ControlSize::Sm => this.size_2p5(),
            ControlSize::Md => this.size_3(),
            ControlSize::Lg | ControlSize::Touch => this.size_3p5(),
        })
        .text_color(color)
        .map(|this| match checked {
            true => this.path(AppIcon::Check.path()),
            _ => this,
        })
        .map(|this| {
            if !disabled && checked != *toggle_state.read(cx) {
                let duration = CHECK_ANIMATION_DURATION;
                cx.spawn({
                    let toggle_state = toggle_state.clone();
                    async move |cx| {
                        cx.background_executor().timer(duration).await;
                        toggle_state.update(cx, |this, _| *this = checked);
                    }
                })
                .detach();

                this.with_animation(
                    ElementId::NamedInteger("toggle".into(), checked as u64),
                    Animation::new(duration),
                    move |this, delta| {
                        let opacity = if checked { delta } else { 1.0 - delta };
                        let scale = 1.0;
                        this.opacity(opacity)
                            .with_transformation(Transformation::scale(gpui_size(scale, scale)))
                    },
                )
                .into_any_element()
            } else {
                this.into_any_element()
            }
        })
}

impl RenderOnce for Checkbox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let checked = self.checked;

        let focus_handle = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let is_focused = focus_handle.is_focused(window) && window.last_input_was_keyboard();

        let base_color = cx.theme().colors.text;
        let bg = gpui::black().mix(gpui::transparent_black(), 0.05);
        let hover = gpui::black().mix(gpui::transparent_black(), 0.1);
        let border = base_color.mix(gpui::transparent_white(), 0.4);
        let hover_border = base_color.mix(gpui::transparent_white(), 0.6);

        let radius = px(cx.theme().radii.control).min(px(4.));

        let checked_border = crate::presentation::UxColor::Stable.color(cx.theme());
        let checked_bg = checked_border.mix(gpui::transparent_black(), 0.12);

        let box_toggle_state = window
            .use_keyed_state((self.id.clone(), "box-toggle"), cx, |_, _| checked)
            .clone();
        let is_box_toggling = !self.disabled && checked != *box_toggle_state.read(cx);

        div().child(
            self.base
                .id(self.id.clone())
                .when(!self.disabled, |this| {
                    this.track_focus(&focus_handle.tab_stop(self.tab_stop).tab_index(0))
                })
                .row()
                .gap_2()
                .items_start()
                .line_height(relative(1.))
                .text_color(cx.theme().colors.text)
                .map(|this| match self.size {
                    ControlSize::Xs => this.text_xs(),
                    ControlSize::Sm => this.text_sm(),
                    ControlSize::Md => this.text_base(),
                    ControlSize::Lg | ControlSize::Touch => this.text_lg(),
                })
                .when(self.disabled, |this| {
                    this.text_color(cx.theme().colors.text_muted)
                })
                .rounded(px(cx.theme().radii.control) * 0.5)
                .focus_ring(is_focused, px(2.), window, cx)
                .refine_style(&self.style)
                .child({
                    let inner_div = div()
                        .id((self.id.clone(), "inner"))
                        .relative()
                        .map(|this| match self.size {
                            ControlSize::Xs => this.size_3(),
                            ControlSize::Sm => this.size_3p5(),
                            ControlSize::Md => this.size_4(),
                            ControlSize::Lg | ControlSize::Touch => this.size(rems(1.125)),
                        })
                        .flex_shrink_0()
                        .border_1()
                        .rounded(radius)
                        .when(!self.disabled, |this| this.shadow_xs())
                        .hover(|s| s.bg(hover).border_color(hover_border))
                        .when(!checked, |this| {
                            this.child(
                                div()
                                    .id((self.id.clone(), "inner-check"))
                                    .absolute()
                                    .flex()
                                    .size_full()
                                    .items_center()
                                    .justify_center()
                                    .child(Icon::new(AppIcon::Check).map(|this| match self.size {
                                        ControlSize::Xs => this.size_2(),
                                        ControlSize::Sm => this.size_2p5(),
                                        ControlSize::Md => this.size_3(),
                                        ControlSize::Lg | ControlSize::Touch => this.size_3p5(),
                                    }))
                                    .text_color(gpui::transparent_black())
                                    .hover(|s| {
                                        s.text_color(cx.theme().colors.text_muted.alpha(0.5))
                                    }),
                            )
                        })
                        .child(checkbox_check_icon(
                            self.id,
                            self.size,
                            checked,
                            self.disabled,
                            window,
                            cx,
                        ));

                    if is_box_toggling {
                        let duration = CHECK_ANIMATION_DURATION;
                        cx.spawn({
                            let box_toggle_state = box_toggle_state.clone();
                            async move |cx| {
                                cx.background_executor().timer(duration).await;
                                box_toggle_state.update(cx, |this, _| *this = checked);
                            }
                        })
                        .detach();

                        inner_div
                            .with_animation(
                                ElementId::NamedInteger("checkbox-box".into(), checked as u64),
                                Animation::new(duration),
                                move |this, delta| {
                                    let t = ease_out_cubic(delta);
                                    let (bg_now, border_now) = if checked {
                                        (bg.mix(checked_bg, t), border.mix(checked_border, t))
                                    } else {
                                        (checked_bg.mix(bg, t), checked_border.mix(border, t))
                                    };
                                    this.bg(bg_now).border_color(border_now)
                                },
                            )
                            .into_any_element()
                    } else {
                        inner_div
                            .bg(if checked { checked_bg } else { bg })
                            .border_color(if checked { checked_border } else { border })
                            .into_any_element()
                    }
                })
                .on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
                    window.prevent_default();
                })
                .when(!self.disabled, |this| {
                    this.on_click({
                        let on_click = self.on_click.clone();
                        move |_, window, cx| {
                            window.prevent_default();
                            Self::handle_click(&on_click, checked, window, cx);
                        }
                    })
                })
                .when(false, |this| this),
        )
    }
}
