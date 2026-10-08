use gpui::{
    AnyElement, App, InteractiveElement, IntoElement, ParentElement, Pixels, RenderOnce,
    StatefulInteractiveElement, StyleRefinement, Styled, Window, div, px,
};

use gpui_kit::foundation::StyledExt as _;
use smallvec::SmallVec;

#[derive(Clone)]
pub struct CenterPanelState {
    pub min_width: Pixels,
}

impl Default for CenterPanelState {
    fn default() -> Self {
        Self {
            min_width: px(500.),
        }
    }
}
#[derive(IntoElement)]
pub struct CenterPanel {
    base: gpui::Stateful<gpui::Div>,
    children: SmallVec<[AnyElement; 8]>,
}

impl CenterPanel {
    pub fn new() -> Self {
        Self {
            base: div().id("center-panel"),
            children: SmallVec::new(),
        }
    }
}

impl Styled for CenterPanel {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for CenterPanel {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for CenterPanel {}

impl ParentElement for CenterPanel {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for CenterPanel {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        self.base
            .size_full()
            .child(div().column().size_full().children(self.children))
    }
}
