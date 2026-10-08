
use crate::components::Button;
use gpui::{
    Div, Hsla, InteractiveElement, Stateful, StatefulInteractiveElement, Styled,
    prelude::FluentBuilder,
};


pub trait LogErr {
    type Ok;
    fn log_err(self) -> Option<Self::Ok>;
}

impl<T, E: std::fmt::Display> LogErr for Result<T, E> {
    type Ok = T;

    fn log_err(self) -> Option<T> {
        match self {
            Ok(value) => Some(value),
            Err(err) => {
                tracing::error!("{err}");
                None
            }
        }
    }
}

#[derive(Clone, Copy)]
pub struct ButtonColors {
    pub bg: Hsla,
    pub fg: Hsla,
    pub hover: Hsla,
    pub active: Hsla,
    pub border: Option<Hsla>,
}


pub trait ButtonColorizeExt {
    fn button_colors(self, colors: ButtonColors) -> Self;
}

impl ButtonColorizeExt for Stateful<Div> {
    fn button_colors(self, colors: ButtonColors) -> Self {
        self.bg(colors.bg)
            .hover(|s| s.bg(colors.hover))
            .active(|s| s.bg(colors.active))
            .when_some(colors.border, |this, color| {
                this.border_1().border_color(color)
            })
    }
}

impl ButtonColorizeExt for Div {
    fn button_colors(self, colors: ButtonColors) -> Self {
        self.bg(colors.bg)
            .hover(|s| s.bg(colors.hover))
            .when_some(colors.border, |this, color| {
                this.border_1().border_color(color)
            })
    }
}

impl ButtonColorizeExt for Button {
    fn button_colors(self, colors: ButtonColors) -> Self {
        self.bg(colors.bg)
            .hover(|s| s.bg(colors.hover))
            .when_some(colors.border, |this, color| {
                this.border_1().border_color(color)
            })
    }
}
