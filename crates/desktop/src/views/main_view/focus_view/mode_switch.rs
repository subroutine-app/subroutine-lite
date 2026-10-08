use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, MouseButton, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px, relative,
};
use gpui_kit::foundation::{FocusRing as _, StyledExt as _, text};
use gpui_kit_semantics::{NodeSpec, Role, Semantic as _};
use gpui_kit_theme::{ActiveTheme, ControlSize, Radius, Space, TypeScale};

use crate::components::transition::{self, WindowTransitionExt as _};

use super::{FocusMode, FocusView};

fn selection_position(mode: FocusMode) -> f32 {
    match mode {
        FocusMode::Action => 0.0,
        FocusMode::Event => 1.0,
    }
}

fn keyboard_mode(current: FocusMode, key: &str) -> Option<FocusMode> {
    match key {
        "left" | "up" | "home" => Some(FocusMode::Action),
        "right" | "down" | "end" => Some(FocusMode::Event),
        "space" | "enter" => Some(current),
        _ => None,
    }
}

impl FocusView {
    pub(super) fn render_mode_switch(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let metrics = theme.control.get(ControlSize::Sm);
        let gap = theme.space(Space::Xxs);
        let focus = window
            .use_keyed_state("focus-mode.focus", cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let target = selection_position(self.mode);
        let selection =
            window.keyed_transition("focus-mode.selection", cx, transition::QUICK, || target);
        selection.set(target, cx);
        let position = selection.animate(window, cx);

        let indicator = div()
            .absolute()
            .bottom_0()
            .h(px(theme.borders.thick))
            .left(relative(position * 0.5))
            .ml(px(metrics.padding_x + position * gap * 0.5))
            .right(relative((1.0 - position) * 0.5))
            .mr(px(metrics.padding_x + (1.0 - position) * gap * 0.5))
            .rounded_full()
            .bg(theme.colors.accent);

        let segments = FocusMode::ALL.map(|mode| {
            let focus = focus.clone();
            div()
                .id(format!("focus-mode.{}", mode.id()))
                .row()
                .justify_center()
                .min_w_0()
                .h(px(metrics.height))
                .px(px(metrics.padding_x))
                .radius(&theme, Radius::Control)
                .cursor_pointer()
                .hover(|style| style.bg(theme.colors.hover))
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    focus.focus(window, cx);
                })
                .on_click(cx.listener(move |view, _, _, cx| view.set_mode(mode, cx)))
                .child(
                    text(&theme, TypeScale::Label, mode.label())
                        .text_size(px(metrics.font_size))
                        .text_color(if self.mode == mode {
                            theme.colors.text
                        } else {
                            theme.colors.text_muted
                        }),
                )
                .semantic_in(
                    cx,
                    NodeSpec::new(format!("focus-mode.{}", mode.id()), Role::Radio)
                        .parent("focus-mode")
                        .text(mode.label())
                        .checked(self.mode == mode),
                )
        });

        div()
            .semantic_in(
                cx,
                NodeSpec::new("focus-mode", Role::Group).text("Focus content"),
            )
            .track_focus(&focus.tab_stop(true))
            .focus_ring(&theme)
            .flex_none()
            .radius(&theme, Radius::Control)
            .on_key_down(cx.listener(|view, event: &gpui::KeyDownEvent, _, cx| {
                if let Some(mode) = keyboard_mode(view.mode, &event.keystroke.key) {
                    view.set_mode(mode, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .relative()
                    .grid()
                    .grid_cols(2)
                    .gap(px(gap))
                    .children(segments)
                    .child(indicator),
            )
            .into_any_element()
    }
}
