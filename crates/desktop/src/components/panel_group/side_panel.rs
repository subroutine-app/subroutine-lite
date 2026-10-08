use std::ops::Range;

use gpui::{
    AnyElement, App, InteractiveElement, IntoElement, ParentElement, Pixels, RenderOnce,
    StatefulInteractiveElement, StyleRefinement, Styled, Window, div, px,
};

#[derive(Clone)]
pub struct SidePanelState {
    pub width_range: Range<Pixels>,
    pub opened_proportion: f32,
    pub open: bool,
}

impl Default for SidePanelState {
    fn default() -> Self {
        Self {
            width_range: px(10.)..Pixels::MAX,
            opened_proportion: 0.25,
            open: true,
        }
    }
}

#[derive(IntoElement)]
pub struct SidePanel {
    pub base: gpui::Stateful<gpui::Div>,
    pub width_range: Range<Pixels>,
    pub initial_proportion: f32,
}

impl SidePanel {
    pub fn left() -> Self {
        Self {
            base: div().id("left-panel"),
            width_range: px(10.)..Pixels::MAX,
            initial_proportion: 0.25,
        }
    }

    pub fn width_range_open(mut self, range: Range<Pixels>) -> Self {
        self.width_range = range;
        self
    }
}

impl Styled for SidePanel {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for SidePanel {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for SidePanel {}

impl ParentElement for SidePanel {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.base.extend(elements);
    }
}

impl RenderOnce for SidePanel {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        self.base.size_full()
    }
}
