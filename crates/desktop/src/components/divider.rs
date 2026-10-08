use crate::components::ext::StyledRefineExt as _;
use gpui::{
    App, Div, Hsla, IntoElement, ParentElement, PathBuilder, Pixels, RenderOnce, StyleRefinement,
    Styled, Window, canvas, div, point, px,
};
use gpui_kit_theme::ActiveTheme;

#[derive(Clone, Copy, PartialEq, Default)]
pub enum DividerStyle {
    #[default]
    Solid,
    Dashed,
}

#[derive(IntoElement)]
pub struct Divider {
    base: Div,
    stroke: Pixels,
    style: StyleRefinement,
    color: Option<Hsla>,
    line_style: DividerStyle,
}

impl Divider {
    pub fn horizontal() -> Self {
        Self {
            base: div(),
            stroke: px(1.0),
            color: None,
            style: StyleRefinement::default(),
            line_style: DividerStyle::Solid,
        }
    }

    pub fn stroke(mut self, stroke: impl Into<Pixels>) -> Self {
        self.stroke = stroke.into();
        self
    }

    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }

    pub fn dashed(mut self) -> Self {
        self.line_style = DividerStyle::Dashed;
        self
    }

    fn render_base(stroke: Pixels) -> Div {
        div().absolute().h(stroke).w_full()
    }

    fn render_solid(color: Hsla, stroke: Pixels) -> impl IntoElement {
        Self::render_base(stroke).bg(color)
    }

    fn render_dashed(color: Hsla, stroke: Pixels) -> impl IntoElement {
        Self::render_base(stroke).child(
            canvas(
                move |_, _, _| {},
                move |bounds, _, window, _| {
                    let mut builder = PathBuilder::stroke(stroke).dash_array(&[px(4.), px(2.)]);
                    let x = bounds.origin.x;
                    let y = bounds.origin.y + px(0.5);
                    builder.move_to(point(x, y));
                    builder.line_to(point(x + bounds.size.width, y));
                    if let Ok(line) = builder.build() {
                        window.paint_path(line, color);
                    }
                },
            )
            .size_full(),
        )
    }
}

impl Styled for Divider {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Divider {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self.color.unwrap_or(cx.theme().colors.hairline);
        let line_style = self.line_style;
        let stroke = self.stroke;

        self.base
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .refine_style(&self.style)
            .child(match line_style {
                DividerStyle::Solid => Self::render_solid(color, stroke).into_any_element(),
                DividerStyle::Dashed => Self::render_dashed(color, stroke).into_any_element(),
            })
    }
}
